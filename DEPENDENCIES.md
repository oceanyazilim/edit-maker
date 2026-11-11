# Dependencies and licensing

## Rust (crates.io)

| Crate | License | Use |
|---|---|---|
| symphonia 0.5 (`all`) | MPL-2.0 | Audio decoding (WAV, MP3, FLAC, AAC/M4A, OGG Vorbis…). File-level copyleft only; usable in proprietary apps when unmodified. |
| realfft 3 / rustfft | MIT/Apache-2.0 | STFT |
| rayon | MIT/Apache-2.0 | Parallel STFT and compositing |
| rusqlite 0.32 (`bundled`) | MIT; SQLite is public domain | Library database |
| serde / serde_json | MIT/Apache-2.0 | Models |
| thiserror, anyhow, tracing, tracing-subscriber | MIT/Apache-2.0 | Errors and logging |
| blake3 | CC0/Apache-2.0 | Fingerprints, cache keys |
| uuid | MIT/Apache-2.0 | IDs |
| image 0.25 (jpeg, png) | MIT/Apache-2.0 | Preview JPEG encoding |
| hound | Apache-2.0 | WAV writing (fixtures) |
| tokio, tokio-util | MIT | Declared in the workspace; not used yet (jobs run on OS threads) |
| tauri 2.12, tauri-plugin-dialog 2.8 | MIT/Apache-2.0 | Desktop shell |

Not adopted on purpose: `rand` (stream stability, see ADR-0007), FFmpeg native bindings (ADR-0002), ONNX Runtime (no validated model is selected yet).

## Frontend (npm, versions pinned)

react 19.1, react-dom, zustand 5, @tanstack/react-query 5, lucide-react, @tauri-apps/api 2.12, @tauri-apps/plugin-dialog 2.8, tailwindcss 4.3, vite 8, @vitejs/plugin-react 6, typescript 5.9 (7.0 exists, but 5.x is the mature line), vitest 5. All are MIT/ISC (lucide: ISC).

shadcn/ui is not installed through its CLI. The primitives in `components/ui.tsx` are owned source written in the same style.

## FFmpeg (redistribution decision pending)

- FFmpeg is called as an executable (ADR-0002).
- **GPL builds** include libx264 and libx265. Shipping them with a proprietary app requires GPL compliance for the bundle, or the user installs FFmpeg separately.
- **LGPL builds** have no x264. H.264 then depends on hardware encoders (NVENC, AMF, QSV, MediaFoundation `h264_mf`), which the app only lists after a real test encode.
- H.264 and HEVC patent licensing may apply to distribution in some regions.
- `aes doctor` shows whether the detected build is GPL.

**Recommendation:**
- Do not bundle FFmpeg by default. Let users point the app at their own install.
- Or bundle an LGPL build and rely on hardware or MediaFoundation encoders.

This needs a legal review before the first public release.

## Media and models

- No AI models are bundled. Any future model needs a license compatible with local commercial use.
- Built-in presets and niche profiles are original data in this repository.
- Test fixtures are generated synthetically (music by `studio-audio::synth`, video by FFmpeg `lavfi` sources). There is no third-party media.
