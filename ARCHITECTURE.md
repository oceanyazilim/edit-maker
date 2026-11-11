# Architecture

## Overview

```
React UI (apps/desktop/src) ── Tauri IPC (typed commands, `job` events) ── src-tauri (commands.rs)
                                                                               │
                                     studio-project::Engine  (automation: import → analyze → plan → preview → render)
          ┌───────────────┬───────────────┬────────────────┼───────────────┬────────────────┐
     studio-audio    studio-vision    studio-media     studio-planner   studio-render    studio-jobs
          │               │           (FFmpeg/FFprobe      │               │
          │               │            child processes)    │               │
          └──────── studio-timeline (canonical model) ─────┴───────────────┘
                              studio-core (time, errors, RNG, cancel)
```

There are no circular dependencies. The frontend never owns editing logic. Every mutation goes through a Rust command, and the Rust side validates it.

## Key decisions (ADRs)

| # | Decision | Rationale |
|---|---|---|
| 0001 | **Integer timebase**: 1 tick = 1/705 600 000 s (flicks) | Exactly divisible by every common frame rate (including NTSC x/1001) and every common sample rate. No float drift. Tests check 1 h of frames. |
| 0002 | **FFmpeg as managed executables**, not linked | Keeps the GPL/LGPL decision at the build level. Arguments are passed as vectors (no shell), paths are prefixed with `file:` (no protocol or option injection), there is no console window, and every process is cancellable. |
| 0003 | **Audio is decoded natively** (Symphonia, MPL-2.0) | Event positions are sample-accurate at the native sample rate, and music analysis works without FFmpeg. FFmpeg is the fallback for codecs Symphonia can't decode (Opus, AC-3). |
| 0004 | **One compositor for preview and export** | `studio-render::state::clip_state` (effect semantics) and `compositor` (pixels) are shared. Preview stills and exports therefore cannot diverge. The CPU/rayon implementation is the reference. A future wgpu backend must match it in parity tests. |
| 0005 | **Snapshot undo/redo** | Timelines are a few KB, so full snapshots are simpler and more robust than inverse operations. |
| 0006 | **Project = JSON file, library = SQLite** | Project files are portable and diff-able. They are saved atomically, with rotating backups and an autosave sibling. SQLite stores indexes: recent projects, render history, analysis cache keys, the shared media store and profile ownership. |
| 0007 | **In-tree deterministic RNG** (xoshiro256**) | The same inputs, settings and seed must give the same edit forever. An external crate could change its output stream between versions. |
| 0008 | **Jobs on OS threads** with per-kind limits | CPU-bound analysis and FFmpeg supervision never block the UI or the async runtime. Panics are caught and reported as failed jobs. |
| 0009 | **Profiles enforced in SQL** | `profile_id` foreign keys on projects, renders, asset references, favourites and research. `Engine::check_owner` refuses to open or save another profile's project. |

## Pipeline

1. **Import.** The path is canonicalized, the file is fingerprinted (BLAKE3 of size + first and last MiB), probed with ffprobe (or Symphonia for audio-only), and given a thumbnail. The library records it in the shared `media` store plus a `profile_assets` reference.
2. **Audio analysis.** Decode, then:
   - centered STFT (≈46 ms window / 11.6 ms hop, rayon);
   - log-mel spectral flux, then tempo by autocorrelation with a log-normal prior, then a dynamic-programming beat tracker;
   - kicks from two candidate sources (low-band level rise and low-band spectral flux), scored by multiplying evidence: 10–50 % rise time, attack, broadband transient, low-band share, prominence, loudness and beat-grid alignment; each kick position is refined to the sample;
   - downbeats from accents plus squared structural novelty;
   - bar-level sections, then drop candidates.
   Results are cached by fingerprint, algorithm version and config hash.
3. **Scene analysis.** FFmpeg decodes 160 px RGB at ≤15 fps; `SceneAnalyzer` computes:
   - hard cuts with an adaptive threshold and flash rejection;
   - gradual transitions and black segments;
   - global motion by block matching;
   - motion, impacts, sharpness, exposure, stability;
   - a motion- and detail-weighted focus point.
   Results are cached.
4. **Planning.** See [docs/PLANNER.md](docs/PLANNER.md).
5. **Render.** Each clip has an FFmpeg decoder (`fps=` conforms VFR sources; `1/speed` handles slow motion). Frames go through the Rust compositor (transform, effects, transitions), then into an FFmpeg encoder over stdin. Audio is mixed in a sample-accurate filtergraph. The output is written to `*.partial.mp4`, checked with ffprobe (duration, size, fps, audio) and blackdetect, then renamed atomically.

## Contracts

- Rust serde models are the source of truth. `apps/desktop/src/types/studio.ts` mirrors them by hand; generating them with `ts-rs` is planned.
- Errors cross IPC as `{ code, message, details? }`. `message` is safe to show to users. `details` only appears in the diagnostics panes.

## Security

- **Tauri capabilities:** `core:default`, events and the open/save dialogs. No shell, fs or HTTP plugins.
- **Asset protocol:** limited to the app cache, the projects directory and explicitly imported files.
- **Media input:** paths are canonicalized, must be files, and are passed to FFmpeg as `file:` URLs. Corrupt media gives structured errors (tests cover truncated and random-byte files).
- **Effect presets:** JSON data only. Nothing executable is ever loaded from a preset or pack.
