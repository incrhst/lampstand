# Lampstand architecture

How the pieces fit, and the decisions behind them. The requirements are in [PRD.md](PRD.md); the Phase 1 work is broken into tickets in [TICKETS.md](TICKETS.md).

The public APIs in `crates/*/src` are the contracts. Their doc comments are the spec. Bodies marked `todo!("Tnn")` belong to ticket Tnn. Tests marked `#[ignore = "Tnn"]` are that ticket's acceptance tests: remove the `ignore` and make them pass. If a ticket needs to change a contract, change it here and in the code in the same pull request, and say why.

## Crates

```
app          Tauri shell + Svelte UI, wires everything, speaks lampstand-api
───────────────────────────────────────────────────────────────────────────
services     api    server    library    connectors    relay (binary)
engines      live    layout    slides    render ← media    sync    import
model        core
```

Each layer uses only the layers below it.

| Crate | Owns | Depends on |
| --- | --- | --- |
| `core` | Data model and pure logic: songs, scripture, services, themes, outputs, decks, safe area, the scene format, `Program` | nothing of ours |
| `live` | The live engine: preview, live, navigation, overlays | core |
| `layout` | Fonts, splitting text into slides, fitting text, composing each output's scene | core |
| `slides` | PPTX → scenes, builds, transitions, notes, fidelity report | core |
| `render` | wgpu: output windows, offscreen rendering, transitions, animations, calibration pattern | core, media |
| `media` | Video and audio decoding (GStreamer behind a feature) | — |
| `sync` | Automerge documents, blob store, sync protocol, LAN discovery, teams and roles | core |
| `library` | Typed access to documents, song search, service packages, readiness check, CCLI usage | core, sync |
| `import` | Song and Bible importers, finding other apps' data | core |
| `connectors` | Google Drive and Slides, share links | — |
| `server` | Local HTTP and WebSocket server: OBS browser source, peer sync, picker page | core, render, sync |
| `api` | Messages between the UI and the core | core, live, sync |
| `relay` | Self-hostable sync relay binary; guest upload links | sync |
| `app` | Tauri shell, web UI, wiring everything together | all |

`core`, `live`, `layout` (apart from loading system fonts), `slides`, `import` and `library::search` are pure: no network, no async, no GPU. Most of the behaviour lives there, so it is all unit-tested.

Third-party versions are pinned once, in `[workspace.dependencies]`. A crate adds a dependency there first, then uses `foo.workspace = true`.

## One Sunday-morning click, end to end

1. The operator presses Space. The UI sends `Request::Live(Command::Next)` over Tauri IPC.
2. `LiveEngine::apply` moves the cursor and updates the `Program`: background, content, overlay, next slide.
3. For each enabled output, `layout::compose(program, output, size, theme, fonts)` builds that output's `Scene` in its own pixels, using its own safe area, theme and role (audience, stage or stream).
4. The scene goes to the render thread as `OutputCommand::Show { scene, transition }`, or to the local server for browser-source outputs. On a deck build step, the step's animations go as `OutputCommand::Animate` instead.
5. The render thread draws it at the next vsync.

Budget: 50 ms from click to frame (NFR). Steps 1 to 4 run in-process with no disk or network access. Composition is cached per (content, output), so a click that only toggles an overlay costs one cheap compose per output.

## Decisions

### Scenes are the only thing renderers see

Decks, lyrics, scripture, countdowns and the stage display all become a `core::scene::Scene`: a tree of paths, text blocks, images and videos with transforms. Renderers do no layout. They shape the text runs they are given into the box they are given, at the size they are given. This means:

- The wgpu renderer and the OBS browser source (DOM) draw the same thing.
- Golden-image tests compare scenes, not app states.
- PPTX knowledge stays in `slides`. Preset shapes become paths there, inheritance is resolved there, and autofit is applied there.

Node IDs below 2^24 belong to content (deck shapes, so build animations can target them). The compositor numbers its own nodes from 2^24 up.

### One slide list per item, fitted per output

Songs are split by the theme's line count. Scripture is split by what fits the primary audience output. Every other output shows the same `SlideText` and shrinks it to fit (TS-7). Navigation is therefore global: "slide 3" means the same thing on the projector, the stage display and the stream. Text that would go below the theme's minimum size on any output is flagged by the readiness check before the service, not discovered live.

