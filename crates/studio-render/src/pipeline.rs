//! Export pipeline and single-frame preview rendering.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use studio_core::cancel::{report, ProgressSink};
use studio_core::{CancelToken, ErrorCode, StudioError, StudioResult, Tick};
use studio_media::process::{command, spawn_stderr_tail};
use studio_media::{ffmpeg_file_url, ffmpeg_secs, FfmpegTools};
use studio_timeline::{
    validate, AssetInfo, Clip, EffectKind, Timeline, Track, TrackKind, TransitionKind,
    ValidationSeverity,
};

use crate::compositor::{blend_over, mix, overlay, render_layer, Canvas};
use crate::sources::{RenderAsset, RenderAssetKind, SourceManager};
use crate::state::clip_state;
use crate::verify::{check_output, detect_black, Expected, OutputCheck};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RenderSettings {
    pub output: PathBuf,
    pub encoder: String,
    /// CRF-like quality (lower = better).
    pub quality: u8,
    /// "quality" | "fast" | "draft"
    pub speed_preset: String,
    pub video_bitrate_kbps: Option<u32>,
    pub audio_bitrate_kbps: u32,
    /// Resolution scale (1.0 = timeline resolution; 0.5 for previews).
    pub scale: f32,
    pub overwrite: bool,
    /// Run blackdetect on the result (slower; adds warnings for unintended black).
    pub detect_black: bool,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            output: PathBuf::from("output.mp4"),
            encoder: "libx264".into(),
            quality: 18,
            speed_preset: "quality".into(),
            video_bitrate_kbps: None,
            audio_bitrate_kbps: 192,
            scale: 1.0,
            overwrite: false,
            detect_black: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RenderReport {
    pub output: PathBuf,
    pub frames: u64,
    pub width: u32,
    pub height: u32,
    pub duration: Tick,
    pub encoder: String,
    pub elapsed_secs: f64,
    pub render_fps: f64,
    pub check: OutputCheck,
    pub warnings: Vec<String>,
}

enum Layer<'a> {
    Single(&'a Clip),
    Trans {
        a: &'a Clip,
        b: &'a Clip,
        kind: TransitionKind,
        u: f32,
        cut: Tick,
    },
}

fn active_layer(track: &Track, t: Tick) -> Option<Layer<'_>> {
    let clips = &track.clips;
    let i = clips.iter().position(|c| t >= c.start && t < c.end())?;
    let c = &clips[i];
    if let Some(tr) = &c.transition_in {
        if i > 0 && clips[i - 1].end() == c.start {
            let h = tr.half();
            if h > Tick::ZERO && t < c.start + h {
                let u = (t - (c.start - h)).0 as f32 / (tr.duration.0.max(1)) as f32;
                return Some(Layer::Trans {
                    a: &clips[i - 1],
                    b: c,
                    kind: tr.kind,
                    u,
                    cut: c.start,
                });
            }
        }
    }
    if i + 1 < clips.len() {
        let n = &clips[i + 1];
        if n.start == c.end() {
            if let Some(tr) = &n.transition_in {
                let h = tr.half();
                if h > Tick::ZERO && t >= n.start - h {
                    let u = (t - (n.start - h)).0 as f32 / (tr.duration.0.max(1)) as f32;
                    return Some(Layer::Trans {
                        a: c,
                        b: n,
                        kind: tr.kind,
                        u,
                        cut: n.start,
                    });
                }
            }
        }
    }
    Some(Layer::Single(c))
}

struct Scratch {
    a: Vec<u8>,
    b: Vec<u8>,
    m: Vec<u8>,
}

fn draw(
    sources: &mut SourceManager,
    clip: &Clip,
    t: Tick,
    zoom_mul: f32,
    cw: u32,
    ch: u32,
    out: &mut [u8],
) -> StudioResult<()> {
    let mut st = clip_state(clip, t);
    st.zoom *= zoom_mul;
    let f = sources.frame(clip, t)?;
    render_layer(&f, &st, cw, ch, out);
    Ok(())
}

