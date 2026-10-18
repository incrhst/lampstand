# Lampstand

A free, open-source church projection platform: songs, scripture, PowerPoint
and Google Slides, stage displays and OBS output, built to run a whole service
offline from one laptop. "Lampstand" is a working codename.

The product requirements are in [docs/PRD.md](docs/PRD.md).

## Layout

| Path | What it holds |
| --- | --- |
| `crates/lampstand-core` | Data model: songs, scripture, services, themes, outputs, decks, the scene format |
| `crates/lampstand-live` | Live engine: preview, live, navigation, overlays |
| `crates/lampstand-layout` | Fonts, splitting text into slides, composing each output's scene |
| `crates/lampstand-slides` | PPTX import: scenes, builds, transitions, fidelity report |
| `crates/lampstand-render` | wgpu renderer and output windows |
| `crates/lampstand-media` | Video and audio (GStreamer) |
| `crates/lampstand-sync` | Local-first sync, blobs, teams and roles |
| `crates/lampstand-library` | Library access, song search, service packages, readiness, CCLI |
| `crates/lampstand-import` | Song and Bible importers from other tools |
| `crates/lampstand-connectors` | Google Drive and Slides, share links |
| `crates/lampstand-server` | Local server: OBS browser source, peer sync |
| `crates/lampstand-api` | Messages between the web UI and the core |
| `crates/lampstand-relay` | Self-hostable sync relay |
| `app` | Tauri shell and web UI (not scaffolded yet) |

How it fits together: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). Work to do: [docs/TICKETS.md](docs/TICKETS.md).

## Build

```
cargo test --workspace
```

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE).
