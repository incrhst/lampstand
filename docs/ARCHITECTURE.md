# Lampstand architecture

How the pieces fit, and the decisions behind them. The requirements are in [PRD.md](PRD.md); the Phase 1 work is broken into tickets in [TICKETS.md](TICKETS.md).

The public APIs in `crates/*/src` are the contracts. Their doc comments are the spec. Bodies marked `todo!("Tnn")` belong to ticket Tnn. Tests marked `#[ignore = "Tnn"]` are that ticket's acceptance tests: remove the `ignore` and make them pass. If a ticket needs to change a contract, change it here and in the code in the same pull request, and say why.

## Crates

```
app          Tauri shell + Svelte UI, wires everything, speaks lampstand-api
───────────────────────────────────────────────────────────────────────────
services     api    server    library    connectors    relay (binary)
engines      live    slides    layout ← render ← media    sync    import
model        core
```

Each layer uses only the layers below it, except the arrows within a layer: `render` uses `layout`'s font set, so both shape text from the same fonts, and `media`'s decoders.

| Crate | Owns | Depends on |
| --- | --- | --- |
| `core` | Data model and pure logic: songs, scripture, services, themes, outputs, decks, safe area, the scene format, `Program` | nothing of ours |
| `live` | The live engine: preview, live, navigation, overlays | core |
| `layout` | Fonts, splitting text into slides, fitting text, composing each output's scene | core |
| `slides` | PPTX → scenes, builds, transitions, notes, fidelity report | core |
| `render` | wgpu: output windows, offscreen rendering, transitions, animations, calibration pattern | core, layout (fonts only), media |
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
5. The render thread wakes, draws the new scene into a texture, composites it onto each output's surface and presents (FIFO). It reaches the screen at the next vsync.

Budget: 50 ms from click to frame (NFR). Steps 1 to 4 run in-process with no disk or network access. Composition is cached per (content, output), so a click that only toggles an overlay costs one cheap compose per output. Step 5, measured from `send` to `present` on three windows on an M1 (`crates/lampstand-render/tests/windows.rs`), takes a p95 of 10 to 16 ms, so steps 5 and 6 together come to under 33 ms including the refresh wait.

## Decisions

### Scenes are the only thing renderers see

Decks, lyrics, scripture, countdowns and the stage display all become a `core::scene::Scene`: a tree of paths, text blocks, images and videos with transforms. Renderers do no layout. They shape the text runs they are given into the box they are given, at the size they are given. This means:

- The wgpu renderer and the OBS browser source (DOM) draw the same thing.
- Golden-image tests compare scenes, not app states.
- PPTX knowledge stays in `slides`. Preset shapes become paths there, inheritance is resolved there, and autofit is applied there.

Both renderers read a scene the same way. The wgpu renderer's choices (T24), which the browser source follows:

