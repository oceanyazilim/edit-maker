//! Locating and verifying FFmpeg / FFprobe executables.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use studio_core::{ErrorCode, StudioError, StudioResult};

use crate::process::command;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FfmpegTools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub version: String,
    /// Where the tools were found (env, bundled, dev, path).
    pub source: String,
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn candidate_dirs(extra: &[PathBuf]) -> Vec<(PathBuf, &'static str)> {
    let mut v: Vec<(PathBuf, &'static str)> = Vec::new();
    if let Some(d) = std::env::var_os("STUDIO_FFMPEG_DIR") {
        v.push((PathBuf::from(d), "env"));
    }
    for d in extra {
        v.push((d.clone(), "configured"));
    }
    // Bundled next to the executable (Tauri sidecar / installer layout).
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(dir) = exe_path.parent() {
            v.push((dir.to_path_buf(), "bundled"));
            v.push((dir.join("ffmpeg"), "bundled"));
            // Development: walk up to find third_party/ffmpeg.
            for anc in dir.ancestors().take(8) {
                v.push((anc.join("third_party").join("ffmpeg"), "dev"));
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        for anc in cwd.ancestors().take(6) {
            v.push((anc.join("third_party").join("ffmpeg"), "dev"));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&path) {
            v.push((d, "path"));
        }
    }
    // Windows Package Manager installs (e.g. `winget install Gyan.FFmpeg`): found even when
    // the current process was started before PATH was updated.
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let winget = PathBuf::from(local).join("Microsoft").join("WinGet");
        v.push((winget.join("Links"), "winget"));
        if let Ok(rd) = std::fs::read_dir(winget.join("Packages")) {
            for pkg in rd.flatten().filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .contains("ffmpeg")
            }) {
                if let Ok(inner) = std::fs::read_dir(pkg.path()) {
                    for build in inner.flatten() {
                        v.push((build.path().join("bin"), "winget"));
                    }
                }
            }
        }
    }
    v
}

impl FfmpegTools {
    /// Locate FFmpeg/FFprobe. `extra_dirs` are user-configured locations (Settings).
    pub fn locate(extra_dirs: &[PathBuf]) -> StudioResult<FfmpegTools> {
        for (dir, source) in candidate_dirs(extra_dirs) {
            let ff = dir.join(exe("ffmpeg"));
            let fp = dir.join(exe("ffprobe"));
            if ff.is_file() && fp.is_file() {
                if let Ok(version) = Self::version_of(&ff) {
                    return Ok(FfmpegTools {
                        ffmpeg: ff,
                        ffprobe: fp,
                        version,
                        source: source.to_string(),
                    });
                }
            }
        }
        Err(StudioError::new(
            ErrorCode::ToolMissing,
            "FFmpeg was not found. Install FFmpeg or set its folder in Settings → Media Engine.",
        ))
    }

    pub fn from_dir(dir: &Path) -> StudioResult<FfmpegTools> {
        let ff = dir.join(exe("ffmpeg"));
        let fp = dir.join(exe("ffprobe"));
        if !ff.is_file() || !fp.is_file() {
            return Err(StudioError::new(
                ErrorCode::ToolMissing,
                "ffmpeg and ffprobe executables were not found in that folder.",
            ));
        }
        let version = Self::version_of(&ff)?;
        Ok(FfmpegTools {
            ffmpeg: ff,
            ffprobe: fp,
            version,
            source: "configured".into(),
        })
    }

    fn version_of(ff: &Path) -> StudioResult<String> {
        let out = command(ff)
            .args(["-hide_banner", "-version"])
            .output()
            .map_err(|e| {
                StudioError::new(ErrorCode::ToolMissing, "FFmpeg could not be started.")
                    .with_details(e.to_string())
            })?;
        if !out.status.success() {
            return Err(StudioError::new(
                ErrorCode::ToolMissing,
                "FFmpeg did not run correctly.",
            ));
        }
        let s = String::from_utf8_lossy(&out.stdout);
        Ok(s.lines().next().unwrap_or("ffmpeg").trim().to_string())
    }

    /// Whether this build is GPL-configured (affects redistribution, see DEPENDENCIES.md).
    pub fn build_configuration(&self) -> StudioResult<String> {
        let out = command(&self.ffmpeg)
            .args(["-hide_banner", "-buildconf"])
            .output()?;
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }
}
