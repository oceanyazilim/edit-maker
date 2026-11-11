# Implementation status

Legend:
- ✅ implemented and covered by tests that were executed
- 🟡 implemented but not yet verified end-to-end
- ⏳ not implemented

## Phase 1 – working MVP

| Area | Status | Evidence / notes |
|---|---|---|
| Timebase, IDs, errors, RNG, cancellation | ✅ | studio-core: 8 tests |
| Audio decoding (WAV/MP3/FLAC/AAC/OGG via Symphonia) | ✅ WAV round-trip, corrupt/truncated/missing files | Other formats rely on Symphonia and are not tested with real files yet |
| Tempo, beats, downbeats | ✅ | Synthetic evaluation: beat F = 0.998 (±70 ms), downbeat F = 1.0, BPM error < 0.1 |
| Kick detection | ✅ | F = 1.0 for EDM and trap; 0.97 for distorted hardtekk (with and without sidechain). Sustained or wobble bass gives ≤ 2 false kicks. Mean timing error 5.1 ms. |
| Snare candidates | ✅ | F = 0.90 on synthetic EDM |
| Sections and drop candidates | ✅ | Drops found within 10 ms of truth in tests |
| Scene analysis (cuts, flash rejection, gradual, black, pan, motion, focus) | ✅ synthetic frames | Real video only through FFmpeg (🟡) |
| FFmpeg integration (probe, frames, thumbnails, proxy, encoder detection) | ✅ | FFmpeg 9.0.2 (winget, Gyan full build). Auto-discovered, including winget Packages/Links when PATH is stale. Real test encodes: libx264, h264_nvenc, h264_qsv, h264_mf, libx265, hevc_nvenc, hevc_qsv work; AMF unavailable (no AMD GPU). |
| Planner, 16 presets, 47 niche profiles, 6 variants | ✅ | 6 tests: determinism, 100 % event-aligned cuts, cut at drop, preset/control effects, variant similarity < 0.85 |
| Timeline ops (split/trim/move/ripple/locks/snapping), undo/redo, validation | ✅ | 14 tests |
| Compositor and effect semantics | ✅ | Unit tests (identity mapping, cover crop bounds, flash/black, punch decay, deterministic shake) |
| Export pipeline, verification, atomic finalize | ✅ | `tests/e2e.rs` runs for real (2/2). `out/beat_sync_demo.mp4`: H.264 High, yuv420p, 1080×1920, 30/1, 600/600 frames each 1/30 s, AAC-LC 48 kHz stereo, both streams start 0 / duration 20.000 s, full decode without errors, bt709 tags. All 17 planned cuts are ≤ 36 ms (mean 18.7 ms) from beats/kicks detected in the rendered file's own audio. Release render speed ≈ 22–27 fps at 1080×1920 (CPU compositor). |
| Project persistence (atomic save, backups, autosave recovery, migration, relink) | ✅ | 4 tests |
| Job scheduler (limits, cancel, panic isolation, owner) | ✅ | 4 tests |
| Desktop UI | 🟡 | `tsc`, `vite build` and 3 vitest tests pass. `pnpm tauri dev` launched: the app ran ~4.5 min without panics until it was stopped by a timeout. The backend migrated the real `library.db` to v2. **No click-through UI test has been done.** |

**Phase 1 acceptance gate** ("a real beat-synchronized MP4 is produced"): **met with synthetic fixtures** (synthetic 128 BPM music + FFmpeg lavfi videos). Not yet verified with commercially produced music.

## Cues and folder import (user workflow: own music per project, the day's clips, pinned seconds)

