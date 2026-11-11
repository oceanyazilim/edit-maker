# Test plan

Run everything with:

```
cargo test --workspace --no-fail-fast
cd apps/desktop
pnpm test
pnpm build
```

| Suite | Location | What it verifies |
|---|---|---|
| Timebase | studio-core `time.rs` | Exact frame and sample conversions; no drift over 1 h; serde form |
| Audio evaluation | studio-audio `tests/evaluation.rs` | Beat F-measure (±70 ms) ≥ 0.9, downbeat F ≥ 0.9, kick F ≥ 0.9 (±30 ms) and MAE < 10 ms, snare F ≥ 0.8, drops within ±1 beat, section labels. Regressions: sustained/wobble bass false kicks, distorted 170 BPM kicks (with and without sidechain), syncopated trap, 22.05/48 kHz, BPM override, silence, cancellation |
| Decoding | studio-audio `tests/decoding.rs` | WAV round-trip, random-byte file, truncated WAV, missing file |
| Scenes | studio-vision `tests/scenes.rs` | Hard cut, flash rejection, black segment, dissolve, pan direction, focus point, static scene |
| Media | studio-media | ffprobe JSON parsing: rotation, VFR, HDR, cover art, images, truncated files; fingerprint |
| Timeline | studio-timeline | Insert/overlap, split continuity and frame snapping, trims against source bounds, ripple, track moves, locks, undo/redo, snapping, serde, keyframes, validation (missing media, overruns, handles) |
| Planner | studio-planner `tests/planning.rs` | Determinism, duration, gapless tiling, cut on drop, drop position, ≥ 90 % event-aligned cuts, min shot length, kick-effect cooldown, preset/control effects, variant similarity, repetition limits |
| Render | studio-render | Effect state (punch/flash decay, shake determinism, fades), compositor geometry, audio graph, encoder arguments |
| Jobs | studio-jobs | Results/progress, cancellation, panic isolation, concurrency limit, owner retention |
| Project / library | studio-project | Save/load, backups, autosave recovery, migrations, damaged files; profile isolation, shared assets, profile lifecycle, v1→v2 DB migration; clip discovery |
| Profiles (engine) | studio-project `tests/profiles.rs` | Defaults → project, overrides don't write back, cross-profile open/save refused, shared music, active profile restored after restart, legacy project files |
| **End-to-end** | studio-project `tests/e2e.rs` | Synthetic music plus 3 lavfi videos → analysis, cut detection, plan, preview JPEG, **real MP4 render**, ffprobe validation, reopen, cache reuse; render cancellation leaves no partial file. **Requires FFmpeg; prints SKIPPED otherwise.** |
| Locks / regeneration | studio-planner `tests/planning.rs`, studio-project `tests/e2e.rs::lock_regenerate_versions_and_audio_edits` | Locked clips identical after regeneration, range regeneration, replace scene, effects-only, unique ids, versions, persistent audio corrections, real render |
| Segment selector | studio-planner `segments.rs` | Suggestions only from detected structure, bar/phrase-aligned starts, no drop suggestions without a drop |
| Frontend | apps/desktop `src/lib/time.test.ts`, `src/lib/export.test.ts` | Tick and frame math, timecode; aspect-aware export resolutions, even dimensions, Windows file-name rules, extensions, path joining |
| UI smoke (manual, scripted) | WebView2 remote debugging (`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333`) + a CDP script | Workstation renders a real project; Inspector tabs; menus; "Analyze music"; export dialog → MP4 ×2 (9:16 + reframed 1:1), PNG, WAV, verified with ffprobe |

**Not automated yet:**
- UI interaction tests as a committed suite (Playwright/WebDriver for Tauri); the UI smoke above is run by hand
- Preview/export pixel-parity comparisons
- Real-music audio benchmark
