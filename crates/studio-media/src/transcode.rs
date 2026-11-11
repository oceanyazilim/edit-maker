//! Thumbnails, proxies and audio extraction.

use std::path::Path;

use studio_core::cancel::ProgressSink;
use studio_core::{CancelToken, StudioResult, Tick};

use crate::process::run_ffmpeg;
use crate::tools::FfmpegTools;
use crate::{ffmpeg_file_url, ffmpeg_secs};

/// Write a JPEG thumbnail of the frame at `t`, `width` px wide.
pub fn thumbnail(
    tools: &FfmpegTools,
    input: &Path,
    t: Tick,
    width: u32,
    out: &Path,
    cancel: &CancelToken,
) -> StudioResult<()> {
    let args = vec![
        "-ss".into(),
        ffmpeg_secs(t),
        "-i".into(),
        ffmpeg_file_url(input),
        "-frames:v".into(),
        "1".into(),
        "-vf".into(),
        format!("scale={width}:-2:flags=bicubic"),
        "-q:v".into(),
        "4".into(),
        ffmpeg_file_url(out),
    ];
    run_ffmpeg(
        &tools.ffmpeg,
        &args,
        None,
        cancel,
        &studio_core::cancel::noop_progress(),
        "Thumbnail",
    )
}

/// Low-resolution, edit-friendly proxy (short GOP H.264 + AAC).
pub fn proxy(
    tools: &FfmpegTools,
    input: &Path,
    out: &Path,
    height: u32,
    duration: Tick,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> StudioResult<()> {
    let tmp = out.with_extension("partial.mp4");
    let args = vec![
        "-i".into(),
        ffmpeg_file_url(input),
        "-map".into(),
        "0:v:0".into(),
        "-map".into(),
        "0:a:0?".into(),
        "-vf".into(),
        format!("scale=-2:{height}:flags=bicubic,format=yuv420p"),
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "veryfast".into(),
        "-crf".into(),
        "26".into(),
        "-g".into(),
        "15".into(),
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "128k".into(),
        "-movflags".into(),
        "+faststart".into(),
        ffmpeg_file_url(&tmp),
    ];
    run_ffmpeg(
        &tools.ffmpeg,
        &args,
        Some(duration),
        cancel,
        progress,
        "Generating proxy",
    )?;
    std::fs::rename(&tmp, out)?;
    Ok(())
}

/// Decode the first audio stream to 16-bit PCM WAV (fallback when the native decoder
/// does not support a codec, e.g. Opus/AC-3 in video containers).
pub fn extract_audio_wav(
    tools: &FfmpegTools,
    input: &Path,
    out: &Path,
    cancel: &CancelToken,
) -> StudioResult<()> {
    let args = vec![
        "-i".into(),
        ffmpeg_file_url(input),
        "-map".into(),
        "0:a:0".into(),
        "-vn".into(),
        "-c:a".into(),
        "pcm_s16le".into(),
        ffmpeg_file_url(out),
    ];
    run_ffmpeg(
        &tools.ffmpeg,
        &args,
        None,
        cancel,
        &studio_core::cancel::noop_progress(),
        "Extracting audio",
    )
}

/// Generate a synthetic test video (used by integration tests and diagnostics).
/// `segments`: (duration_secs, lavfi source expression).
pub fn synth_test_video(
    tools: &FfmpegTools,
    out: &Path,
    width: u32,
    height: u32,
    fps: u32,
    segments: &[(f64, String)],
    cancel: &CancelToken,
) -> StudioResult<()> {
    let mut args: Vec<String> = Vec::new();
    for (d, src) in segments {
        // lavfi syntax: `name=opt=v:opt=v`; the first option follows '=' when the source has none.
        let sep = if src.contains('=') { ':' } else { '=' };
        args.extend([
            "-f".into(),
            "lavfi".into(),
            "-t".into(),
            format!("{d}"),
            "-i".into(),
            format!("{src}{sep}size={width}x{height}:rate={fps}"),
        ]);
    }
    let n = segments.len();
    let inputs: String = (0..n).map(|i| format!("[{i}:v]")).collect();
    args.extend([
        "-filter_complex".into(),
        format!("{inputs}concat=n={n}:v=1:a=0,format=yuv420p[v]"),
        "-map".into(),
        "[v]".into(),
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "veryfast".into(),
        "-crf".into(),
        "23".into(),
        ffmpeg_file_url(out),
    ]);
    run_ffmpeg(
        &tools.ffmpeg,
        &args,
        None,
        cancel,
        &studio_core::cancel::noop_progress(),
        "Generating test video",
    )
}