fn compose(
    tl: &Timeline,
    sources: &mut SourceManager,
    t: Tick,
    canvas: &mut Canvas,
    s: &mut Scratch,
) -> StudioResult<()> {
    let (cw, ch) = (canvas.width, canvas.height);
    canvas.fill(tl.settings.background);
    for track in tl
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Video && !t.hidden)
    {
        let Some(layer) = active_layer(track, t) else {
            continue;
        };
        match layer {
            Layer::Single(c) => {
                draw(sources, c, t, 1.0, cw, ch, &mut s.a)?;
                blend_over(&mut canvas.data, &s.a, 1.0);
            }
            Layer::Trans { a, b, kind, u, cut } => match kind {
                TransitionKind::FlashCut => {
                    let c = if t < cut { a } else { b };
                    draw(sources, c, t, 1.0, cw, ch, &mut s.a)?;
                    overlay(&mut s.a, [255, 255, 255], 1.0 - (2.0 * u - 1.0).abs());
                    blend_over(&mut canvas.data, &s.a, 1.0);
                }
                TransitionKind::DipToBlack => {
                    let (c, alpha) = if t < cut {
                        (a, (2.0 * u).min(1.0))
                    } else {
                        (b, (2.0 - 2.0 * u).clamp(0.0, 1.0))
                    };
                    draw(sources, c, t, 1.0, cw, ch, &mut s.a)?;
                    overlay(&mut s.a, [0, 0, 0], alpha);
                    blend_over(&mut canvas.data, &s.a, 1.0);
                }
                TransitionKind::CrossDissolve | TransitionKind::ZoomBlend => {
                    let zoom = kind == TransitionKind::ZoomBlend;
                    draw(
                        sources,
                        a,
                        t,
                        if zoom { 1.0 + 0.3 * u } else { 1.0 },
                        cw,
                        ch,
                        &mut s.a,
                    )?;
                    draw(
                        sources,
                        b,
                        t,
                        if zoom { 1.0 + 0.3 * (1.0 - u) } else { 1.0 },
                        cw,
                        ch,
                        &mut s.b,
                    )?;
                    mix(&s.a, &s.b, u, &mut s.m);
                    blend_over(&mut canvas.data, &s.m, 1.0);
                }
            },
        }
    }
    Ok(())
}

fn even_scaled(v: u32, scale: f32) -> u32 {
    ((((v as f32 * scale) / 2.0).round() as u32) * 2).max(16)
}

fn asset_infos(assets: &HashMap<String, RenderAsset>) -> HashMap<String, AssetInfo> {
    assets
        .iter()
        .map(|(k, a)| {
            (
                k.clone(),
                AssetInfo {
                    duration: a.duration,
                    has_video: a.kind != RenderAssetKind::Audio,
                    has_audio: a.has_audio,
                    is_image: a.kind == RenderAssetKind::Image,
                },
            )
        })
        .collect()
}

/// Composite a single frame at `t` (preview). Same code path as export.
pub fn render_frame(
    tools: &FfmpegTools,
    tl: &Timeline,
    assets: &HashMap<String, RenderAsset>,
    t: Tick,
    scale: f32,
) -> StudioResult<Canvas> {
    let cw = even_scaled(tl.settings.width, scale);
    let ch = even_scaled(tl.settings.height, scale);
    let mut canvas = Canvas::new(cw, ch, tl.settings.background);
    let n = (cw * ch * 4) as usize;
    let mut s = Scratch {
        a: vec![0; n],
        b: vec![0; n],
        m: vec![0; n],
    };
    let mut sources = SourceManager::new(tools, assets, tl.settings.fps, cw, ch);
    compose(
        tl,
        &mut sources,
        t.snap_to_frame(tl.settings.fps),
        &mut canvas,
        &mut s,
    )?;
    Ok(canvas)
}

/// Encode a canvas as JPEG (preview frames for the UI).
pub fn canvas_to_jpeg(c: &Canvas, quality: u8) -> StudioResult<Vec<u8>> {
    let rgb: Vec<u8> = c
        .data
        .chunks_exact(4)
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    let mut out = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    enc.encode(&rgb, c.width, c.height, image::ExtendedColorType::Rgb8)
        .map_err(|e| StudioError::internal("JPEG encoding failed.").with_details(e.to_string()))?;
    Ok(out)
}

