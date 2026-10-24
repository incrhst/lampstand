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
| `app` | Tauri 2 shell (`app/src-tauri`) and Svelte 5 web UI (`app/src`) |

How it fits together: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). Work to do: [docs/TICKETS.md](docs/TICKETS.md).

## Build

The app crate embeds the built web UI, so build that once before any
workspace-wide cargo command:

```
cd app && npm ci && npm run build && cd ..
cargo test --workspace
```

## Run the app

Needs Node 22 (tested with 22.22, npm 10.9) and Rust 1.95 or newer (tested
with 1.99). The Tauri CLI comes with the npm dependencies (2.12); a global
`cargo tauri` (tested with 2.9.4) works too. On Linux, install
`libwebkit2gtk-4.1-dev libsoup-3.0-dev libgtk-3-dev librsvg2-dev` first.
Linux installers are not built for now (the last release with one is
[v0.7.0](https://github.com/incrhst/lampstand/releases/tag/v0.7.0)), so on Linux you build from source.

```
cd app
npm ci
npm run tauri dev
```

Data lives in the OS app-data folder (`~/Library/Application Support/org.lampstand.app`
on macOS). To start from a fresh folder, set `LAMPSTAND_DATA_DIR`:

```
LAMPSTAND_DATA_DIR=/tmp/lampstand-fresh npm run tauri dev
```

First run: two public-domain hymns are already in the library. Under
**Plan**, create a service, add a song and click **Open for live**. Under
**Setup › Screens**, add a window and turn it on. Under **Present**,
press Space or Right for Next, Left for Previous and full stop for Black;
1 to 9 jump to a verse, C to the chorus, B to the bridge. To fill the
library, use **Get songs** (a free song database by language) or **Import
files…** / **Import folder…** for your own OpenSong library, OpenLyrics,
SongSelect, FreeShow or ProPresenter files.

## Help, problems and ideas

- **A question?** Ask it in
  **[Discussions](https://github.com/incrhst/lampstand/discussions)**: how to do
  something, setting up a projector or OBS, moving over from another app.
- **Something broken, or an idea?** Open an
  **[issue](https://github.com/incrhst/lampstand/issues/new/choose)**.

In the app, **Help › Help and feedback** has **Ask for help**, **Report a problem** and
**Suggest an idea** buttons that open the right page in your browser with your
version already filled in. Everything is public, so please do not paste
passwords or OBS Browser source addresses (they contain a key).

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE).

## Code signing policy

Free code signing for Windows provided by [SignPath.io](https://signpath.io),
certificate by [SignPath Foundation](https://signpath.org).

- Windows installers are built from this repository by the GitHub Actions
  workflow in `.github/workflows/release.yml`, then submitted for signing.
- Signing requests are approved by the project maintainer, David Bain
  ([@incrhst](https://github.com/incrhst)). Only builds of tagged releases
  from this repository are signed.
- Privacy: Lampstand collects no personal data. See the
  [privacy policy](https://lampstand.dbain.com/privacy.html).