### Sizes are relative to a 1080-pixel-tall output

Theme sizes are pixels on a 1080-line screen and scale with output height, so one theme looks the same on 720p, 4K, 4:3 and ultra-wide outputs (TS-9). Decks keep their own aspect ratio and fit inside the safe area (TS-4).

### Outputs render natively on one render thread

Each physical output is a borderless fullscreen Tauri window with no webview and a wgpu surface (Tauri 2 exposes raw window handles). One render thread owns the device and every surface. It paces itself with FIFO present and never waits on the UI thread. The cursor is hidden over output windows, and they are created only once the first frame is ready, so a desktop or a white flash never shows (NFR Reliability).

On laptops with two GPUs, the integrated one is preferred: it has lower power draw and avoids switching stalls. If no adapter works, a software adapter is used so the app always starts.

### Crash recovery: a journal now, a separate process later if needed

Phase 1 runs in one process. The live cursor and overlay state are written to a small journal file on every change. On start, if the journal says a service was live less than 10 minutes ago, the app reopens it with the same slide on the same outputs. The target is under 5 seconds (NFR). Panics on the render thread are caught and the thread is restarted with the last scene.

`OutputCommand` is serialisable on purpose. If field reports show whole-app crashes, outputs can move to a child process without changing any caller.

### Text: cosmic-text everywhere

`layout` measures with cosmic-text, and `render` draws with glyphon on the same `FontSystem` and the same font files. A line that fits in layout therefore fits on screen. The bundled fonts are:

- Noto Sans and Noto Serif
- Metric-compatible stand-ins for the usual PowerPoint fonts: Carlito (Calibri), Caladea (Cambria), Arimo (Arial), Tinos (Times New Roman) and Cousine (Courier New)

A missing font in a deck uses its stand-in where one exists, and the fidelity report says so (PS-4).

### PPTX: evaluate DeckCraft, plan to own the parser

A short spike (T28) checks DeckCraft's licence, reader quality and fit with our scene format. Unless it is clearly usable, `slides` owns a parser built on `zip` and `quick-xml`. Either way, the parser's output is our `ParsedDeck`, so the choice is internal to `slides`.

Google Slides arrive as their PPTX export and take the same path. A 200-deck fidelity corpus with PowerPoint-exported reference images is measured with SSIM in CI (T32). Real church decks cannot go in a public repo, so the corpus lives in private storage and the CI job reads it with a secret. The public repo carries synthetic decks.

### Sync: one Automerge document per thing, blobs beside them

- **Documents.** Every song, service, theme, deck, translation's metadata and device usage log is its own Automerge document. One more team document holds members, roles, invites and an index of every document. The document ID is the entity's UUID. Core types map to documents with autosurgeon, so a planner's edit becomes only the operations for the fields that changed.
- **Ordering.** Service items are a map keyed by item ID, each with a fractional `Position`, not a list. A CRDT list has no "move", and concurrent moves would duplicate items.
- **Blobs.** Deck files, images, video and Bible text are content-addressed files (BLAKE3). Peers fetch them by hash on demand. They are never inside documents, which keeps documents small and sync fast.
- **Wire protocol.** One protocol for every link: a WebSocket carrying Automerge sync messages and blob requests. LAN peers advertise `_lampstand._tcp` over mDNS and connect to each other's local server (CD-2). The relay speaks the same protocol (CD-3). Shared-folder sync (CD-7, Phase 2) writes the same saved chunks to a folder.
- **No samod.** The repo wraps `automerge` directly: samod cannot create a document under a chosen ID and has no hook to drop changes by author role ([decision 0002](decisions/0002-sync-repo.md)). Document IDs are UUIDs and messages are plain Automerge sync messages, so a relay can still bridge to automerge-repo for the Phase 2 browser planner.
- **Storage.** `docs/<uuid>/` holds a `kind` file and the document's chunks: one incremental chunk per change, compacted into a snapshot after 32. `blobs/<hash>` holds blobs, written to a temp file, hashed while streaming, then renamed.
- **Team index.** The team document's `index` map (ID → kind) lists every document; its `deleted` map holds tombstones (`Repo::is_deleted` tells a deleted document from one that never existed). Its first change is made by a fixed actor at time zero, so every device's copy shares it and the maps merge. Until T18 the team document's ID is the nil UUID.
- **Actors.** A change's actor ID is the device's 32-byte public key plus 8 random bytes per `Repo::open`, so peers can find the author's role (`actor_device`) and a device that lost its store never reuses a sequence number.
- **Sync API.** `Repo::generate_sync_message` and `receive_sync_message` keep one Automerge sync state per (peer device, document) and carry the document's kind, so the receiver can create an unseen document and check permissions. T17 puts them on the WebSocket; tests pass them between in-memory repos.

