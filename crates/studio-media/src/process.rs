//! Managed child processes with cancellation, progress and stderr capture.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use studio_core::cancel::{report, ProgressSink};
use studio_core::{CancelToken, ErrorCode, StudioError, StudioResult, Tick};

/// A `Command` that never opens a console window on Windows.
pub fn command(program: &Path) -> Command {
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c.stdin(Stdio::null());
    c
}

/// Keeps the last N lines of a stream.
#[derive(Clone, Default)]
pub struct Tail(Arc<Mutex<VecDeque<String>>>);

impl Tail {
    pub fn push(&self, line: String) {
        let mut g = self.0.lock().unwrap();
        if g.len() >= 40 {
            g.pop_front();
        }
        g.push_back(line);
    }
    pub fn text(&self) -> String {
        self.0
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub fn spawn_stderr_tail(child: &mut Child) -> (Tail, Option<std::thread::JoinHandle<()>>) {
    let tail = Tail::default();
    let h = child.stderr.take().map(|err| {
        let t = tail.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                t.push(line);
            }
        })
    });
    (tail, h)
}

/// Run FFmpeg to completion. `total` (output duration) enables fractional progress
/// parsed from `-progress pipe:1`. Kills the process on cancellation.
pub fn run_ffmpeg(
    ffmpeg: &Path,
    args: &[String],
    total: Option<Tick>,
    cancel: &CancelToken,
    progress: &ProgressSink,
    stage: &str,
) -> StudioResult<()> {
    let mut full: Vec<String> = vec![
        "-hide_banner".into(),
        "-nostdin".into(),
        "-y".into(),
        "-progress".into(),
        "pipe:1".into(),
        "-nostats".into(),
    ];
    full.extend(args.iter().cloned());
    tracing::debug!(target: "ffmpeg", "ffmpeg {:?}", full);
    let mut child = command(ffmpeg)
        .args(&full)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            StudioError::new(ErrorCode::ToolMissing, "FFmpeg could not be started.")
                .with_details(e.to_string())
        })?;
    let (tail, tail_h) = spawn_stderr_tail(&mut child);
    let stdout = child.stdout.take();
    let prog = progress.clone();
    let stage_s = stage.to_string();
    let out_h = stdout.map(|o| {
        std::thread::spawn(move || {
            for line in BufReader::new(o).lines().map_while(Result::ok) {
                if let Some(v) = line
                    .strip_prefix("out_time_us=")
                    .or_else(|| line.strip_prefix("out_time_ms="))
                {
                    if let (Ok(us), Some(total)) = (v.trim().parse::<i64>(), total) {
                        let frac = (us as f64 / 1e6) / total.as_seconds_f64().max(1e-6);
                        report(&prog, frac as f32, &stage_s);
                    }
                }
            }
        })
    });
    let status = loop {
        if cancel.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(StudioError::cancelled());
        }
        match child.try_wait()? {
            Some(s) => break s,
            None => std::thread::sleep(Duration::from_millis(40)),
        }
    };
    if let Some(h) = out_h {
        let _ = h.join();
    }
    if let Some(h) = tail_h {
        let _ = h.join();
    }
    if !status.success() {
        let details = tail.text();
        return Err(classify_ffmpeg_error(&details).with_details(details));
    }
    Ok(())
}

/// Map common FFmpeg failures to user-presentable messages.
pub fn classify_ffmpeg_error(stderr: &str) -> StudioError {
    let s = stderr.to_ascii_lowercase();
    if s.contains("no space left") {
        StudioError::new(ErrorCode::Io, "Not enough disk space.")
    } else if s.contains("unknown encoder")
        || s.contains("cannot load nvcuda")
        || s.contains("no nvenc capable devices")
        || s.contains("failed to initialise")
        || s.contains("error while opening encoder")
    {
        StudioError::new(
            ErrorCode::EncoderUnavailable,
            "Rendering failed because the selected encoder is unavailable.",
        )
    } else if s.contains("no such file or directory") {
        StudioError::not_found("Source media could not be found.")
    } else if s.contains("invalid data found") || s.contains("moov atom not found") {
        StudioError::invalid_media("The media file is corrupted or not supported.")
    } else if s.contains("permission denied") {
        StudioError::new(ErrorCode::Io, "Permission denied while accessing a file.")
    } else {
        StudioError::new(ErrorCode::ProcessFailed, "Media processing failed.")
    }
}