fn intentional_black(tl: &Timeline) -> Vec<(f64, f64)> {
    let mut v = Vec::new();
    for tr in tl.tracks.iter().filter(|t| t.kind == TrackKind::Video) {
        for c in &tr.clips {
            for e in &c.effects {
                if matches!(e.kind, EffectKind::Fade { .. }) {
                    let s = (c.start + e.offset).as_seconds_f64();
                    v.push((s - 0.1, s + e.duration.as_seconds_f64() + 0.1));
                }
            }
            if let Some(tr) = &c.transition_in {
                if tr.kind == TransitionKind::DipToBlack {
                    let h = tr.half().as_seconds_f64();
                    let s = c.start.as_seconds_f64();
                    v.push((s - h - 0.1, s + h + 0.1));
                }
            }
        }
    }
    v
}

pub fn render_timeline(
    tools: &FfmpegTools,
    tl: &Timeline,
    assets: &HashMap<String, RenderAsset>,
    settings: &RenderSettings,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> StudioResult<RenderReport> {
    let started = Instant::now();
    report(progress, 0.0, "Validating project");
    let issues = validate(tl, &asset_infos(assets));
    if let Some(i) = issues
        .iter()
        .find(|i| i.severity == ValidationSeverity::Error)
    {
        return Err(StudioError::validation(format!(
            "The timeline is not renderable: {}",
            i.message
        ))
        .with_details(format!("{issues:?}")));
    }
    for a in assets.values() {
        if !a.path.is_file() {
            return Err(StudioError::not_found("Source media could not be found.")
                .with_details(a.path.display().to_string()));
        }
    }
    if settings.output.exists() && !settings.overwrite {
        return Err(
            StudioError::new(ErrorCode::Io, "The output file already exists.")
                .with_details(settings.output.display().to_string()),
        );
    }
    if let Some(parent) = settings.output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let fps = tl.settings.fps;
    let total = tl.duration().snap_to_frame(fps);
    let n_frames = total.to_frame_round(fps).max(1) as u64;
    let cw = even_scaled(tl.settings.width, settings.scale);
    let ch = even_scaled(tl.settings.height, settings.scale);
    let partial = partial_path(&settings.output);
    let _ = std::fs::remove_file(&partial);

    let audio = crate::audio_graph::build(tl, assets, total, 1);
    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-f".into(),
        "rawvideo".into(),
        "-pix_fmt".into(),
        "rgba".into(),
        "-s".into(),
        format!("{cw}x{ch}"),
        "-framerate".into(),
        fps.to_string(),
        "-i".into(),
        "pipe:0".into(),
    ];
    let vchain = "[0:v]scale=out_color_matrix=bt709:out_range=tv:flags=bicubic,format=yuv420p,setsar=1,setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv[vout]".to_string();
    match &audio {
        Some(g) => {
            args.extend(g.input_args.iter().cloned());
            args.extend([
                "-filter_complex".into(),
                format!("{vchain};{}", g.filter),
                "-map".into(),
                "[vout]".into(),
                "-map".into(),
                format!("[{}]", g.out_label),
            ]);
            args.extend([
                "-c:a".into(),
                "aac".into(),
                "-b:a".into(),
                format!("{}k", settings.audio_bitrate_kbps),
                "-ar".into(),
                "48000".into(),
            ]);
        }
        None => {
            args.extend([
                "-filter_complex".into(),
                vchain,
                "-map".into(),
                "[vout]".into(),
                "-an".into(),
            ]);
        }
    }
    args.extend(crate::encode::video_args(
        &settings.encoder,
        settings.quality,
        &settings.speed_preset,
        settings.video_bitrate_kbps,
    ));
    args.extend([
        "-colorspace".into(),
        "bt709".into(),
        "-color_primaries".into(),
        "bt709".into(),
        "-color_trc".into(),
        "bt709".into(),
        "-color_range".into(),
        "tv".into(),
        "-t".into(),
        ffmpeg_secs(total),
        "-movflags".into(),
        "+faststart".into(),
        ffmpeg_file_url(&partial),
    ]);
    tracing::debug!(target: "render", "encoder args: {:?}", args);
    let mut child = command(&tools.ffmpeg)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            StudioError::new(ErrorCode::ToolMissing, "FFmpeg could not be started.")
                .with_details(e.to_string())
        })?;
    let (tail, tail_h) = spawn_stderr_tail(&mut child);
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| StudioError::internal("No encoder stdin."))?;

    let cleanup = |child: &mut std::process::Child| {
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_file(&partial);
    };

    let mut canvas = Canvas::new(cw, ch, tl.settings.background);
    let nbytes = (cw * ch * 4) as usize;
    let mut scratch = Scratch {
        a: vec![0; nbytes],
        b: vec![0; nbytes],
        m: vec![0; nbytes],
    };
    let mut sources = SourceManager::new(tools, assets, fps, cw, ch);
    let clip_ends: HashMap<String, Tick> = tl
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .map(|c| {
            (
                c.id.clone(),
                c.end()
                    + c.transition_in
                        .as_ref()
                        .map(|t| t.half())
                        .unwrap_or(Tick::ZERO),
            )
        })
        .collect();
    let keep = Tick::from_seconds_f64(2.0);

    for n in 0..n_frames {
        if cancel.is_cancelled() {
            drop(stdin);
            cleanup(&mut child);
            return Err(StudioError::cancelled());
        }
        let t = Tick::from_frame(n as i64, fps);
        if let Err(e) = compose(tl, &mut sources, t, &mut canvas, &mut scratch) {
            drop(stdin);
            cleanup(&mut child);
            return Err(e);
        }
        if let Err(e) = stdin.write_all(&canvas.data) {
            drop(stdin);
            let _ = child.wait();
            if let Some(h) = tail_h {
                let _ = h.join();
            }
            let details = tail.text();
            let _ = std::fs::remove_file(&partial);
            return Err(studio_media::process::classify_ffmpeg_error(&details)
                .with_details(format!("{e}\n{details}")));
        }
        if n % 15 == 0 {
            sources.release_before(&clip_ends, t, keep);
            report(progress, 0.95 * (n as f32 / n_frames as f32), "Rendering");
        }
    }
    drop(stdin);
    let status = child.wait()?;
    if let Some(h) = tail_h {
        let _ = h.join();
    }
    if !status.success() {
        let details = tail.text();
        let _ = std::fs::remove_file(&partial);
        return Err(studio_media::process::classify_ffmpeg_error(&details).with_details(details));
    }
    let elapsed_render = started.elapsed().as_secs_f64();

    report(progress, 0.96, "Validating output");
    let mut check = check_output(
        tools,
        &partial,
        &Expected {
            duration: total,
            width: cw,
            height: ch,
            fps,
            has_audio: audio.is_some(),
        },
    )?;
    let mut warnings = Vec::new();
    if settings.detect_black {
        let blacks = detect_black(tools, &partial, 0.5, cancel)?;
        let intentional = intentional_black(tl);
        for (s, e) in &blacks {
            let covered = intentional.iter().any(|(a, b)| *s >= *a && *e <= *b);
            if !covered {
                warnings.push(format!("Unintended black segment {:.2}s–{:.2}s.", s, e));
            }
        }
        check.black_segments = blacks;
    }
    if !check.ok {
        let _ = std::fs::remove_file(&partial);
        return Err(StudioError::new(
            ErrorCode::ProcessFailed,
            "The rendered file failed validation.",
        )
        .with_details(check.problems.join("\n")));
    }
    if settings.output.exists() && settings.overwrite {
        std::fs::remove_file(&settings.output)?;
    }
    std::fs::rename(&partial, &settings.output)?;
    report(progress, 1.0, "Render complete");
    Ok(RenderReport {
        output: settings.output.clone(),
        frames: n_frames,
        width: cw,
        height: ch,
        duration: total,
        encoder: settings.encoder.clone(),
        elapsed_secs: started.elapsed().as_secs_f64(),
        render_fps: n_frames as f64 / elapsed_render.max(1e-6),
        check,
        warnings,
    })
}

