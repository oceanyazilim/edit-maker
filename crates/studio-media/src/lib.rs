//! Media Engine.
//!
//! FFmpeg and FFprobe are used as managed external executables (see ADR-0002).
//! Every invocation:
//!   * passes arguments as a vector (never through a shell),
//!   * prefixes local paths with `file:` so names cannot be interpreted as
//!     protocols or options,
//!   * runs without a console window on Windows,
//!   * is cancellable and captures the stderr tail for diagnostics.

pub mod analyze;
pub mod encoders;
pub mod fingerprint;
pub mod frames;
pub mod probe;
pub mod process;
pub mod tools;
pub mod transcode;

pub use encoders::{detect_encoders, EncoderInfo, VideoCodec};
pub use fingerprint::fingerprint;
pub use frames::{FrameReader, FrameRequest, PixelFormat};
pub use probe::{probe, AudioStreamInfo, MediaKind, MediaMetadata, VideoStreamInfo};
pub use tools::FfmpegTools;

/// Format a local path as an FFmpeg URL that can only mean "this local file".
pub fn ffmpeg_file_url(p: &std::path::Path) -> String {
    format!("file:{}", p.display())
}

/// Seconds with microsecond precision for FFmpeg time arguments.
pub fn ffmpeg_secs(t: studio_core::Tick) -> String {
    format!("{:.6}", t.as_seconds_f64().max(0.0))
}

/// Validate a user-supplied input path: must exist, be a regular file, and is returned
/// in canonical absolute form (no relative components, no traversal ambiguity).
pub fn validate_input_path(p: &std::path::Path) -> studio_core::StudioResult<std::path::PathBuf> {
    let c = std::fs::canonicalize(p).map_err(|_| {
        studio_core::StudioError::not_found("Source media could not be found.")
            .with_details(p.display().to_string())
    })?;
    let md = std::fs::metadata(&c)?;
    if !md.is_file() {
        return Err(studio_core::StudioError::invalid_input(
            "The selected path is not a file.",
        ));
    }
    // Strip the Windows verbatim prefix for tools that do not understand it.
    let s = c.display().to_string();
    let s = s.strip_prefix(r"\\?\").map(|x| x.to_string()).unwrap_or(s);
    Ok(std::path::PathBuf::from(s))
}
