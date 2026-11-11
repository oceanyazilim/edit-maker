# Roadmap

| Phase | Scope | State |
|---|---|---|
| 0 Foundation | Architecture, ADRs, dependency audit, timebase, timeline schema, render design | done |
| 1 Desktop MVP | Import, metadata, thumbnails, waveform, beats/kicks/drops, scene cuts, planner, timeline, preview, MP4 export, save/load | code complete. **Gate pending:** run the end-to-end test with FFmpeg, then manual QA in `tauri dev` |
| Update A | Multi-profile workspaces | backend done and tested; UI built, interactive QA pending |
| Update B | Clip-based workflow | discovery and reference role done. Next: semantic tags (local model), trend-reference structure analysis |
| 2 Intelligent editing | Manual beat correction UI, better structure labels, speed ramps, motion-based transitions, smart-crop tracking per frame, preview parity tests | next |
| 3 Advanced VFX | wgpu compositor (parity with the CPU reference), motion blur, glitch, chromatic aberration, directional blur, LUTs, effect pack manager (data-only) | — |
| 4 Trend Intelligence | Provider framework, YouTube Data API (user key), status for providers needing approval, profile-scoped research history (storage done) | — |
| 5 Creative AI | Local embeddings and semantic search, structured-action assistant | — |
| 6 Batch & campaigns | Batch queue with minimum-variation constraint (similarity metric done), campaigns | — |
| 7 Release | FFmpeg licensing decision, installer, clean-install tests, security audit | — |

## Next concrete tasks

1. Install FFmpeg, then run `cargo test -p studio-project --test e2e` and fix any issues.
2. Run `pnpm tauri dev` and test the main flow: create profile → project → import → generate → edit → export.
3. Lyrics tracks (text rendering in the compositor), speed ramps.
