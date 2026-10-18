# Lampstand

A free, open-source church projection platform: songs, scripture, PowerPoint
and Google Slides, stage displays and OBS output, built to run a whole service
offline from one laptop. "Lampstand" is a working codename.

The product requirements are in [docs/PRD.md](docs/PRD.md).

## Layout

| Path | What it holds |
| --- | --- |
| `crates/lampstand-core` | Data model: songs, services, safe areas |
| `crates/lampstand-slides` | PPTX import and playback (PS-1 to PS-14) |
| `crates/lampstand-render` | wgpu renderer, one per output (LV, TS, OB) |
| `crates/lampstand-sync` | Local-first sync (CD-1 to CD-7) |
| `crates/lampstand-relay` | Self-hostable sync relay and browser planner |
| `app` | Tauri shell and web UI (not scaffolded yet) |

## Build

```
cargo test --workspace
```

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE).