| Item | Status |
|---|---|
| Cues: pin "clip X at source second S" to output second T; optionally snapped to the nearest beat/kick/downbeat within ½ beat; reported as `ResolvedCue` | ✅ planner test. Real render: cue requested at 5.1 s was placed at 5.167 s (on a beat), clip_b at 9.0 s, confirmed by extracted frames. An unsnapped cue at 13.37 s landed at 13.367 s (frame grid). |
| Out-of-range cues and source moments too close to a scene boundary produce warnings | ✅ |
| Cue panel in the Auto Editor (persisted in project settings) | 🟡 built, not clicked through |
| Folder import (`--video-dir`, Tauri `import_folder`, defaults to the profile's media folder; duplicates skipped by fingerprint) | ✅ real CLI run |
| Automatic downloading of trending platform videos | ⛔ not implemented by design (platform terms / copyright). The user supplies the day's clips. |

## Update: profiles (Stage A)

| Item | Status |
|---|---|
| SQLite v2 migration (profiles, app_state, profile_id on projects/renders, profile_assets, favourites, trend_research), run in a transaction | ✅ the migration test turns a v1 DB with data into the default profile |
| Profile CRUD, duplicate, archive/restore, delete (archive first; files never deleted), search, export config | ✅ library tests |
| Backend-enforced isolation (queries scoped by profile_id; `check_owner` blocks open/save across profiles) | ✅ engine test |
| Profile defaults → new project settings; project overrides never write back | ✅ |
| Shared asset store with per-profile references and sharing; removing a reference keeps media used elsewhere | ✅ |
| Jobs keep their owner profile; switching does not cancel jobs | ✅ (owner test); the UI shows "(other profile)" |
| Active profile restored after restart | ✅ (simulated restart test) |
| Pre-profile project files load into the default profile | ✅ |
| Top-right profile switcher, profile dialog, profile management page | 🟡 typechecked and built, not exercised interactively |

## Update: clip-based editing (Stage B)

| Item | Status |
|---|---|
| Clip-only workflow (no generative visuals anywhere) | ✅ by design: no generation code exists |
| `reference` asset role (trend/style references are never placed on the timeline) | ✅ (the planner uses only `footage`) |
| Clip Discovery over the indexed local library (text/tags/motion/quality/duration, own + shared media, no rescans) | ✅ unit test; 🟡 UI |
| Character/subject or semantic search | ⏳ needs a validated local model. The UI says so explicitly. |
| Trend Intelligence providers (YouTube Data API, TikTok, Meta) | ⏳ no provider connected; no data is shown or invented |
| Speed ramps, motion blur, glitch, chromatic aberration, directional blur | ⏳ current effects: zoom punch, shake, flash, RGB split, color, vignette, fades, 4 transitions, constant-speed slow motion |
| Lyrics videos (word timing, animated typography) | ⏳ text rendering is not implemented |
| Profile-specific trend recommendations | ⏳ only the storage exists (`trend_research` per profile) |

## Known limitations

- Audio evaluation uses synthetic music with exact annotations. A real-music annotated benchmark has not been run.
- Downbeats assume 4/4.
- Preview playback plays a low-resolution render made with the export compositor. Stills are frame-accurate. A real-time playback engine and wgpu are not implemented yet.
- HDR sources are not tone-mapped (a warning is shown).
- Footage audio is not mixed. Only the audio-track clips (the music) are.
- Manual beat-marker editing in the Audio UI is not implemented.
- TS types are hand-mirrored (ts-rs is planned).

## Real-music validation (first commercially produced track)

Track: "FUNK MISTERIO" (Brazilian funk / phonk), 5 versions (normal, sped up, slowed, superslowed, ultraslowed). No hand annotations exist. Accuracy is checked by **cross-version consistency**: the versions are time-scaled copies, so correctly detected events must scale with the duration ratio.

| Check | Before fixes | After fixes |
|---|---|---|
| BPM vs expected from time ratio | 4/5 correct; superslowed at 2× | 5/5 within 0.5 BPM (115.3 / 123.7 / 97.5 / 88.4 / 79.8) |
| Beat phase | **grid locked onto the off-beat** (95 % of strong kicks at phase 0.4–0.6) | corrected: 77/77 kicks on the grid |
| Kicks found (normal version) | 2 | 77 (73–87 across versions) |
| Kick agreement across versions (F, ±50 ms scaled) | n/a | 0.88–0.91 |
| Beat agreement across versions (F, ±70 ms scaled) | 0.65–1.00 | 0.88–1.00 |
| Main drop agreement | 3/4 within 0.6 s | 2/4 exact, sped up −0.5 s (1 beat), slowed −2.5 s (1 bar) |

Fixes made because of this track:
- half-beat phase correction (from kick pulse regularity and phase concentration);
- double-time correction from the kick pulse;
- kick evidence rebalanced for sub-heavy 808-style kicks;
- on-beat "rhythmic rescue" with attack/rise guards.

All synthetic regression tests still pass (wobble/sustained bass false kicks = 0).

Edit test with this song (synthetic clips, 20 s, brazilian-funk-velocity):
- 29/29 cuts within ±16 ms of the song's detected beats/kicks.
- An independent re-analysis of the rendered 20 s file finds 21/29 within one frame, worst 105 ms.
- Pacing: 1–2-beat shots with ≤ 4 equal lengths in a row (was 20 s of constant 1-beat cuts before the phrasing fix).

Still not validated: real footage (anime, movie, gameplay clips) and whether the result looks good to a human viewer.

## Effects added (all in the shared compositor, so preview equals export)

- **New effects:** glitch (band displacement + channel split), chromatic aberration, directional motion blur, zoom blur. All have unit tests.
- **Velocity ramps:**
  - into the drop: fast → slow motion, with the speed change on a beat and source time continuous;
  - occasional bursts inside drop sections.
  - Covered by a planner test and a real render: 2.58× → 0.40×, with the change one beat before the drop.
- **Kick effect pool:** zoom / shake / glitch / motion blur / chroma, weighted and deterministic, never the same effect three times in a row.
- **Phrased pacing:** seeded shot-length patterns per section, longer holds on 8-bar phrase starts, and a monotony guard that only picks other on-grid lengths.
- **Persistence fix:** serde_json `float_roundtrip`, so computed speeds survive save/load bit-exactly. The e2e reopen test caught this.

## V4 turn 1: locks, regeneration, segment selection, audio corrections

| Feature | Status | Evidence |
|---|---|---|
| Clip lock (`Clip.locked`): kept by regeneration; move/trim/split/delete/ripple refused; parameters still editable | ✅ | timeline test `clip_locks_block_timing_edits_but_not_parameters` |
| Regenerate unlocked (keep locked clips) | ✅ | planner test plus **real-media e2e** `lock_regenerate_versions_and_audio_edits`: locked clips identical field by field, the rest changed |
| Regenerate selected range | ✅ | planner test plus e2e: clips outside 6–10 s unchanged; temporary locks are not left behind |
| Replace scene | ✅ | planner test: same slot, different scene |
| Reapply effects (keep sources and timing) | ✅ | planner test |
| Version history (saved before every generate/regenerate, max 20, restore) | ✅ | e2e restore check; Editor "History" tab |
| Unique clip/effect ids after partial regeneration | ✅ | found by a test (id collision) and fixed |
| Smart Music Segment Selector (main drop, second drop, build-up→drop, high energy, intro); phrase/bar-aligned starts; phrase-fit alternative length | ✅ | unit tests; real song: Main Drop Edit 63.65–84.53 s (fit 20.88 s); rendered drop landed at 4.17 s vs predicted 4.18 s |
| Drop placement (preset / early / after intro / middle / late) | ✅ | planner input `drop_position` |
| Persistent manual audio corrections (BPM override, grid offset, add/remove drops) | ✅ | e2e: correction survives save/reopen and drives the plan |
| UI: segment panel, lock toggle (L / inspector), Shift+drag range, regenerate toolbar, History tab, corrections panel | 🟡 | typechecked and built; the app launches; **not click-tested** |

Still open from V4: speed-curve editor / reverse / freeze, keyframed effect parameters, wgpu, real-time preview engine, per-frame smart crop, multiple video tracks in the UI, text/lyrics, trend providers, reference-video analysis, batch UI. See docs/POST_MVP_AUDIT.md.

## UI/UX redesign: workstation layout

The editor is a five-region workstation. Every control calls the real Rust backend; unimplemented features are disabled with a tooltip saying so.

| Area | Status | Evidence |
|---|---|---|
| Top menu bar (File / Edit / Clip / Sequence / View / Help), project name, save status, jobs, Export, settings, profile avatar | ✅ | Clicked in the running Tauri app (WebView2 + CDP). Cut/Copy/Paste and Speed Curve are disabled ("not implemented") |
| Tool rail (Media, Auto Edit, Transitions, Effects, Audio, Library, Presets) | ✅ | Text, Overlays and Captions are disabled: there is no text rendering and no overlay opacity/blend |
| Resizable, persisted panels (tool panel, inspector, timeline); View menu toggles and reset | ✅ | `useResizable` stores sizes in localStorage; the layout and the active tool are persisted too |
| Auto Edit panel: real music facts (BPM + confidence, main drop, used range), whole-song waveform (sections, bars, drops, used range, playhead), click to seek, Alt+click to set the start bar; style grid from the preset engine (Built-in / My Styles, favorites); setup, segments, cues, controls, versions; generate / regenerate unlocked / effects / variation | ✅ | Real song project: 115.3 BPM, drop 67.83 s, range 63.6–84.5 s shown; "Analyze music" run from the panel (128.0 BPM on the 128 BPM fixture) |
| Preview: aspect/resolution, preview scale, viewer zoom (Fit/50/100 %), safe areas, transport, volume, playback speed, fullscreen | ✅ | Volume/speed apply to the preview render's `<video>` |
| Inspector tabs: Video (framing, transition, effects), Speed (constant speed with source-bound check), Animate (transform keyframes + presets), Adjust (clip-wide color / vignette) | ✅ | Speed: the backend now rejects speeds that run past the source media ("At 16.00x this clip would need 16.12s of source…"), verified through IPC. Keyframes use the existing renderer support (`transform_at`). Speed curves and enhancement are shown as not implemented |
| Timeline: undo/redo, select/razor, snap, split, delete, marker (M; Alt+click removes), add video/audio track, regenerate tools, zoom slider, fit; beat-grid row (sections, bars, beats, kicks, drops); track headers; clip thumbnails; music waveform | ✅ | Backend ops `add_marker`, `remove_marker`, `add_track`, `insert_clip` |
| Export dialog: Video / Audio Only / Still Frame; Entire / Selected range; 720p–4K / Custom (aspect-aware); 24–60 fps incl. 59.94; Standard / High / Custom quality; Windows file-name validation; folder picker; overwrite check; hardware Auto/GPU/CPU (GPU disabled without a working encoder); extra aspect ratios as reframed exports; progress, cancel, Open Folder / Open File | ✅ | Driven in the running app: 9:16 720×1280 + 1:1 720×720 MP4, PNG still and WAV export; ffprobe confirmed h264/aac, 30 fps, 15.000 s, correct sizes. "AI enhancement", "smooth motion" and "detail recovery" are disabled (no real implementation) |
| Dashboard / browser pages restyled with the same tokens and grouped navigation | ✅ | |

**Bug found by driving the UI:** the timeline drag effect added a new `mouseup` listener on every render without removing the old ones, so one drag could commit several "Move clip" edits. Fixed (`Timeline.tsx`: the cleanup now removes both listeners).

**Not yet:** automated UI tests (the checks above were scripted through the WebView2 DevTools protocol, not a committed test suite); drag-and-drop from the media panel to the timeline (double-click / + inserts at the playhead instead); clip thumbnails do not appear for media whose thumbnail cache lies outside the app's asset scope (seen with a project created by the test suite in a temp directory); the Inter/Geist fonts are used only if installed (no font is bundled; Segoe UI Variable is the fallback).