- Colours are straight-alpha sRGB, and blending and gradient interpolation happen on the sRGB values (premultiplied), as in CSS. No linear-light blending.
- A node's opacity applies to the node as a whole: a faded group or text block fades as one image, without darker overlaps.
- Gradient and dash coordinates, stroke widths and shadow offsets and blurs are in the node's local units, so they scale and rotate with it. A shadow's `blur` is a CSS blur radius (twice the Gaussian's standard deviation). Strokes are centred, with miter joins and butt caps; fills are non-zero.
- Text: line height is `size × line_spacing`; `space_before`, `space_after` and `indent` are in scene units; a paragraph wraps (by word, then by glyph) at the block's width minus its indent; a bullet hangs in the indent, at the block's left edge, on the first line's baseline, in the first run's style. Text that overflows its rect is drawn, not clipped. An outline is drawn under the fill and shows `width` outside the glyph edge (CSS `paint-order: stroke` with a stroke of twice the width).
- Images: `crop` fractions outside 0 to 1 leave that part of the rect empty.

Node IDs below 2^24 belong to content (deck shapes, so build animations can target them). The compositor numbers its own nodes from 2^24 up.

### One slide list per item, fitted per output

Songs are split by the theme's line count. Scripture is split by what fits the primary audience output. Every other output shows the same `SlideText` and shrinks it to fit (TS-7). Navigation is therefore global: "slide 3" means the same thing on the projector, the stage display and the stream. Text that would go below the theme's minimum size on any output is flagged by the readiness check before the service, not discovered live.

### Sizes are relative to a 1080-pixel-tall output

Theme sizes are pixels on a 1080-line screen and scale with output height, so one theme looks the same on 720p, 4K, 4:3 and ultra-wide outputs (TS-9). Decks keep their own aspect ratio and fit inside the safe area (TS-4).

### Outputs render natively on one render thread

Each physical output is a borderless fullscreen Tauri window with no webview and a wgpu surface. One render thread owns the device and every surface. It paces itself with FIFO present and never waits on the UI thread. The cursor is hidden over output windows, and they are created only once the first frame is ready, so a desktop or a white flash never shows (NFR Reliability).

How T25 built it (details at the top of `lampstand-render/src/lib.rs`):

- `Outputs::spawn` takes over a `Renderer` and starts the thread. `Outputs` implements `OutputSink`, and commands travel over an unbounded channel. Events come back through a callback that must not block: `FirstFrame`, `Presented` (frame timestamps), `SurfaceLost` and `Restarted`. Thumbnails use a second `Renderer`, so they never contend with the frame loop.
- The loop draws only outputs that changed or are animating, and blocks on the channel otherwise. Each scene is drawn once into a texture. A frame is then a few textured quads, laid out by pure, unit-tested timeline functions. Frame latency is 1.
- `Outputs::attach` takes any `wgpu::SurfaceTarget<'static>`, such as a winit or Tauri window. The surface is created on the caller's thread and presented from the render thread.
- Windows are created hidden, with a black background, and shown on `FirstFrame`. macOS will not present to a hidden window, so there the window appears black and its first frame follows within one refresh. On macOS, fullscreen means simple fullscreen, so there is no new Space and no animation.
- Displays are matched by name, then size, then nearest position (`output::assign_displays`). A display that matches on neither name nor size is never used, so a missing projector's output waits hidden instead of covering the operator's screen.
- A surface failure affects only its own output. The output keeps its scene, so re-attaching a window shows the live slide at once.
- `core::scene::Direction` always names a side: the side a push or cover enters from, where a wipe starts, where a fly-in comes from. `Effect::Transparency` is PowerPoint's transparency value (0.5 means half see-through).

How the app hosts them (T46, `app/src-tauri/src/shell.rs`):

- **Webview-less Tauri windows.** Output windows are `tauri::window::Window`s built with `tauri::window::WindowBuilder`, which needs Tauri 2's `unstable` feature (Tauri 2.12). The alternative, creating our own winit windows and handing their raw handles to T25, is not possible: Tauri's event loop (tao) is the only one a process may run on macOS, and winit cannot share it. A webview window with a surface on top would load a webview per projector for nothing. A `tauri::Window` implements raw-window-handle 0.6, so it goes straight into `Outputs::attach`. If `unstable` changes under us, the fallback is the same window type built through `WebviewWindowBuilder` with an empty page, at the cost of a webview per output.
- **Threads.** The main thread runs Tauri's event loop and creates, places and destroys output windows (macOS wants surfaces created there); it never locks the app state. `api` requests run on Tauri's blocking pool and lock `App`. An app thread (`lampstand-app`) handles window resizes and closes, render-thread events, library change notifications, the 100 ms live tick and a once-a-second display poll (Tauri has no hot-plug event). Imports and song-database downloads run without the `App` lock, on the shared `Library`, so a minutes-long import never blocks live control.
- **Sizes.** Each output is composed at its window's real inner size in physical pixels, reported on attach and on every resize, so a Retina rehearsal window gets a 2× scene. A window is reopened only when the placement it was asked for changes (a different display, a new configured size), never because its real size differs.
- **Rehearsal windows** (`OutputTarget::Window`) are normal decorated windows, opened in the bottom-right corner of the operator's screen. macOS never presents to a window that is completely covered (wgpu reports it occluded and T25 retries), so a rehearsal window hidden behind the operator window looks frozen until uncovered. Projectors and stage displays are not affected. Closing a rehearsal window disables its output.
- **Display outputs** are borderless, always on top and kept out of the taskbar, placed with physical coordinates on the display `assign_displays` picks, and made simple-fullscreen with the cursor hidden on their first frame.

On laptops with two GPUs, the integrated one is preferred: it has lower power draw and avoids switching stalls. If no adapter works, a software adapter is used so the app always starts.

### Crash recovery: a journal now, a separate process later if needed

Phase 1 runs in one process. The live cursor and overlay state are written to a small journal file on every change. On start, if the journal says a service was live less than 10 minutes ago, the app reopens it with the same slide on the same outputs. The target is under 5 seconds (NFR). Panics on the render thread are caught and the thread is restarted with the last scene. Each output's frame is caught on its own, so a scene that panics resets only its output. Its transition and build steps jump to their end, and its last scene is drawn again. If the same output panics twice within 10 seconds, it shows black instead of crashing in a loop. wgpu's default error handler, which panics, is replaced with one that logs.

`OutputCommand` is serialisable on purpose. If field reports show whole-app crashes, outputs can move to a child process without changing any caller.

### Text: cosmic-text everywhere

`layout` measures with cosmic-text, and `render` draws with glyphon on the same `FontSystem` and the same font files. A line that fits in layout therefore fits on screen. The bundled fonts are:

- Noto Sans and Noto Serif
- Metric-compatible stand-ins for the usual PowerPoint fonts: Carlito (Calibri), Caladea (Cambria), Arimo (Arial), Tinos (Times New Roman) and Cousine (Courier New)

A missing font in a deck uses its stand-in where one exists, and the fidelity report says so (PS-4).

### PPTX: depend on DeckCraft's reader, own the mapping

Both evaluations of DeckCraft, the T28 spike ([0001](decisions/0001-pptx-reader.md)) and the DeckCraft evaluation in PR #2, recommend the same thing: depend on DeckCraft's `deckcraft-pptx`, `deckcraft-model` and `deckcraft-anim` crates, pinned to a commit, rather than write our own reader. Its licence (MIT OR Apache-2.0) is compatible with GPL-3.0, and its reader handled the test decks. `slides` converts DeckCraft's `Presentation` into our `ParsedDeck`, so nothing from DeckCraft appears in `slides`' API. We don't use DeckCraft's layout, text shaping or renderer; ours are T20, T22 and T24. If upstream is slow to take a fix, we fork with `[patch]`.

DeckCraft needs Rust 1.95, which sets the workspace's minimum (`rust-version = "1.95"`).

If T29 finds the pinned reader loses fidelity on real decks, the fallback is our own parser on `zip` and `quick-xml`. The `ParsedDeck` boundary keeps that switch inside `slides`.

Google Slides arrive as their PPTX export and take the same path. A 200-deck fidelity corpus with PowerPoint-exported reference images is measured with SSIM in CI (T32). Real church decks cannot go in a public repo, so the corpus lives in private storage and the CI job reads it with a secret. The public repo carries synthetic decks.

### Sync: one Automerge document per thing, blobs beside them

- **Documents.** Every song, service, theme, deck, translation's metadata and device usage log is its own Automerge document. One more team document holds members, roles, invites and an index of every document. The document ID is the entity's UUID. Core types map to documents with autosurgeon, so a planner's edit becomes only the operations for the fields that changed.
- **Ordering.** Service items are a map keyed by item ID, each with a fractional `Position`, not a list. A CRDT list has no "move", and concurrent moves would duplicate items.
- **Blobs.** Deck files, images, video and Bible text are content-addressed files (BLAKE3). Peers fetch them by hash on demand. They are never inside documents, which keeps documents small and sync fast.
- **Wire protocol.** One protocol for every link: a WebSocket carrying Automerge sync messages and blob requests. LAN peers advertise `_lampstand._tcp` over mDNS and connect to each other's local server (CD-2). The relay speaks the same protocol (CD-3). Shared-folder sync (CD-7, Phase 2) writes the same saved chunks to a folder.
- **No samod.** The repo wraps `automerge` directly: samod cannot create a document under a chosen ID and has no hook to drop changes by author role ([decision 0002](decisions/0002-sync-repo.md)). Document IDs are UUIDs and messages are plain Automerge sync messages, so a relay can still bridge to automerge-repo for the Phase 2 browser planner.
- **Storage.** `docs/<uuid>/` holds a `kind` file and the document's chunks: one incremental chunk per change, compacted into a snapshot after 32. `blobs/<hash>` holds blobs, written to a temp file, hashed while streaming, then renamed. Each `put`, and each batch of received sync messages (`Repo::receive_sync_messages`), is made durable as a unit: on Apple platforms every file gets a plain `fsync` and the batch one `F_FULLFSYNC` (Rust's `sync_all` is a full drive-cache flush there, about 6 ms each), which took a `put` from about 40 ms to about 10 ms.
- **Team index.** The team document's `index` map (ID → kind) lists every document; its `deleted` map holds tombstones (`Repo::is_deleted` tells a deleted document from one that never existed). Its first change is made by a fixed actor at time zero, so every device's copy shares it and the maps merge. Until T18 the team document's ID is the nil UUID.
- **Actors.** A change's actor ID is the device's 32-byte public key plus 8 random bytes per `Repo::open`, so peers can find the author's role (`actor_device`) and a device that lost its store never reuses a sequence number.
- **Sync API.** `Repo::generate_sync_message` and `receive_sync_message` keep one Automerge sync state per (peer device, document) and carry the document's kind, so the receiver can create an unseen document and check permissions. T17 puts them on the WebSocket; tests pass them between in-memory repos.
- **Network (T17, `sync::net`, behind the `net` feature).** A `SyncNode` per device. Frames are postcard-encoded, one per binary WebSocket message: `Hello`, `Proof`, `Refused`, `Sync { kind, id, data }`, `BlobWant`, `BlobChunk` (256 KiB) and `BlobMissing`. Either device may dial; the server side is `WS /sync` on the local server. If two devices dial each other at once, both keep the connection dialled by the smaller device key.
  - *Handshake.* Each side sends a hello (device key, name, a fresh nonce), then signs a hash of both hellos with its device key, so neither can claim a key it does not hold and a proof cannot be replayed. A device that is not paired is refused before any document moves.
  - *Pairing, until T18.* The presenting machine shows a six-digit code for five minutes; the service device types it. The code is never sent: both run SPAKE2 with it inside the handshake and confirm the derived key over the same transcript, so a listener learns nothing and an impostor gets one guess per attempt (five wrong guesses void the code). Each side then adds the other to `paired.json`, which is device-only. T18 replaces the list with team membership and adds role checks at the `// T18:` hook.
  - *Push, then sweep.* On connect, each side offers every document. After that every local change is offered to every connected peer at once (with the team document, whose index a new document changes), so a fix on a service device reaches the presenting machine in tens of milliseconds. Sync messages that arrive together are applied as one batch. A 500-song first sync takes about 0.7 s (debug build, one machine).
  - *Reconnect.* Each paired device has a dialler that reconnects with backoff (0.5 s doubling to 10 s, reset after a connection that lasted) to its mDNS address or the address it was last reached at, and wakes when mDNS sees it again. On disconnect the peer's sync state is dropped (`Repo::forget_peer`), so the next connection resumes from a fresh sync handshake and catches up.
  - *Discovery.* mDNS `_lampstand._tcp` with TXT `id` (device key), `name`, `pairing` and `v`. Church networks often block multicast between Wi-Fi and wired, so every screen that needs a device also takes an address (`host` or `host:port`).
  - *Blobs* are fetched on request (`SyncNode::fetch_blob`) from whichever connected device has them, and verified on arrival (`BlobStore::put_verified`).
  - *Measured* (`crates/lampstand-server/tests/sync.rs`, two processes on an M1, debug build): found by mDNS in under 1 s, converged 40 ms after pairing, edit to change on the other process median 19 ms, a restarted peer caught up in under 1 s. `app/src-tauri/tests/sync.rs` times edit to `Event::Loaded` on the presenting app at about 50 ms. Target: under 1 s on a LAN.