pub fn partial_path(out: &Path) -> PathBuf {
    let stem = out
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "render".into());
    out.with_file_name(format!("{stem}.partial.mp4"))
}

/// Export only the mixed audio of the timeline (same sample-accurate graph as video export).
/// `format`: "m4a" (AAC) or "wav" (PCM 16-bit). Output is validated before it is moved into place.
#[allow(clippy::too_many_arguments)]
pub fn render_audio_only(
    tools: &FfmpegTools,
    tl: &Timeline,
    assets: &HashMap<String, RenderAsset>,
    output: &Path,
    format: &str,
    bitrate_kbps: u32,
    overwrite: bool,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> StudioResult<PathBuf> {
    if output.exists() && !overwrite {
        return Err(
            StudioError::new(ErrorCode::Io, "The output file already exists.")
                .with_details(output.display().to_string()),
        );
    }
    let total = tl.duration().snap_to_frame(tl.settings.fps);
    let graph = crate::audio_graph::build(tl, assets, total, 0)
        .ok_or_else(|| StudioError::validation("The timeline has no audible audio clips."))?;
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let ext = if format == "wav" { "wav" } else { "m4a" };
    let partial = output.with_extension(format!("partial.{ext}"));
    let _ = std::fs::remove_file(&partial);
    let mut args = graph.input_args.clone();
    args.extend([
        "-filter_complex".into(),
        graph.filter.clone(),
        "-map".into(),
        format!("[{}]", graph.out_label),
    ]);
    if ext == "wav" {
        args.extend(["-c:a".into(), "pcm_s16le".into()]);
    } else {
        args.extend([
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            format!("{bitrate_kbps}k"),
        ]);
    }
    args.extend([
        "-ar".into(),
        "48000".into(),
        "-t".into(),
        ffmpeg_secs(total),
        ffmpeg_file_url(&partial),
    ]);
    let r = studio_media::process::run_ffmpeg(
        &tools.ffmpeg,
        &args,
        Some(total),
        cancel,
        progress,
        "Rendering audio",
    );
    if let Err(e) = r {
        let _ = std::fs::remove_file(&partial);
        return Err(e);
    }
    let m = studio_media::probe(tools, &partial)?;
    if !m.has_audio() || (m.duration - total).abs() > Tick::from_millis(100) {
        let _ = std::fs::remove_file(&partial);
        return Err(StudioError::new(
            ErrorCode::ProcessFailed,
            "The rendered audio failed validation.",
        )
        .with_details(format!(
            "duration {:.3}s, expected {:.3}s",
            m.duration.as_seconds_f64(),
            total.as_seconds_f64()
        )));
    }
    if output.exists() {
        std::fs::remove_file(output)?;
    }
    std::fs::rename(&partial, output)?;
    report(progress, 1.0, "Audio export complete");
    Ok(output.to_path_buf())
}

/// Export one composited frame (same compositor as preview/export) as PNG or JPEG.
pub fn render_still(
    tools: &FfmpegTools,
    tl: &Timeline,
    assets: &HashMap<String, RenderAsset>,
    t: Tick,
    output: &Path,
    overwrite: bool,
) -> StudioResult<PathBuf> {
    if output.exists() && !overwrite {
        return Err(
            StudioError::new(ErrorCode::Io, "The output file already exists.")
                .with_details(output.display().to_string()),
        );
    }
    let c = render_frame(tools, tl, assets, t, 1.0)?;
    let rgb: Vec<u8> = c
        .data
        .chunks_exact(4)
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    let img = image::RgbImage::from_raw(c.width, c.height, rgb)
        .ok_or_else(|| StudioError::internal("Frame buffer size mismatch."))?;
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let ext = output
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let fmt = match ext.as_str() {
        "png" => image::ImageFormat::Png,
        "jpg" | "jpeg" => image::ImageFormat::Jpeg,
        _ => {
            return Err(StudioError::invalid_input(
                "Still frames can be saved as .png or .jpg.",
            ))
        }
    };
    img.save_with_format(output, fmt).map_err(|e| {
        StudioError::new(ErrorCode::Io, "Could not write the image.").with_details(e.to_string())
    })?;
    Ok(output.to_path_buf())
}
