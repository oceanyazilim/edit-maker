//! Streaming raw frame reader (FFmpeg decode → rawvideo pipe).
//!
//! Frames are conformed to a constant rate with the `fps` filter, so frame `k`
//! of a request corresponds to source time `start + k / fps` – this is how
//! variable-frame-rate sources are mapped onto the project timebase.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, ChildStdout, Stdio};

use studio_core::{ErrorCode, Rational, StudioError, StudioResult, Tick};

use crate::process::{command, spawn_stderr_tail, Tail};
use crate::tools::FfmpegTools;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Rgb24,
    Rgba,
}

impl PixelFormat {
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            PixelFormat::Rgb24 => 3,
            PixelFormat::Rgba => 4,
        }
    }
    fn ffmpeg_name(self) -> &'static str {
        match self {
            PixelFormat::Rgb24 => "rgb24",
            PixelFormat::Rgba => "rgba",
        }
    }
}

#[derive(Clone, Debug)]
pub struct FrameRequest {
    pub path: PathBuf,
    /// Source time of the first frame.
    pub start: Tick,
    /// Optional maximum duration (source time).
    pub duration: Option<Tick>,
    pub fps: Rational,
    /// Output size; the source is scaled to exactly this size (callers preserve aspect).
    pub width: u32,
    pub height: u32,
    pub pixel_format: PixelFormat,
    /// Loop a still image (for image assets).
    pub still_image: bool,
}

pub struct FrameReader {
    child: Child,
    stdout: ChildStdout,
    frame_bytes: usize,
    tail: Tail,
    pub frames_read: u64,
}

impl FrameReader {
    pub fn open(tools: &FfmpegTools, req: &FrameRequest) -> StudioResult<FrameReader> {
        if req.width == 0 || req.height == 0 {
            return Err(StudioError::invalid_input("Frame size must be positive."));
        }
        let mut args: Vec<String> = vec![
            "-hide_banner".into(),
            "-nostdin".into(),
            "-loglevel".into(),
            "error".into(),
        ];
        if req.still_image {
            args.extend([
                "-loop".into(),
                "1".into(),
                "-framerate".into(),
                req.fps.to_string(),
            ]);
        } else if req.start > Tick::ZERO {
            args.extend(["-ss".into(), crate::ffmpeg_secs(req.start)]);
        }
        args.extend(["-i".into(), crate::ffmpeg_file_url(&req.path)]);
        if let Some(d) = req.duration {
            args.extend(["-t".into(), crate::ffmpeg_secs(d)]);
        }
        let vf = format!(
            "fps={},scale={}:{}:flags=bicubic,setsar=1,format={}",
            req.fps,
            req.width,
            req.height,
            req.pixel_format.ffmpeg_name()
        );
        args.extend([
            "-vf".into(),
            vf,
            "-an".into(),
            "-sn".into(),
            "-f".into(),
            "rawvideo".into(),
            "-pix_fmt".into(),
            req.pixel_format.ffmpeg_name().into(),
            "pipe:1".into(),
        ]);
        let mut child = command(&tools.ffmpeg)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                StudioError::new(ErrorCode::ToolMissing, "FFmpeg could not be started.")
                    .with_details(e.to_string())
            })?;
        let (tail, _h) = spawn_stderr_tail(&mut child);
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| StudioError::internal("No stdout from FFmpeg."))?;
        Ok(FrameReader {
            child,
            stdout,
            frame_bytes: req.width as usize
                * req.height as usize
                * req.pixel_format.bytes_per_pixel(),
            tail,
            frames_read: 0,
        })
    }

    pub fn frame_bytes(&self) -> usize {
        self.frame_bytes
    }

    /// Fill `buf` with the next frame. Returns `Ok(false)` at end of stream.
    pub fn read_frame(&mut self, buf: &mut [u8]) -> StudioResult<bool> {
        debug_assert_eq!(buf.len(), self.frame_bytes);
        let mut filled = 0;
        while filled < buf.len() {
            match self.stdout.read(&mut buf[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
        }
        if filled == 0 {
            // EOF: check exit status for errors.
            if let Ok(Some(st)) = self.child.try_wait() {
                if !st.success() && self.frames_read == 0 {
                    let t = self.tail.text();
                    return Err(crate::process::classify_ffmpeg_error(&t).with_details(t));
                }
            } else if let Ok(st) = self.child.wait() {
                if !st.success() && self.frames_read == 0 {
                    let t = self.tail.text();
                    return Err(crate::process::classify_ffmpeg_error(&t).with_details(t));
                }
            }
            return Ok(false);
        }
        if filled < buf.len() {
            // Truncated last frame: treat as end of stream.
            return Ok(false);
        }
        self.frames_read += 1;
        Ok(true)
    }

    pub fn stderr_tail(&self) -> String {
        self.tail.text()
    }
}

impl Drop for FrameReader {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