### What syncs and what stays on the device

| Synced | Device only |
| --- | --- |
| Songs, services, themes, decks (metadata and current file), translations, team, usage logs | Output configuration, display mapping, safe areas (they belong to this machine's screens), window layout, Google tokens (OS keychain), the device key, the paired-devices list (until T18), caches (parsed decks, thumbnails, search index) |

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

How the app is laid out (T46):

- `app/src-tauri/src/handler.rs` holds `App`, the app state (library, live engine, fonts, per-output composition cache, device-local output configuration) and every request handler. It has no Tauri in it: the render thread is behind `OutputSink` and the windows behind a `WindowHost` trait, so `app/src-tauri/tests/handler.rs` drives the whole click path with a recording sink and fake windows.
- `app/src/lib/api.ts` is the only way to the core: `request(req, "Services")` returns the payload of the expected response type and throws `ApiError` on `Response::Error`; `send(req)` for `Ok`; `onEvent(handler)` for the `api-event` stream. `app/src/lib/state.svelte.ts` keeps the loaded service, cursors and overlay current from events.
- Screens live in `app/src/routes/<name>/`, one folder per ticket (T47 `operate`, T48 `library`, T49 `plan`, T50 `outputs`...). Shared components and design tokens are in `app/src/lib/kit/`.
- `ServiceRequest::Open` answers `Response::Loaded`: every item with its slides, as the live engine holds them, so the operator view never paginates. `Event::Loaded` brings the same after a reload, and `Event::Live` carries the overlay so Black shows its state.
- `Request::Team` is answered by `app/src-tauri/src/sync.rs` on the async runtime, without the `App` lock: `Status`, `StartPairing`/`StopPairing`, `Join { invite, address }` (until T18 the invite is the pairing code), `Connect { address }` and `Unpair`, each answering `Response::Team(TeamStatus)`. `Event::SyncStatus` fires when connections or nearby devices change. The devices screen is `app/src/routes/devices/`. A library change wakes the app thread at once, so a synced edit reloads the open service without waiting for the 100 ms tick.
- Operator keys (in Lampstand's windows): Space, Right or Page Down for Next; Left or Page Up for Previous; full stop for Black; 1 to 9 jump to that verse of the live song; C, B, P, T, I and E jump to the next chorus, bridge, pre-chorus, tag, intro or ending, wrapping round. B means Bridge here, not Black: clickers send B for black, so T56's clicker mode maps the clicker's B to Black while it holds the global shortcuts.

### Getting songs in

`LibraryRequest::ImportFiles` takes files and folders and works out the format by extension, then by content: OpenLyrics, OpenSong (a song file, a `Songs` folder, OpenSong's data folder, or an OpenSongApp `.osb` backup), SongSelect, FreeShow, ProPresenter, EasyWorship, and the Bible formats. Imported songs get an ID derived from their title and lyrics (UUIDv5), so importing the same library twice skips the songs already there and leaves edited copies alone.

A first library can come from Worship Leader App's free song database (`lampstand-connectors::song_databases`): about 50 languages, published daily as `.osb` backups. The operator picks a language and the app downloads and imports it. Nothing is bundled: many of those songs are under copyright, and the church's own licence covers projecting them. The database carries no copyright or CCLI numbers, so CCLI reports for those songs need the numbers added. A fresh install also imports two public-domain hymns (`app/src-tauri/seed/`), so there is always something to show.

Importing the 3,100-song English database took about 110 s on an M1 (release build), almost all of it in `Repo::put`'s drive-cache flushes. T17 made each `put` one flush instead of six (see "Sync", Storage), about four times faster; a `put` of many documents with one flush would close the rest of the gap (follow-up).

### New service from a list (proposed)

A pasted list of songs and readings ("Come Thou Fount, 10000 reasons, Ps 23, sermon John 10:1-18") becomes a draft service that the person checks and creates ([decision 0003](decisions/0003-service-from-list.md), not built yet). A small Gemma model, run by a local runtime where the machine has one, only splits the text into song, scripture and heading entries; without one, a line parser does. Library search and `parse_reference` do the matching, so the model never picks a song, never touches the live engine, and nothing leaves the machine.

### Media: GStreamer, behind a feature

Video backgrounds, media items and video embedded in decks decode through GStreamer (`playbin3` into an `appsink` of RGBA frames, uploaded to wgpu textures). The same pipeline gives NDI, Spout and Syphon in Phase 2.

GStreamer is behind a `gstreamer` feature so contributors and CI can build without it. It is also the main threat to the "tens of MB" install size: a trimmed Windows and macOS runtime is roughly 60 to 100 MB. T27 measures it; the fallback is platform decoders (Media Foundation, AVFoundation) behind the same `MediaBackend` trait.

### Local server

One axum server on port 7340 serves the OBS browser source, peer sync and the Google picker page. T42's skeleton has `Server::start` and `WS /sync`, which hands each socket to the app's `SyncNode` (`Backend::sync`); T43 added the OBS routes (below); the picker page answers 404 until T40. The app listens on every interface; if 7340 is taken (a second copy on one machine) it takes any free port and advertises that one. `LAMPSTAND_PORT`, `LAMPSTAND_DEVICE_NAME` and `LAMPSTAND_NO_MDNS` override the defaults. Browser-source URLs carry a per-output random key. The browser source draws scenes in the DOM and gets deck slides as frames rasterised by our renderer.

How T43 built the browser source (`crates/lampstand-server/src/obs/`, page in `web/obs/`):

- `GET /obs/<output>?k=<key>` is a small vanilla TypeScript page with a transparent body. It opens `WS /obs/<output>/ws?k=<key>`, which sends `{"seq", "scene"}` as JSON (the scene as serde writes it) for the current scene and then every change. The page scales the scene to whatever size OBS gives it. The compiled script (`web/obs/dist`, checked in so cargo needs no Node; CI checks it is current) and the bundled fonts as `@font-face` (`/obs/assets/`, served from the same bytes layout and render shape with, via `lampstand_layout::bundled_font_files`) are public. Scenes, frames and blobs need the output's key; a wrong key or output is a 404. `/blobs/<hash>` takes the output as `?o=<output>`, because the key belongs to an output.
- A deck slide (a stream scene that is one compositor group around content nodes) goes out as `{"seq", "width", "height", "frame"}` instead, and the page shows `GET /frames/<output>/<n>.webp?k=<key>`: the scene through `render_offscreen`, as lossless WebP so transparency survives, rendered once per scene however many pages ask. This needs `Backend::renderer`; without it the deck is drawn in the DOM like any other scene.
- The app feeds it through `SceneHub`: `add_output` gives an output its URL key, `publish(output, scene)` shows a scene, and the app's `Backend` delegates `output_key_valid` and `watch_scene` to it. `Backend::watch_scene` gives the current scene first, then each change.
- The page follows the rules above (the DOM equivalents are in the header of `web/obs/src/render.ts`). Differences that remain: scene changes are cuts, not transitions; videos are plain muted `<video>` elements; Chromium puts each line's baseline on a whole pixel and anti-aliases glyph edges its own way, so text agrees with the GPU renderer to within a pixel, not bit for bit. Guest upload links (CD-9) live on the relay, because guests are not on the church network; without a relay, guests paste a share link instead.

### Clickers

Presenter clickers send Page Up, Page Down, B and full stop (in the operator view, B is Bridge; see "UI"). While a service is live and clicker mode is on, those keys are registered as global shortcuts, so they work whichever window has focus (PS-9). Arrow keys are taken only while a Lampstand window has focus, so other apps keep them.

### Updates

The Tauri updater installs only when `LiveEngine::is_idle()` is true and no service starts within the next hour (CD-11).

## Testing

| Layer | How |
| --- | --- |
| Pure crates | Unit tests next to the code; acceptance tests in the contracts |
| Importers | Fixture files per format in `crates/lampstand-import/tests/fixtures/` |
| Layout | Snapshot tests of composed scenes as JSON (insta) |
| Render | Golden PNGs from `render_offscreen`, compared with a per-pixel tolerance; `LAMPSTAND_BLESS=1` rewrites them. CI renders on lavapipe (Linux), WARP or the runner's adapter (Windows) and Metal (macOS); with no adapter at all the tests skip with a message |
| Slides | Synthetic decks in the repo; the 200-deck SSIM corpus in a private CI job |
| Sync | Two and three in-process repos over in-memory transports, with random edit and partition schedules |
| OBS browser source | Playwright (`crates/lampstand-server/web`) loads the page from a real server and compares screenshots with `render_offscreen` for a lyrics lower third, a gradient plate, a deck frame and an empty scene; skipped where there is no GPU or software adapter |
| App | Playwright against the web UI with a mock API; a manual Sunday-service checklist per release |

CI runs on Windows, macOS and Linux: `cargo fmt --check`, `cargo clippy -D warnings` and `cargo test --workspace` (T58).

## Rust version

`rust-version` tracks stable minus about six months; it is 1.95 now. Bump it in its own pull request.
