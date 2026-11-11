# Post-MVP audit (V4 baseline)

Date of audit: 2026-11-11.

Method:
- ran the full test suite;
- launched the CLI and the Tauri app;
- rendered real MP4s;
- reviewed code per crate.

"Verified" means it was executed with real data in this audit, not only compiled.

## Architecture (unchanged, kept)

- Rust workspace: `studio-core`, `-audio`, `-vision`, `-media`, `-timeline`, `-planner`, `-render`, `-jobs`, `-project`, `-cli`.
- Tauri 2 shell (`apps/desktop/src-tauri`) and React 19 UI.
- FFmpeg 9.0.2 runs as managed child processes.
- SQLite library (schema v2) plus a JSON project file.

See ARCHITECTURE.md.

## Verified (executed)

| Area | Evidence |
|---|---|
| Build | `cargo build/test --workspace`: 85 tests pass. Frontend: `tsc`, `vite build`, 3 vitest tests |
| FFmpeg integration | auto-discovered (winget). Real test encodes: libx264, NVENC, QSV, MF OK; AMF unavailable |
| Real E2E | `cargo test -p studio-project --test e2e` runs for real: synthetic music + lavfi clips → analysis → plan → preview JPEG → MP4 → ffprobe → reopen |
| Real music | "FUNK MISTERIO", 5 versions: BPM consistent with the time ratios (5/5), kicks consistent across versions F 0.88–0.91, 77/77 kicks on the beat grid |
| Render quality | 1080×1920 H.264 High / AAC LC; 600/600 frames at exactly 1/30 s; A/V both start at 0 with equal duration; full decode without errors; bt709 tags |
| Sync | the song's 29/29 planned cuts within ±16 ms of detected beats/kicks |
| Profiles | DB isolation, shared assets, migration v1→v2, restart persistence (engine tests) |
| Tauri launch | app ran ~4.5 min without panic; real `library.db` migrated to v2 |

## Implemented but not verified end-to-end

- **Desktop UI flows.** No click-through testing has been done: profile switcher, project wizard, Auto Editor, timeline drag/trim, inspector, export page.
- **Preview playback.** It plays a low-resolution render of the same compositor; stills are frame-accurate. A real-time playback engine does not exist.
- **Clip Discovery UI and the cue panel UI.**

## Known defects / gaps (against V4)

| # | Gap | V4 section | Priority |
|---|---|---|---|
| 1 | No clip locks, no partial regeneration, no replace scene, no effect-only regeneration, no version history | §14 | high (this turn) |
| 2 | Audio window chosen automatically only; no segment suggestions, no drop placement control | §7 | high (this turn) |
| 3 | No persistent manual audio corrections (drops, grid shift); BPM override exists only in the analysis config | §6 | high (this turn) |
| 4 | Speed: constant per clip plus planner ramps (two clips); no curve editor, reverse, freeze | §15 | next |
| 5 | Effects: 11 types + 4 transitions on CPU (rayon). No keyframed effect parameters, no wgpu | §16–17 | next |
| 6 | Timeline: no slip/slide/roll, copy/paste, groups, multiple tracks in the UI, or text rendering | §18, §25 | next |
| 7 | Scene semantics: measured tags only (motion, pan, dark…); no local semantic model | §10 | later |
| 8 | Smart crop: one focus point per scene; no per-frame tracking or face detection | §20 | next |
| 9 | Trends: no provider connected (honest status page); reference-video analysis not implemented | §23–24 | later |
| 10 | Real-time preview engine, proxies not used by preview | §19 | next |
| 11 | Ana drop placement varies by up to 1 bar between time-scaled versions of the same song | §6 | open |
| 12 | Snare candidates over-detected on percussion-heavy real music (300+) | §6 | open |
| 13 | TS types hand-mirrored | — | debt |

## Niche presets (V4 §22)

The repository already ships 16 editing presets and 47 niche profiles. They are kept because they are a working feature and the planner tests depend on them. They are documented as **generic technical starting points**, not the user's niche-specific creative styles. The preset schema (`presets/editing/*.json`, `EditPreset`) is data-driven: it is versioned, validated, and has user presets in the data directory, so custom niche styles can be added without code changes.

## Technical debt

- `plan.rs` is large (~1300 lines). It should be split into window, cuts, scenes, effects and build modules once the lock feature is in.
- 14 minor clippy warnings (style only).
- The compositor is CPU-only. It is the parity reference for a future wgpu backend.

## Plan for this turn

1. Clip locks and regeneration modes: all, unlocked, selected range, replace scene, effects only. Timeline version history with undo.
2. Smart music segment selector and drop placement.
3. Persistent manual audio corrections.
4. UI wiring and a real-media acceptance test: lock the first 5 s, regenerate, check that the locked part is unchanged.
