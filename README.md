# Lampstand

A free, open-source church projection platform: songs, scripture, PowerPoint
and Google Slides, stage displays and OBS output, built to run a whole service
offline from one laptop. "Lampstand" is a working codename.

The product requirements are in [docs/PRD.md](docs/PRD.md). Version 0.1, which
runs a service on a Mac, is described in [docs/v0.1.md](docs/v0.1.md).

## Layout

| Path | What it holds |
| --- | --- |
| `crates/lampstand-core` | Data model: songs, safe areas |
| `crates/lampstand-songs` | Song import (OpenLyrics, OpenSong, ChordPro) and search |
| `crates/lampstand-scripture` | References, passage splitting, Bible import |
| `crates/lampstand-slides` | PPTX import via DeckCraft (PS-1 to PS-14) |
| `crates/lampstand-show` | Show engine: library, service order, live state; protocol types |
| `crates/lampstand-server` | Local HTTP and WebSocket server; operator, stage and OBS pages in `web/` |
| `crates/lampstand-render` | wgpu renderer, safe areas and calibration (LV, TS, OB) |
| `crates/lampstand-output` | Projector process: one fullscreen window per output |
| `crates/lampstand-sync` | Local-first sync (CD-1 to CD-7; not wired into v0.1) |
| `crates/lampstand-relay` | Self-hostable sync relay |
| `app/src-tauri` | Desktop app: runs the server, opens the operator window, supervises outputs |

## Build and run

Needs Rust 1.95 or later.

```bash
cargo test --workspace
```

Build the projector process first: the app looks for it next to its own executable.

```bash
cargo build -p lampstand-output -p lampstand-app
```

```bash
./target/debug/lampstand
```

The operator window opens on `http://127.0.0.1:7420/operator`. Open **Library** to
import songs, a Bible translation your church is licensed to use, decks and a logo.
With a projector or TV connected, the output goes fullscreen on it (on every display
but the main one); with one display it opens in a window. Press **C** on an output to
calibrate its safe area.

On the same network:

- Stage display: `http://<this Mac>:7420/stage` in any browser.
- OBS: add a Browser source with `http://<this Mac>:7420/stream`, 1920×1080.

To run just the server, for a browser-only setup or development:

```bash
cargo run -p lampstand-server -- --data /tmp/lampstand-test
```

Data lives in `~/Library/Application Support/Lampstand` (`LAMPSTAND_DATA` overrides it).

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE).