### What syncs and what stays on the device

| Synced | Device only |
| --- | --- |
| Songs, services, themes, decks (metadata and current file), translations, team, usage logs | Output configuration, display mapping, safe areas (they belong to this machine's screens), window layout, Google tokens (OS keychain), the device key, caches (parsed decks, thumbnails, search index) |

### Teams and roles: signed membership, honest-member threat model

A team is rooted in its founder's device key. Membership entries in the team document are signed by an admin whose own entry chains back to the founder. Any peer, including the relay, can therefore compute every device's role from the document alone.

Peers refuse connections from devices without a role, and drop changes to document kinds the author's role may not write (`team::Role::can_write`). Invites are single-use tokens with an expiry; the team document stores only each token's hash. They are shared as a `lampstand://join` link or a QR code (CD-6).

What this does and does not protect against:

- It keeps outsiders out and stops members' mistakes.
- It does not stop a member who runs a modified client, because Automerge changes are not signed one by one. The remedy is removing that member's device.
- A self-hosted relay can read the team's documents. It needs to, to check roles and to serve the Phase 2 browser planner. End-to-end encryption is a Phase 2 question tied to the public relay (CD-4).

### UI: one web app, one message set

The operator and planning screens are a Svelte 5 + TypeScript app (Vite), in `app/`. Svelte was chosen for its small bundles and plain syntax, which suits a volunteer open-source team.

The UI speaks only `lampstand-api` messages. On the desktop they go through a single Tauri command and event stream; in Phase 2 the browser planner sends the same messages over a WebSocket. TypeScript types are generated from the Rust types with ts-rs and never written by hand.

### Media: GStreamer, behind a feature

Video backgrounds, media items and video embedded in decks decode through GStreamer (`playbin3` into an `appsink` of RGBA frames, uploaded to wgpu textures). The same pipeline gives NDI, Spout and Syphon in Phase 2.

GStreamer is behind a `gstreamer` feature so contributors and CI can build without it. It is also the main threat to the "tens of MB" install size: a trimmed Windows and macOS runtime is roughly 60 to 100 MB. T27 measures it; the fallback is platform decoders (Media Foundation, AVFoundation) behind the same `MediaBackend` trait.

### Local server

One axum server on port 7340 serves the OBS browser source, peer sync and the Google picker page. Browser-source URLs carry a per-output random key. The browser source draws scenes in the DOM and gets deck slides as frames rasterised by our renderer. Guest upload links (CD-9) live on the relay, because guests are not on the church network; without a relay, guests paste a share link instead.

### Clickers

Presenter clickers send Page Up, Page Down, B and full stop. While a service is live and clicker mode is on, those keys are registered as global shortcuts, so they work whichever window has focus (PS-9). Arrow keys are taken only while a Lampstand window has focus, so other apps keep them.

### Updates

The Tauri updater installs only when `LiveEngine::is_idle()` is true and no service starts within the next hour (CD-11).

## Testing

| Layer | How |
| --- | --- |
| Pure crates | Unit tests next to the code; acceptance tests in the contracts |
| Importers | Fixture files per format in `crates/lampstand-import/tests/fixtures/` |
| Layout | Snapshot tests of composed scenes as JSON (insta) |
| Render | Golden PNGs from `render_offscreen` on a software adapter (lavapipe or WARP in CI), compared with a per-pixel tolerance |
| Slides | Synthetic decks in the repo; the 200-deck SSIM corpus in a private CI job |
| Sync | Two and three in-process repos over in-memory transports, with random edit and partition schedules |
| App | Playwright against the web UI with a mock API; a manual Sunday-service checklist per release |

CI runs on Windows, macOS and Linux: `cargo fmt --check`, `cargo clippy -D warnings` and `cargo test --workspace` (T58).

## Rust version

`rust-version` tracks stable minus about six months; it is 1.95 now. Bump it in its own pull request.
