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

## Try the output window

Shows a song full screen on a second display if there is one, otherwise on
the main display:

```
cargo run -p lampstand-render --bin lampstand-output -- --windowed --safe 0.05,0.05,0.2,0.05
```

Pass a song as JSON to show it instead of the demo
(see `crates/lampstand-render/examples/amazing-grace.json`). Drop
`--windowed` for full screen.

| Key | Action |
| --- | --- |
| Right, Down, Space, Page Down | Next slide |
| Left, Up, Page Up | Previous slide |
| B or . | Black screen (press again to return) |
| C or W | Clear text, keep the background |
| S | Show the safe area |
| Ctrl+Q / Cmd+Q | Quit (Esc also quits in a window) |

Set `LAMPSTAND_TIMING=1` to log the time from key press to frame.

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE).
