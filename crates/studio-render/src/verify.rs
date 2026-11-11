//! Output validation: the rendered file is probed and compared with the timeline.

use std::path::Path;

use serde::{Deserialize, Serialize};
use studio_core::{CancelToken, Rational, StudioResult, Tick};
use studio_media::process::command;
use studio_media::{ffmpeg_file_url, probe, FfmpegTools};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OutputCheck {
    pub ok: bool,
    pub duration: Tick,
    pub width: u32,
    pub height: u32,
    pub fps: Option<Rational>,
    pub has_audio: bool,
    pub problems: Vec<String>,
    /// Black segments ≥ 0.5 s found by blackdetect (start, end).
    pub black_segments: Vec<(f64, f64)>,
}

pub struct Expected {
    pub duration: Tick,
    pub width: u32,
    pub height: u32,
    pub fps: Rational,
    pub has_audio: bool,
}

pub fn check_output(tools: &FfmpegTools, path: &Path, exp: &Expected) -> StudioResult<OutputCheck> {
    let m = probe(tools, path)?;
    let mut problems = Vec::new();
    let v = m.video.as_ref();
    let (w, h) = v.map(|v| (v.width, v.height)).unwrap_or((0, 0));
    if v.is_none() {
        problems.push("Output has no video stream.".into());
    }
    if (w, h) != (exp.width, exp.height) {
        problems.push(format!(
            "Resolution {w}x{h} does not match expected {}x{}.",
            exp.width, exp.height
        ));
    }
    let fps = v.map(|v| v.fps);
    if let Some(f) = fps {
        if (f.as_f64() - exp.fps.as_f64()).abs() > 0.01 {
            problems.push(format!(
                "Frame rate {} does not match expected {}.",
                f, exp.fps
            ));
        }
    }
    // Container duration may include encoder padding; allow one frame + 50 ms.
    let tol = exp.fps.frame_duration() + Tick::from_millis(50);
    if (m.duration - exp.duration).abs() > tol {
        problems.push(format!(
            "Duration {:.3}s differs from expected {:.3}s.",
            m.duration.as_seconds_f64(),
            exp.duration.as_seconds_f64()
        ));
    }
    if exp.has_audio && !m.has_audio() {
        problems.push("Output is missing its audio stream.".into());
    }
    Ok(OutputCheck {
        ok: problems.is_empty(),
        duration: m.duration,
        width: w,
        height: h,
        fps,
        has_audio: m.has_audio(),
        problems,
        black_segments: vec![],
    })
}

/// Run FFmpeg's blackdetect and return black segments of at least `min_secs`.
pub fn detect_black(
    tools: &FfmpegTools,
    path: &Path,
    min_secs: f64,
    cancel: &CancelToken,
) -> StudioResult<Vec<(f64, f64)>> {
    cancel.check()?;
    let out = command(&tools.ffmpeg)
        .args(["-hide_banner", "-nostdin", "-i"])
        .arg(ffmpeg_file_url(path))
        .args([
            "-vf",
            &format!("blackdetect=d={min_secs}:pix_th=0.06"),
            "-an",
            "-f",
            "null",
            "-",
        ])
        .output()?;
    let err = String::from_utf8_lossy(&out.stderr);
    let mut segs = Vec::new();
    for line in err.lines().filter(|l| l.contains("black_start")) {
        let get = |k: &str| {
            line.split_whitespace()
                .find_map(|t| t.strip_prefix(k))
                .and_then(|v| v.parse::<f64>().ok())
        };
        if let (Some(s), Some(e)) = (get("black_start:"), get("black_end:")) {
            segs.push((s, e));
        }
    }
    Ok(segs)
}
