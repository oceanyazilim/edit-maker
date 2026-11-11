# AI Auto Edit Studio

A standalone desktop app (Windows 10/11 x64) that edits music-synchronized short-form videos (TikTok, Reels, Shorts, YouTube) from **real footage**. It analyzes the music (tempo, beats, downbeats, kicks, snares, sections, drops), analyzes the scenes in your clips, plans an edit that follows the musical structure, and renders an MP4 you can then edit on a real timeline.

It never generates images or video. It only works with footage you import.

- Rust core (Cargo workspace), Tauri 2 shell, React 19 + TypeScript UI.
- Processing is local. No cloud service is needed.
- Multiple profiles (channel workspaces), each with its own projects, history, media references and defaults.

## Status

What works and what does not is listed in **[IMPLEMENTATION_STATUS.md](IMPLEMENTATION_STATUS.md)**. In short:

- Audio intelligence, the planner, the timeline model, the compositor, the job system and profiles are implemented and tested.
- The end-to-end render is verified: a real MP4 was produced and validated with ffprobe (FFmpeg 9.0.2 via winget).
- The desktop UI is a five-region workstation (menu bar, tool rail, tool panel, preview, inspector, timeline) with a two-column export dialog (video / audio only / still frame, range, resolution, fps, quality, extra aspect ratios). Unimplemented features are disabled, not simulated.

## Requirements

| Tool | Version used | Notes |
|---|---|---|
| Rust | 1.97 (MSVC toolchain) | VS 2022 Build Tools |
| Node.js / pnpm | 24 / 10 | frontend |
| WebView2 | preinstalled on Windows 11 | |
| FFmpeg + FFprobe | 6+ (8.x recommended) | **required** for video import, scene analysis, preview and export |

The app looks for FFmpeg in this order:
1. `STUDIO_FFMPEG_DIR`
2. the folder set in Settings
3. a copy bundled next to the executable
4. `third_party/ffmpeg/` in the repository
5. `PATH`
6. winget package folders (`%LOCALAPPDATA%\Microsoft\WinGet\Packages\*FFmpeg*\*\bin`, `...\WinGet\Links`), so the app works right after `winget install --id Gyan.FFmpeg` without restarting the shell.

On the development machine it was installed with `winget install --id Gyan.FFmpeg --exact` (9.0.2, GPL full build).

## Build & run

```powershell
# Rust tests (96 tests; the end-to-end tests need FFmpeg)
cargo test --workspace

# CLI
cargo run --release -p studio-cli -- doctor
cargo run --release -p studio-cli -- make-fixtures fixtures/generated
cargo run --release -p studio-cli -- analyze-audio fixtures/generated/music_128bpm.wav
cargo run --release -p studio-cli -- auto-edit --music song.mp3 --video-dir D:\clips\today --preset aggressive-phonk --duration 20 --cue "12.0@hook.mp4:3.4" --out edit.mp4
cargo run --release -p studio-cli -- verify-sync edit.mp4   # visual cuts vs. beats of the file's own audio

# Desktop app
cd apps/desktop
pnpm install
pnpm tauri dev       # development
pnpm tauri build     # NSIS installer
```

## Repository layout

```
crates/
  studio-core       timebase (flicks), errors, deterministic RNG, cancellation
  studio-audio      decoding (Symphonia), DSP, tempo/beats/downbeats, kicks/snares, structure, drops, synth fixtures
  studio-vision     scene analysis on decoded frames: cuts, fades, black, motion, camera pan, quality, focus
  studio-media      managed FFmpeg/FFprobe: probe, frame reader, thumbnails, proxies, encoder detection
  studio-timeline   canonical timeline, edit operations, undo/redo, validation, effect specification
  studio-planner    music-driven edit planner, presets, niche profiles, variants, similarity
  studio-render     effect evaluation, CPU compositor, FFmpeg encode pipeline, output verification
  studio-jobs       background job scheduler (limits, progress, cancellation, profile ownership)
  studio-project    project model and persistence, SQLite library, profiles, clip discovery, automation engine
  studio-cli        `aes` command line
apps/desktop        Tauri 2 + React UI
presets/            16 editing presets, 47 niche profiles (data-driven JSON)
docs/               ARCHITECTURE, PLANNER, ADRs
```
