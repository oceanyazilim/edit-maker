//! Drives `studio-vision` over a media file.

use std::path::Path;

use studio_core::cancel::{report, ProgressSink};
use studio_core::{CancelToken, Rational, StudioError, StudioResult};
use studio_vision::{SceneAnalyzer, VideoAnalysis, VisionConfig};

use crate::frames::{FrameReader, FrameRequest, PixelFormat};
use crate::probe::MediaMetadata;
use crate::tools::FfmpegTools;

/// Analysis frame size: 160 px on the long side, even dimensions.
pub fn analysis_size(display_w: u32, display_h: u32) -> (u32, u32) {
    let long = 160.0;
    let (w, h) = if display_w >= display_h {
        (long, long * display_h as f64 / display_w as f64)
    } else {
        (long * display_w as f64 / display_h as f64, long)
    };
    let even = |v: f64| (((v / 2.0).round() as u32) * 2).max(16);
    (even(w), even(h))
}

pub fn analysis_fps(source: Rational) -> Rational {
    if source.as_f64() <= 15.0 {
        source
    } else {
        Rational::new(15, 1).unwrap()
    }
}

pub fn analyze_video(
    tools: &FfmpegTools,
    path: &Path,
    meta: &MediaMetadata,
    cfg: &VisionConfig,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> StudioResult<VideoAnalysis> {
    let v = meta
        .video
        .as_ref()
        .ok_or_else(|| StudioError::invalid_input("The file has no video stream."))?;
    let (w, h) = analysis_size(v.display_width, v.display_height);
    let fps = analysis_fps(v.fps);
    let req = FrameRequest {
        path: path.to_path_buf(),
        start: studio_core::Tick::ZERO,
        duration: None,
        fps,
        width: w,
        height: h,
        pixel_format: PixelFormat::Rgb24,
        still_image: false,
    };
    let mut reader = FrameReader::open(tools, &req)?;
    let mut an = SceneAnalyzer::new(cfg.clone(), fps, w, h);
    let mut buf = vec![0u8; reader.frame_bytes()];
    let expected = (meta.duration.as_seconds_f64() * fps.as_f64()).max(1.0);
    let mut n = 0u64;
    while reader.read_frame(&mut buf)? {
        an.push(&buf);
        n += 1;
        if n % 30 == 0 {
            cancel.check()?;
            report(
                progress,
                (n as f64 / expected).min(0.99) as f32,
                "Analyzing scenes",
            );
        }
    }
    if n == 0 {
        return Err(StudioError::invalid_media(
            "No video frames could be decoded.",
        ));
    }
    report(progress, 1.0, "Scene analysis complete");
    Ok(an.finish())
}
