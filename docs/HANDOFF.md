# Handoff: where development stands

Last updated 2026-11-11. Read this first when continuing on another machine.

## Goal (from the owner)

The owner supplies the music per project. Clips (licensed footage) are cut so that chosen moments land on given seconds and the edit follows the rhythm. Output: short-form vertical videos (TikTok / Reels / Shorts).

## State

- Everything in `IMPLEMENTATION_STATUS.md` marked ✅ is implemented and tested.
- Rust: 96 tests pass (`cargo test --workspace`); the end-to-end tests need FFmpeg and print SKIPPED without it.
- Frontend: `pnpm typecheck`, `pnpm test` (10 tests), `pnpm build` pass.
- The desktop UI is the five-region workstation (menu bar, tool rail, tool panel, preview, inspector, timeline) plus the export dialog. It was driven in the running app; see the "UI/UX redesign" section of `IMPLEMENTATION_STATUS.md`.

## Verify after cloning

```powershell
cargo test --workspace --no-fail-fast
cd apps/desktop
pnpm install --frozen-lockfile
pnpm typecheck; pnpm test; pnpm build
pnpm tauri dev
```

UI smoke without clicking by hand: start the app with
`$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9333"; pnpm tauri dev`
and drive it over the Chrome DevTools Protocol (`http://127.0.0.1:9333/json`). Use a throwaway project; synthetic mouse events can start real edits.

## Reference song analysis: "FUNK MISTERIO – NORMAL.wav"

Produced by `cargo run --release -p studio-cli -- analyze-audio <file>`:

- 115.31 BPM (confidence 0.98), bar = 2.081 s, 168 beats, 77 kicks, 309 snares
- Sections: 0–32.35 drop · 32.35–40.70 groove · 40.70–57.39 drop · 57.39–65.73 breakdown · 65.73–84.53 drop · 84.53–87.25 outro
- Main drop 67.83 s (score 0.73); secondary drop 42.78 s
- Suggested ~21 s segments (bar-aligned, drop at ≈4.2 s into the edit):
  - Main drop edit 63.65–84.53 s (26 kicks, 10 bars), best for anime edits
  - Second drop edit 38.61–59.50 s (25 kicks, 10 bars)
- Fitting presets: `anime-impact`, `brazilian-funk-velocity`

Planned next edit: anime clips (supplied by the owner, with rights to use them) → `anime-impact`, 9:16, start 63.65 s, ~21 s:

```powershell
cargo run --release -p studio-cli -- auto-edit --music "<song.wav>" --video-dir "<clips folder>" --preset anime-impact --duration 20.88 --start 63.65 --aspect 9:16 --out anime_edit.mp4
```

The app never downloads clips from TikTok / YouTube / Instagram. Trends are references only; source footage comes from the owner.

## Open work (priority order)

1. Committed UI test suite (Playwright/WebDriver for Tauri); today the UI checks are scripted by hand.
2. Drag and drop from the Media panel to the timeline (double-click / + insert at the playhead today).
3. Speed curves, reverse, freeze; keyframed effect parameters.
4. Text / captions / lyrics rendering (the Text, Captions and Overlays tools are disabled until then).
5. Real-time preview engine (wgpu); today playback is a low-resolution render, stills are frame-accurate.
6. Trend providers (official APIs only) and reference-video analysis.
7. Owner-defined niche presets (the owner will supply the creative rules).

## Conventions

- Integer timebase (ticks, 705 600 000 per second); seeded RNG; same inputs + settings + seed = identical edit.
- Business logic lives in Rust; the frontend is a thin typed IPC client (`src/services/api.ts`, TS types mirrored by hand in `src/types/studio.ts`).
- Never fake a feature: unimplemented controls are disabled with a tooltip.
- Reports to the owner are in Turkish; code, UI and docs in English.
