# Phase 1 tickets

Phase 1 is every P0 requirement in [PRD.md](PRD.md). Each ticket is sized for one working session and one pull request, and touches only the files it names, so tickets in the same wave can run side by side in separate worktrees. Read [ARCHITECTURE.md](ARCHITECTURE.md) first.

**Model.** **S** tickets are well specified and suit Sonnet. **O** tickets set architecture that other tickets build on, carry the main project risks, or need judgement the spec can't give; give them to Opus. Every pull request gets an Opus review before merge.

**Done means:**

- The ticket's acceptance tests pass, with their `#[ignore]` removed.
- New behaviour has tests.
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean.
- No `todo!("Tnn")` for this ticket remains.
- Any contract change is reflected in ARCHITECTURE.md in the same pull request.
- The ticket's **Status** below says Done, in the same pull request.

**Status** is one of: **Not started**; **Partial** (some of it is on `main`, and the cell says what); **In review** (with a link to the open pull request); **Done** (merged to `main` and meeting the list above); **Skipped** (with the reason). A pull request that starts or finishes a ticket updates its status.

**Brief for a session.** The ticket text plus: "Read docs/ARCHITECTURE.md and the doc comments of the crate you're working in. Touch only the files this ticket names, unless the contract has to change, and then say why."

## Wave 0: foundations (no dependencies; all parallel)

Where tickets in one crate each add a module, each also adds one `mod` line to that crate's `lib.rs`. Those one-line conflicts are expected; resolve them by keeping both lines.

| ID | Title | Model | PRD | Files | Status |
| --- | --- | --- | --- | --- | --- |
| T01 | Scripture references, display and verse lookup | S | BI-1 | `core/src/scripture.rs` | Done |
| T02 | Fractional positions and service ordering | S | PL-1 | `core/src/service.rs` | Done |
| T03 | Section kinds and ChordPro helpers | S | SL-1 | `core/src/song.rs` | Done |
| T15 | Song search index with typo tolerance | S | SL-3 | `library/src/search.rs` | Done |
| T20 | Font set: bundled fonts, substitutions, `FontLookup` | S | PS-4, LV-4 | `layout/src/fonts.rs`, `assets/fonts/` | Done |
| T28 | DeckCraft evaluation spike | S | PS-1 | `docs/decisions/0001-pptx-reader.md` | Done |
| T33 | Live engine state machine | S | LV-1, LV-2, PS-8 | `live/src/lib.rs` | Done |
| T39 | Share-link classification and download | S | PS-1, CD-9 | `connectors/src/links.rs` | Done |
| T58 | CI on Windows, macOS and Linux | S | NFR Platforms | `.github/workflows/` | Done |
| T16 | Sync repo: documents, storage, blob store | **O** | CD-1 | `sync/src/lib.rs`, `sync/src/repo/`, `core` (feature only) | Done |
| T45 | API types to TypeScript | S | — | `api/`, `app/src/lib/api/` | Done |
| T04–T13 | Importers (see below) | S | SL-4, BI-2 | one file each in `import/src/` | Done (T06 skipped) |

**T01. Scripture references, display and verse lookup.** Implement `parse_reference`, `Display for Passage` and `BibleText::verses`. The book table should hold full names, SBL abbreviations, common short forms ("Jn", "Ps", "Phil", "Phm", "Song of Songs") and roman or ordinal prefixes ("I", "II", "First"). Done when the four `T01` tests pass. Also add round-trip tests: parse, display, parse again gives the same passage.

**T02. Fractional positions and service ordering.** Implement `Position::between`, `Service::insert` and `Service::move_item` over the base-62 alphabet `0-9A-Za-z`. A small crate such as `fractional_index` is fine only if it guarantees that alphabet; otherwise write it yourself (about 60 lines). Done when the `T02` tests pass, plus a property test: random inserts and moves always keep `ordered()` consistent with the intended order.

**T03. Section kinds and ChordPro helpers.** Implement `SectionKind::from_label`, `lyrics` and `chords`. Done when the `T03` tests pass.

**T15. Song search.** Implement `SongIndex` as specified in `search.rs`: normalise text (lowercase, NFKD with accents stripped, punctuation removed), keep a word → song postings map, and match with prefixes and bounded edit distance. Don't add tantivy. Done when the `T15` tests pass and a benchmark with 5,000 generated songs returns under 10 ms per query in release mode.

**T20. Font set.** Add `cosmic-text` and implement `Fonts::load` and `FontLookup`. Commit the bundled OFL and Apache-licensed fonts under `assets/fonts/`, with their licence files. The substitution table maps Calibri → Carlito, Cambria → Caladea, Arial → Arimo, Times New Roman → Tinos, Courier New → Cousine and Helvetica → Arimo; anything else falls back to Noto Sans. Done when tests cover exact match, substitute and missing, and loading takes under 300 ms in a release build.

**T28. DeckCraft spike (half a session).** Answer four questions in a decision record:

1. What is its licence, file by file?
2. How well does its reader handle five synthetic decks with masters, placeholders, groups, tables, images and animations?
3. Could its model map onto `ParsedDeck`?
4. How active is it upstream?

End with a recommendation: adopt the reader, borrow parts, or write our own. T29 waits on this.

**T33. Live engine.** Implement `LiveEngine::load` and `apply` to the rules in the `live` module docs. Done when the eight `T33` tests pass. Add tests for `Preview` and `GoLive` on headers, an empty service, and `Show` on a missing item. Each must be a no-op that returns no events.

**T39. Share links.** Implement `classify`, and `fetch` with `reqwest` (rustls). Done when the `T39` test passes and `fetch` has tests against a local mock HTTP server, covering a 304 or unchanged version and a 403. One `#[ignore]` live-network test uses a public sample deck.

**T58. CI.** Set up GitHub Actions on Linux, macOS and Windows: fmt, clippy with `-D warnings`, tests and `cargo deny` for licences (GPL-compatible only). Cache with `Swatinem/rust-cache`. On Linux, install Mesa lavapipe so later render tests can find a software adapter. Done when the workflow is green on this repo.

**T16. Sync repo (Opus).** Add `automerge`, `autosurgeon` and possibly `samod`. First evaluate samod (wire compatibility with automerge-repo, storage layout, maturity) and record the choice in `docs/decisions/0002-sync-repo.md`. Then:

- Implement `Repo` and `BlobStore` as specified.
- Add the `automerge` feature to `core`, with `Hydrate` and `Reconcile` for every document type. Write `Id` and `BlobHash` by hand; map keys are strings.
- Set the actor ID from the device key.

Done when:

- Put, get, list and delete round-trip.
- Two repos sharing changes in memory converge after concurrent edits to the same song (lyrics in different sections) and the same service (two concurrent moves don't duplicate the item).
- A blob write is atomic and verified.

**T45. API to TypeScript.** Add `ts-rs` derives across `api` and the core types it exposes, and an `xtask` or test that writes `app/src/lib/api/*.ts`. Done when generation is reproducible and CI fails if the generated files are stale.

### Importers

Each importer is one file in `crates/lampstand-import/src/`, with fixtures in `crates/lampstand-import/tests/fixtures/<format>/` and tests in `crates/lampstand-import/tests/<format>.rs`. For each one, done means:

- Every song or verse in the fixtures imports with its title, sections, arrangement, CCLI number, copyright and author.
- Chords are preserved as ChordPro, where the format has them.
- Malformed entries go to `skipped` with a reason and never fail the whole import.

| ID | Format | Notes | Status |
| --- | --- | --- | --- |
| T04 | OpenLyrics XML | Do first: T06 reuses it. Handles `<verse name="v1">`, `<lines>` with `<br/>` and `<chord name=""/>`, and `verseOrder`. | Done |
| T05 | OpenSong | Chord lines (`.`) above lyric lines become inline ChordPro. Handles `[V1]`, `presentation` order and multi-verse lines (`1`, `2` prefixes). | Done |
| T06 | OpenLP (skipped for now: the importer returns an error suggesting an OpenLyrics export) | Reads `songs.sqlite` with `rusqlite` (bundled feature). Lyrics are OpenLyrics: call T04's parser. Also reads authors, CCLI number and copyright. | Skipped |
| T07 | SongSelect `.usr` and `.txt` | Converts `.usr` `Words=` with `/t` section separators and `Fields=` labels. Recognises `.txt` section headings. | Done |
| T08 | FreeShow `.show` | JSON: `slides`, `layouts` (arrangement), `meta` (CCLI, author, copyright). | Done |
| T09 | EasyWorship 6 and 7 | EW7 is SQLite with RTF lyrics; write a minimal RTF-to-text helper. EW6 is Paradox `.DB`: port only the reading logic from OpenLP's `easyworship.py`. GPL-2.0-or-later code is compatible with ours. | Done |
| T10 | ProPresenter 6 and 7 | Pro6 is XML with base64 RTF. Pro7 is protobuf: first check the licence of the community `.proto` definitions and note it in the PR, then generate with `prost`. | Done |
| T11 | OSIS and Zefania Bibles | Streaming `quick-xml`, because a full Bible is 4 to 6 MB. Must handle milestone `<verse sID/eID>` as well as container verses. | Done |
| T12 | USFM Bibles | `\id`, `\c`, `\v`, and dropping footnotes `\f…\f*` and cross-references `\x…\x*`. Fixture: WEB from eBible.org (public domain). | Done |
| T13 | OpenSong and OpenLP Bibles | OpenSong: `<b n><c n><v n>` XML. OpenLP: `.sqlite` `book` and `verse` tables. | Done |

## Wave 1: needs wave 0

| ID | Title | Model | Needs | PRD | Files | Status |
| --- | --- | --- | --- | --- | --- | --- |
| T21 | Song and scripture pagination | S | T03, T20 | BI-1, SL-1 | `layout/src/paginate.rs` | Done |
| T22 | Fit text with shrink-to-minimum | S | T20 | TS-7 | `layout/src/fit.rs` | Done |
| T24 | wgpu renderer core and offscreen rendering | **O** | T20 | LV-3, NFR | `render/src/` | Done |
| T29 | PPTX parser: structure, text, shapes, images, notes | **O** | T20, T28 | PS-1, PS-3 | `slides/src/` | Done |
| T14 | Bundled public-domain Bibles (KJV, WEB) | S | T11, T12 | BI-2 | `xtask/`, `assets/bibles/` | Not started |
| T34 | Countdown timers and announcement loop | S | T33 | LV-2 | `live/src/lib.rs` (`tick`) | Done |

**T21. Pagination.** Implement `paginate_song` and `paginate_passage` to their doc comments. Done when there are tests for the even-split rule, arrangements with repeats, blank-line breaks, the copyright footer on the first slide only, and passages that break between verses and inside an over-long verse.

**T22. Fit text.** Implement `fit_text`. Done when tests cover fitting at the preferred size, shrinking, the `below_minimum` flag, and wrapping. One test checks against cosmic-text directly that the chosen size fits and the next half-pixel step up does not.

**T24. Renderer core (Opus).** Add `wgpu`, `lyon` and `glyphon`, and implement `Renderer::new` and `render_offscreen` for every `NodeKind` except video. That means:

- paths (fill, stroke, dash)
- solid and gradient paints
- images with fit and crop
- text blocks (valign, paragraphs, runs, bullets, shadow, outline)
- groups, transforms and opacity

Make the GPU architecture choices for the whole crate: pipelines, atlases, how surfaces will be added in T25. Done when golden PNG tests on the software adapter cover each node kind, and CI renders them on all three OSes within tolerance.

**T29. PPTX parser (Opus).** Base it on `zip` and `quick-xml`, or on DeckCraft if T28 says so. Cover:

- presentation size
- master → layout → slide inheritance and placeholders
- theme colours and fonts
- text bodies (paragraphs, runs, bullets, `normAutofit`)
- `sp`, `grpSp`, `pic`, `cxnSp` and tables
- preset geometry → paths: the 40 most common presets, measured on the corpus; the rest become the bounding rectangle plus a fidelity issue
- solid and gradient fills, outlines, outer shadow
- notes and hidden slides
- embedded media extracted to `media`

Done when 10 synthetic decks in `slides/tests/decks/` produce snapshot scenes, and render (via T24) visibly matches their PowerPoint PNG exports.

**T14. Bundled Bibles.** Write an `xtask` that downloads KJV and WEB from eBible.org, imports them with T11 or T12, and writes postcard plus zstd blobs to `assets/bibles/`. The blobs are committed, with source and licence notes. Done when the app can load both and `John 3:16` matches the published text.

**T34. Timers and loops.** Implement `LiveEngine::tick`: auto-advance items that wrap, and countdown content updates. Done when tests use an injected clock.


## Wave 2: engines

| ID | Title | Model | Needs | PRD | Files | Status |
| --- | --- | --- | --- | --- | --- | --- |
| T17 | LAN discovery, WebSocket sync protocol, handshake | **O** | T16 | CD-2 | `sync/src/net/` | Done |
| T18 | Teams, device keys, signed membership, invites | **O** | T16 | CD-6 | `sync/src/team.rs` | Not started |
| T23 | Compositor: audience, stage and stream layouts | S | T21, T22 | LV-3, TS-2–4, TS-9, OB-1, PS-7 | `layout/src/compose.rs` | Done |
| T25 | Output windows, display mapping, transitions, animations | **O** | T24 | LV-3, NFR | `render/src/output/` | Done |
| T26 | Calibration pattern and safe-area overlay | S | T24 | TS-1, TS-8 | `render/src/calibrate.rs` | Done |
| T27 | GStreamer backend and video nodes | S | T24 | LV-4, PS-3 | `media/`, `render/src/video.rs` | Not started |
| T30 | Animations, builds and transitions from PPTX | S | T29 | PS-3 | `slides/src/anim.rs` | Done |
| T31 | Fidelity report | S | T29 | PS-4 | `slides/src/report.rs` | Not started |
| T35 | Library facade over the repo | S | T16 | SL-1, BI-2 | `library/src/lib.rs` | Done |

**T17. Network sync (Opus).** WebSocket transport (tokio-tungstenite), mDNS with `mdns-sd`, blob transfer by hash, reconnection with backoff, and the device-key challenge handshake. Done when two processes on one machine find each other and converge, a third joins through a relay-mode server, and a peer that is cut off and reconnected catches up.

The key scenario is a **service device** (a planner's laptop or the office PC) making a quick change before or during the service: a lyric fix, a reorder, an added song. It must reach the presenting machine fast. So local changes are pushed to connected peers as they happen (on `DocChanged { remote: false }`), and the full document sweep runs only on connect. Measure edit-to-reload on the presenting machine (target under 1 s on a LAN) and record it. Also offer a manual "connect to address" for networks that block mDNS. Until T18 lands, trust is a device-local paired-devices list set up with a short pairing code. Hand-over notes with the full gap analysis: the "Service device" section of the T46 notes.

**T18. Teams (Opus).** Ed25519 device keys stored in the OS keychain (`keyring`). Team document schema with signed membership entries, invite tokens and a pure `effective_role(team_doc, key, now)`. Change filtering by role at receive. Done when there are tests for forged membership, expired, reused and revoked invites, a removed member being refused, and a planner's theme edit being dropped when they are only an operator.

**T23. Compositor.** Implement `compose` to its doc comment, with insta snapshot tests of scenes for each role × {4:3, 16:9, 32:9} × {lyrics, scripture, deck, overlay black, overlay logo, nothing}. Done when the snapshots are reviewed, text always stays inside the safe area, and backgrounds bleed or fit as configured.

**T25. Outputs (Opus).** Borderless fullscreen windows on chosen displays with no webview and a hidden cursor. Match displays with `DisplayMatch`. Add the render thread, the frame loop, `OutputSink`, transitions between scenes, the animation timeline for build steps, video persistence across scenes, and panic recovery. Done when:

- A demo binary drives three windows from a script.
- Click-to-photon is under 50 ms, measured with a frame timestamp test.
- Unplugging a display never shows the desktop on the others.

**T26. Calibration.** Draw `CalibrationPattern` as a scene: numbered edges at 1% steps, corner markers and a bright outline of the current margins. Add a safe-area overlay for previews. Done when golden tests pass. Do T62 (the full TS-1 design ported from PR #5) in the same session.

**T27. Video.** `GstBackend` behind the `gstreamer` feature: `playbin3` → `appsink` (RGBA), looping, muting, seeking. Video nodes in the renderer upload frames to a texture. Report the install size the GStreamer runtime adds on Windows and macOS. Done when a test plays a bundled 2-second clip to the end and counts frames, and the install-size numbers are in the PR.

**T30. Builds and transitions.** Map `p:timing` (click, with and after previous; delays) and `p:transition` to `BuildStep`s and `Transition`s, covering the effect set in `scene::Effect` and approximating the rest. Implement `DeckSlide::scene_at_step`. Done when synthetic decks with each effect produce the expected steps.

**T31. Fidelity report.** Report missing fonts (via `FontLookup`), approximated or unsupported effects, unsupported content (SmartArt without fallback, charts, OLE, 3D) and unreachable linked media. Done when each issue kind has a synthetic deck test.

**T35. Library facade.** Implement the `Library` methods over `Repo`, with a cached decoded `BibleText`, a search index kept current from `subscribe`, and defaults (a starter theme) on first run. Done when tests run against a temporary directory.

## Wave 3: features

| ID | Title | Model | Needs | PRD | Files | Status |
| --- | --- | --- | --- | --- | --- | --- |
| T19 | Relay binary and Docker image | S | T17, T18 | CD-3 | `relay/` | Not started |
| T32 | Fidelity corpus harness (SSIM, private corpus job) | S | T24, T29 | NFR fidelity | `slides/tests/corpus.rs`, CI | Not started |
| T36 | Readiness check and pre-service caching | S | T35, T23 | CD-10, PS-6 | `library/src/readiness.rs` | Not started |
| T37 | Service packages | S | T35 | CD-8 | `library/src/package.rs` | Not started |
| T38 | CCLI usage log and report | S | T35 | RP-1 | `library/src/usage.rs` | Not started |
| T40 | Google sign-in, picker, Drive download and export | S | T39, T42 | PS-2 | `connectors/src/google.rs` | Not started |
| T41 | Linked deck updates: poll, pending, accept | S | T39, T40, T35, T87 | PS-5 | `library/src/decks.rs` | Not started |
| T42 | Local server skeleton | S | T16 | CD-2 | `server/` | Done (with T17, T43) |
| T43 | OBS browser source | S | T42, T23 | OB-1, OB-2 | `server/src/obs/`, `server/web/obs/` | Done |
| T44 | Guest upload links on the relay | S | T19 | CD-9 | `relay/src/upload.rs` | Not started |

**T19. Relay.** One binary built on axum: the sync endpoint from T17 with role checks from T18, a blob store with a size quota, and TOML configuration. Ship a Dockerfile and builds for arm64 and x86-64. Done when it runs on a Raspberry Pi 4 image in CI (QEMU) and uses under 50 MB of memory idle with 10 teams. Do T64 (Docker image, TLS guidance and relay tests ported from PR #4) in the same session. T75 (encrypted sessions) must land before the relay is used beyond a test network.

**T32. Corpus harness.** Render every slide of every corpus deck with `render_offscreen`, compute SSIM against the reference PNGs, and write an HTML report. Fail CI if the share of decks at or above 0.95 SSIM drops. The private corpus is downloaded with a CI secret, and the job is skipped on forks.

**T36. Readiness.** Implement `check` (no network) and `prepare`, which fetches missing blobs from peers and refreshes linked decks (PS-6). Done when there are tests for each `Problem` kind.

**T37. Packages.** Implement export and import to the format in `package.rs`. Done when a round trip into an empty library reproduces the service, and licensed Bible text never appears in the zip (tested).

**T38. CCLI.** Write usage entries from `Event::SongShown` (and from streamed when a stream output is enabled). Implement `report` and `to_csv`. Done when there are tests for one count per service, merging across devices, and the CSV columns.

**T40. Google.** Implement the PKCE loopback OAuth flow, the picker page served by the local server, `files.export` and `files.get`, and the version check. Before building, check the current Google rules for desktop OAuth and picker origins, and note them in the PR. Done when there are tests with a mock Google server and one manual end-to-end run, recorded in the PR.

**T41. Deck updates.** Write a poller (every 10 minutes while the app runs, and on service open) that sets `Deck.pending`. Accepting moves pending to current, unless the deck is live; then acceptance waits until it leaves live. Done when tests use mocked fetchers.

**T42. Local server.** An axum server with routes as listed in `server/src/lib.rs`, per-output keys, blob serving and the sync endpoint mounted. Done when integration tests cover keys, 404s and a WebSocket upgrade.

**T43. OBS browser source.** Write a small vanilla TypeScript page that draws `Scene` JSON in the DOM (absolutely positioned divs, SVG paths, bundled web fonts) with a transparent body, plus a WebSocket feed. Deck slides come as WebP frames from `render_offscreen`. Done when a Playwright test checks that the page matches `render_offscreen` within tolerance for a lyrics lower third, and a manual OBS 30 check is noted in the PR.

**T44. Guest uploads.** A per-service upload link (a random token with an expiry) and an upload page served by the relay. The uploaded deck becomes a `Deck` with a `GuestUpload` source plus a service item. Done when there are tests for expiry, size limits and file-type checks.

## Wave 4: the app

| ID | Title | Model | Needs | PRD | Files | Status |
| --- | --- | --- | --- | --- | --- | --- |
| T46 | Tauri 2 scaffold, API bridge, app state wiring | **O** | T25, T33, T35, T45 | — | `app/` | Done |
| T47 | Operator view | S | T46 | LV-1, LV-2, TS-8, PS-7 | `app/src/routes/operate/` | Partial (T46 slice) |
| T48 | Library and song editor | S | T46, T15 | SL-1, SL-3 | `app/src/routes/library/` | Not started |
| T49 | Service planner, live collaborative | S | T46, T02 | PL-1, PS-8 | `app/src/routes/plan/` | Partial (T46 slice) |
| T50 | Outputs setup and calibration wizard | S | T46, T26 | LV-3, TS-1, TS-2 | `app/src/routes/outputs/` | Partial (T46 slice) |
| T51 | Theme editor | S | T46, T23 | LV-4 | `app/src/routes/themes/` | Not started |
| T52 | Scripture panel | S | T46, T01, T21 | BI-1 | `app/src/routes/operate/scripture/` | Partial (planner scripture tab, #19) |
| T53 | Deck import, fidelity report, updates | S | T46, T31, T41, T87 | PS-1, PS-4, PS-5 | `app/src/routes/decks/` | Partial (add decks from file or link, #20; no fidelity report or updates) |
| T54 | Team, invites with QR, sync status | S | T46, T18 | CD-6 | `app/src/routes/team/` | Not started |
| T55 | Migration wizard | S | T46, T04–T13 | SL-4, BI-2 | `import/src/detect.rs`, `app/src/routes/welcome/` | Not started |
| T56 | Clicker mode and global shortcuts | S | T46 | PS-9 | `app/src-tauri/src/keys.rs` | Not started |
| T57 | Crash recovery journal | S | T46 | NFR Reliability | `app/src-tauri/src/journal.rs` | Not started |
| T59 | Early-adopter packaging: macOS, Linux, Windows preview, updater | S | T46 | CD-11 | `app/src-tauri/tauri.conf.json`, CI | Not started |
| T60 | Accessibility pass (WCAG 2.2 AA) | S | T47–T54 | NFR | `app/src/` | Not started |
| T61 | Performance and latency on a 2019 laptop | **O** | T47 | NFR | `docs/perf.md`, fixes anywhere | Not started |
| T88 | Launch packaging: Windows signing and Flathub | S | T59 | CD-11 | `app/src-tauri/tauri.conf.json`, CI, Flathub manifest | Not started |

**T46. App scaffold (Opus).** Set up the integration hub:

- Tauri 2 with Svelte 5, TypeScript and Vite.
- One `api` command and an `api-event` stream. The Rust side owns `Library`, `LiveEngine`, the compositor cache, `Renderer` with its `OutputSink`, the `Server` and the sync tasks.
- The data flow from the "one click" section of ARCHITECTURE.md, wired end to end.

Done when, from a fresh install, the operator can open a service with one song and send it to a window output and to OBS. This is the first vertical slice.

**T47–T54. UI screens.** Each one builds its screen against the generated API types, using the shared component kit that T46 sets up. Done for each means: keyboard-reachable, Playwright tests against a mock API, and screenshots in the PR. The operator view (T47) must cover:

- one key to go live
- blank, black, logo and clear
- the preview and live panes with the safe-area overlay (TS-8)
- flags for text below the minimum size (TS-7)

**T55. Migration wizard.** Implement `detect::installed` (the default data paths per OS and app version) and a wizard that imports songs, Bibles, sets and media in one pass and shows the skipped report. Done when there are tests with fake directory trees.

**T56. Clickers.** In clicker mode, while a service is live, register Page Up, Page Down, B and full stop with `tauri-plugin-global-shortcut`. Handle arrow keys in app windows only, including the output windows. Done when tests cover the registration lifecycle and a manual test with a USB clicker is noted in the PR.

**T57. Crash recovery.** Journal the live cursor, overlay and output state on every change, with an fsync no more than every 100 ms. On start, restore when the journal is less than 10 minutes old. Done when killing the process with `kill -9` mid-service restores the live slide in under 5 seconds in a CI test with a window output.

**T59. Early-adopter packaging.** The early-adopter release ships on macOS and Linux first (PRD, Phasing). Build a signed and notarised DMG for macOS, an AppImage and a .deb, and an unsigned NSIS installer for Windows, named and released as a preview, with release notes that say SmartScreen will warn. Add the Tauri updater with its own update-signing key, and gate it on `is_idle()` with no service starting within an hour. Done when CI produces all four artefacts on tags, using signing secrets as repository secrets, with steps documented, and the DMG opens on a clean Mac without a Gatekeeper warning.

**T88. Launch packaging.** For Phase 1: sign the Windows installer (MSI or NSIS) so SmartScreen accepts it, and publish a Flatpak on Flathub. Done when CI signs the Windows installer on tags, it installs on a clean Windows 10 machine without a SmartScreen block, and the Flathub manifest builds and is accepted.

**T60. Accessibility.** Run axe on each screen, check contrast, add focus rings, labels and shortcuts for every live action. Done when there are zero axe violations at AA and a keyboard-only mock service runs end to end.

**T61. Performance (Opus).** On 2019-class hardware (8 GB RAM, integrated graphics), a three-output service must hold 60 fps with a video background, and click-to-photon must stay under 50 ms. Profile, fix, and record the method and the numbers in `docs/perf.md`.

## Follow-ups

Tickets found while reviewing the first wave and the pull requests it superseded. They have no wave: each says what it needs.

Code from a closed pull request is still on GitHub. Fetch every pull request head once with `git fetch origin 'refs/pull/*/head:refs/remotes/pr/*'`, then read a file with `git show pr/N:path`. Port the behaviour and the tests, not the code as is: those pull requests used an older model (one library document, positional service items) that ARCHITECTURE.md replaced.

### Ported from closed PRs

| ID | Title | Model | Needs | Source | Files | Status |
| --- | --- | --- | --- | --- | --- | --- |
| T62 | Full TS-1 calibration design | S | T26 | #5 | `render/src/calibrate.rs` | Done |
| T63 | Ultra-wide text column and authored-line fitting | **O** | — | #5 | `layout/src/fit.rs`, `layout/src/compose.rs`, `core/src/theme.rs` | Not started |
| T64 | Relay Docker image, TLS guidance and relay tests | S | T19 | #4 | `relay/` | Not started |
| T65 | Corrupt chunks and stray temp files don't fail a load | S | — | #4 | `sync/src/repo/store.rs` | Not started |
| T66 | Song search by author and CCLI number; finished words | S | — | #1 | `library/src/search.rs` | Not started |
| T67 | OpenLyrics 0.9 chords | S | — | #1 | `import/src/songs/openlyrics.rs` | Not started |
| T68 | ChordPro file importer | S | — | #1 | `import/src/songs/chordpro.rs` | Not started |
| T69 | Contract decision: alternate titles and several authors | **O** | — | #1 | `core/src/song.rs`, ARCHITECTURE.md | Not started |
| T70 | Scripture slides: chapter labels and clause breaks | S | — | #3 | `layout/src/paginate.rs` | Not started |
| T71 | Chapter counts and "did you mean" for book names | S | — | #3 | `core/src/scripture.rs` | Not started |
| T72 | USFM Psalm superscription fixture | S | — | #3 | `import/tests/` | Not started |
| T73 | Every Bible format imports the same text (optional) | S | — | #3 | `import/tests/` | Not started |
| T74 | Contract decision: verse lists | **O** | — | #3 | `core/src/scripture.rs`, ARCHITECTURE.md | Not started |

**T62. Full TS-1 calibration design.** Port the pattern from `git show pr/5:crates/lampstand-render/src/calibration.rs`, drawn as `Scene` nodes rather than #5's direct drawing:

- numbered rings stepping in from each edge
- a different colour in each corner, so a flipped or rotated output is obvious
- a 10% grid
- the safe area outlined in cyan
- a label naming the output and its aspect ("32:9")
- the edge being adjusted highlighted

T26 asks for rings at 1% steps; #5 used 2%. Use 1%. Port the golden tests in `pr/5:crates/lampstand-render/tests/render.rs` that apply.

**T63. Ultra-wide text column (Opus).** Two pieces from `git show pr/5:crates/lampstand-render/src/layout.rs` and `pr/5:crates/lampstand-render/tests/layout.rs`:

- On ultra-wide outputs (TS-9), cap the text column at 2.4 times its height, centred in the safe area, so a 32:9 LED wall doesn't run one lyric line edge to edge. Port the 32:9 tests (`text_stays_in_safe_area_on_every_aspect`, `layout_adapts_to_aspect_ratio`).
- Fitting first tries to keep each authored line unbroken, shrinking towards the minimum, and only then wraps (`lyric_lines_shrink_before_wrapping`).

#5 made the cap a theme field (`max_column_aspect`). That is a contract change to `core::theme::TextStyle`; decide whether it belongs there or in `compose`, and update ARCHITECTURE.md if the contract changes.

**T64. Relay packaging and tests.** From `git show pr/4:crates/lampstand-relay/...`:

- `Dockerfile` and `Dockerfile.dockerignore`: an Alpine multi-stage build. Keep it, adjusted to the T19 binary.
- `README.md`: the guidance on putting the relay behind a TLS reverse proxy.
- `tests/relay.rs`: three tests, rewritten on `SyncNode` and `Repo`. Two peers converge through a relay (`two_peers_converge_through_relay`); a change stored on the relay reaches a peer that connects after a relay restart (`relay_stores_and_forwards_across_restarts`); teams are kept apart and strangers refused (`relay_keeps_churches_apart_and_strangers_out`).

**T65. Corrupt chunks don't lose a document.** A follow-up to T16. Today one unreadable chunk makes `Store::load` fail (`repo/store.rs`, around line 71) and the whole document is lost to the app. Skip a chunk that will not load and report it, and ignore leftover `.tmp-*` files in a document folder. Use `git show pr/4:crates/lampstand-sync/src/store.rs` (`torn_append_is_ignored_on_load`, around line 448) as the template for the tests. Done when a document with a corrupt chunk loads its other changes, and a stray temp file is ignored.

**T66. Song search additions.** A follow-up to T15, from `git show pr/1:crates/lampstand-songs/src/search.rs` and `tests/search.rs`:

- Search authors and CCLI numbers as well as titles and lyrics.
- A trailing space means the last word is finished: match it whole, not as a prefix.

Port the tests `finds_by_title_lyrics_and_author`, `finds_by_ccli_number` and `matches_prefix_while_typing`.

**T67. OpenLyrics 0.9 chords.** A follow-up to T04. OpenLyrics 0.9 writes chords as `root`, `structure` and `bass` attributes instead of `name`. See `chord_name` in `git show pr/1:crates/lampstand-songs/src/openlyrics.rs` (around line 176) and its test `reads_09_chords_with_root_and_structure`. Add the `pr/1:crates/lampstand-songs/tests/fixtures/amazing-grace.xml` fixture.

**T68. ChordPro importer.** A new file importer for `.cho`, `.chopro` and `.chordpro` files: directives (`{title}`, `{artist}`, `{ccli}`, `{copyright}`), `{start_of_verse}`-style environments and inline chords. Port from `git show pr/1:crates/lampstand-songs/src/chordpro.rs`, with the fixture `pr/1:crates/lampstand-songs/tests/fixtures/library/blessed-assurance.cho`. Register it in `detect.rs` and the importer dispatch. Done to the importer rules above.

**T69. Alternate titles and several authors (Opus).** OpenLyrics, CCLI and most song databases carry alternate titles and more than one author, and #1 kept them. `core::song::Song` has one title and one author string. Decide whether to add `alternate_titles: Vec<String>` and `authors: Vec<String>`, how search (T15, T66) and the CCLI report (T38) use them, and how existing documents migrate. Record the decision in ARCHITECTURE.md; the importers follow in their own tickets.

**T70. Scripture slide labels and clause breaks.** A follow-up to T21, from `git show pr/3:crates/lampstand-scripture/src/split.rs`:

- When a passage crosses chapters, verse labels include the chapter ("3:36", "4:1"); within one chapter they stay bare.
- A verse too long for one slide breaks between words at a clause (after a comma, semicolon or colon) rather than at the last word that fits.

Port the tests `chapter_labels_when_crossing_chapters` and `long_verse_splits_between_words_at_a_clause`.

**T71. Book chapter counts and suggestions.** A follow-up to T01, from `git show pr/3:crates/lampstand-scripture/src/book.rs`:

- A table of chapter counts for the 66 books, so "Jude 2:1" or "Ps 151" is a bounds error at parse time, not an empty lookup.
- "Did you mean" suggestions for a book name that doesn't match ("Jhon" → John), for the scripture panel (T52).

**T72. Psalm superscription fixture.** A follow-up to T12. Add `pr/3:crates/lampstand-scripture/tests/fixtures/PSA.usfm` and a test that a Psalm's superscription (`\d`) is not part of verse 1.

**T73. Bible formats agree (optional).** #3 had excerpts of the same KJV text in OpenSong, OSIS, Zefania and USFM (`pr/3:crates/lampstand-scripture/tests/fixtures/`). Add a test that every format imports the same verses with the same text.

**T74. Verse lists (Opus).** `Passage` is one range. Operators type lists: "Ps 23:1-4,6" and "Jn 3:16; 4:1". Decide whether `Passage` becomes a list of ranges or the app splits a list into several items, and how `Display`, pagination and the CCLI and usage logs treat it. #3's `pr/3:crates/lampstand-scripture/src/reference.rs` parsed lists; read it as input. Record the decision in ARCHITECTURE.md.

### Security and robustness

| ID | Title | Model | Needs | PRD | Files | Status |
| --- | --- | --- | --- | --- | --- | --- |
| T75 | Encrypted, authenticated sync sessions | **O** | — | CD-2, CD-3 | `sync/src/net/` | Done |
| T76 | Blob transfers: size cap and per-session temp files | S | — | CD-2 | `sync/src/net/session.rs` | Not started |
| T77 | Bound documents created by unknown sync messages | **O** | — | CD-1 | `sync/src/repo/mod.rs` | Not started |
| T78 | Don't advance sync state before changes are durable | **O** | — | CD-1 | `sync/src/repo/mod.rs` | Not started |
| T79 | `device.key` permissions and one atomic-write helper | S | — | NFR Security | `app/src-tauri/src/config.rs`, `sync/src/repo/fsutil.rs` | Not started |
| T80 | Size caps on zip entries, deck downloads and Bible decompression | S | — | NFR Reliability | `import/src/songs/opensong.rs`, `connectors/src/links.rs`, `library/src/lib.rs` | Not started |
| T81 | Share links: `percent_decode` and filename sanitising | S | — | PS-1 | `connectors/src/links.rs` | Not started |
| T82 | Render thread: decode off-thread, one shaping helper | **O** | — | LV-3, NFR | `render/src/`, `layout/src/` | Not started |
| T83 | Render tests fail without a GPU when CI expects one | S | — | NFR | `render/tests/`, `.github/workflows/` | Not started |
| T84 | Contract decision: deck builds on recompose | **O** | — | PS-3 | `layout/src/compose.rs`, `slides/`, ARCHITECTURE.md | Done (with T30) |
| T85 | Mark and track the remaining render stubs | S | — | LV-4 | `render/src/frame.rs`, `layout/src/compose.rs` | Not started |
| T86 | Wire the OBS browser source into the app | S | T43, T46 | OB-1, OB-2 | `app/src-tauri/src/`, Outputs view | Not started |
| T87 | Deck budget: price media, one ~1 GB peak | **O** | T29 | NFR Reliability, NFR Hardware | `slides/src/pptx/budget.rs`, `slides/tests/memory.rs`, ARCHITECTURE.md | Done |

**T75. Encrypted sync sessions (Opus).** `sync::net` dials plain `ws://`, and after the handshake no frame is authenticated: anyone on the path can read every song and service, and inject or change frames. Derive a session key bound to the handshake transcript (both device keys, both nonces, the SPAKE2 messages), then encrypt and authenticate every later frame with an AEAD and per-direction counters. Noise (`snow`, XX or IK pattern with the device keys) is a reasonable alternative to building it on the current handshake; record the choice in `docs/decisions/`. Done when a test shows a tampered, replayed or reordered frame closes the session, and a capture shows no plaintext. This blocks T19 and any use beyond a test LAN.

**T76. Blob transfer limits.** In `net/session.rs` (around lines 285–303), an incoming blob is written to a shared `incoming/{hash}.part` with no size limit:

- Cap a blob at a configured maximum and drop the transfer past it.
- Use a separate temp file per session (or per transfer), so two peers sending the same blob at once can't interleave writes into one file.

Done when tests cover an oversized blob and two concurrent senders of the same hash.

**T77. Unknown-id sync messages (Opus).** A sync message for an id this device has never seen creates an in-memory document and fixes its kind from the message (`repo/mod.rs`, around line 333). A peer can make a device hold any number of empty documents, and the first message decides the kind for good. Bound how many such documents a peer can create, and don't record the kind until a change is actually applied. Done when tests cover a flood of unknown ids and a first message that carries no changes.

**T78. Durable before acknowledged (Opus).** After receiving changes, `Repo` updates the in-memory document and the per-peer sync state, then flushes (`repo/mod.rs`, around lines 350–372). If the final flush fails, the sync state already says the changes were received, so the peer never resends them and a restart loses them. Don't advance `saved` or the sync state until the write is durable. Done when a test with an injected write failure shows the changes arrive again after the next connect.

**T79. Device key file.** `app/src-tauri/src/config.rs` (around line 89) writes `device.key` with default permissions and without fsync. Write it with mode 0600 on Unix (and owner-only on Windows), and fsync the file and its directory. There are three atomic-write helpers (`config.rs` `write_atomically`, and the method and function in `sync/src/repo/fsutil.rs`); merge them into one that the app reuses. Done when a test checks the key's mode on Unix.

**T80. Size caps.** Three reads have no upper bound:

- zip entries read to the end in `import/src/songs/opensong.rs` (around line 97)
- deck downloads in `connectors/src/links.rs` (around line 172)
- zstd decompression of Bible text in `library/src/lib.rs` (around line 47)

Cap each (a song file at a few MB, a deck at the T44 upload limit, a Bible at about 50 MB decompressed) and return an error past it. Done when each has a test with an oversized input.

**T81. Share-link decoding.** In `connectors/src/links.rs` (around line 220), `percent_decode` checks `i + 2 < len`, so an escape at the very end ("%20") is left encoded, and it slices `&s[i + 1..i + 3]`, which panics if those bytes are not a character boundary. Fix both, and sanitise the `Content-Disposition` filename: strip path separators, control characters and leading dots before it is used as a file name. Done when tests cover a trailing escape, a multi-byte character after `%`, and `../` in a filename.

**T82. Render thread work (Opus).** Two things that make a click slower than it should be or text inconsistent:

- `render/src/images.rs` `decode` runs on the render thread from the engine's draw path, so a large background image stalls every output. Decode on a worker and show the previous frame (or nothing) until it is ready.
- Pagination and footer shaping in `layout` measure with different font stand-ins from `fit.rs` and `render/src/text.rs`, so a slide can paginate to fit and then overflow. Use the same stand-ins everywhere, ideally through one shared cosmic-text buffer helper.

Done when a test shows a large image doesn't delay a text slide on another output, and a test checks that pagination, fit and render agree on line breaks for a stand-in font.

**T83. GPU tests that can't pass silently.** Render tests skip when no adapter is found, so a broken lavapipe install on the Linux CI job passes. Read an environment variable (`LAMPSTAND_REQUIRE_GPU=1`, set on the lavapipe job) and fail instead of skipping when it is set. Add a test for `SurfaceLost`: the output recreates its surface and keeps drawing.

**T84. Deck builds on recompose (Opus).** Decide before T30. `compose.rs` ignores `Content::DeckSlide.step`, and `layout` can't call `slides::scene_at_step` (it doesn't depend on `slides`). So any recompose (a theme edit, an output resize, a synced change) shows the slide as authored and built-up content vanishes. Options: carry the stepped scene in `Content`, move `scene_at_step` to `core`, or let the app compose deck slides itself. Record the choice in ARCHITECTURE.md.

**T85. Remaining stubs.** Two stubs are not marked with a ticket: video nodes draw nothing (`render/src/frame.rs`, around line 265, belongs to T27), and the logo overlay is a placeholder ring (`layout/src/compose.rs`) until the contract has a logo asset. Mark each with `todo`-style comments naming its ticket, add a logo asset to the theme or output contract, and make the operator view say when an overlay is a placeholder.

**T86. Wire the OBS browser source into the app.** T43 built the server half (`SceneHub`, the routes and the page) but nothing in the app uses it, so OB-1 and OB-2 are not met for users until this lands. Build one `SceneHub` and give it to `AppBackend`, which delegates `output_key_valid` and `watch_scene` to it. Run a compose loop for each enabled stream output that calls `hub.publish` whenever the program, theme or output config changes. Make `Backend::renderer()` return the app's thumbnail `Renderer`, so deck slides go out as frames. Keep each stream output's browser-source key in device-local config next to the output config (not in a synced document), and restore it with `SceneHub::add_output_with_key` at start so the URL pasted into OBS survives restarts. Show and copy the URL (`http://<host>:<port>/obs/<output>?k=<key>`) and a "new key" button (`rotate_key`) in the Outputs view. Done when a test publishes a program change and a WebSocket client receives the stream scene, the key survives a restart, and a manual OBS 30 check (outlined lyrics in particular) is noted in the PR.

**T87. Deck budget covers media (Opus).** The T29 parser refuses decks whose parse would allocate too much, but the estimate doesn't count the bytes DeckCraft holds for entries that aren't XML. Its `Package` keeps every entry inflated, and `media_for` copies each referenced media part. So an entry that stops the XML scan at once and then compresses well costs nothing in the estimate and about twice its size in memory. In [the fifth review of #15](https://github.com/incrhst/lampstand/pull/15#pullrequestreview-5579843423), a 1.06 MB deck (two images replaced by `<!X` plus 500 MiB of zeros) was accepted and peaked at 2.1 to 2.4 GB. That's enough to push the NFR laptop (8 GB, three outputs live) into swap during a service.

- Make the budget one number that bounds the whole import peak. Charge every entry's inflated bytes at 2× (the package plus DeckCraft's copy) on top of the parse estimate, whatever the parse does.
- Set the default total to about 1 GB. Past it, refuse with `TooLarge`; T31 can then suggest linking a video instead of embedding it.
- Add a media case to `slides/tests/memory.rs` (the `<!X` deck, scaled to the budget), and include media in the peak-ceiling assertion.
- Correct ARCHITECTURE.md ("Hostile decks"): it says a deck's peak is "about its unpacked size plus the parse estimate" and quotes 512 MB and 1 GiB unpacked as separate limits. Replace them with the one total this ticket enforces.
- Real decks must still pass: a 200-slide sermon deck with 100 photos peaked at 148 MB in the review, and a deck with one 350 MB embedded video should fit.

Done when the review's media repro is refused, the counting-allocator test bounds a media-heavy deck under the 1 GB total, and the existing deck snapshots are unchanged. T41 and T53 wait on this, so no deck reaches the parser from the app or from a linked source before it lands. Importing in a child process with its own memory limit is the longer-term answer (ARCHITECTURE already allows for one); propose it as a separate ticket if the numbers here are still uncomfortable.

### Managing OBS

[Decision 0005](decisions/0005-manage-obs-not-replace.md): OBS stays the streaming engine, and Lampstand drives it so a volunteer never opens OBS during a service.

| ID | Title | Model | Needs | PRD | Files |
| --- | --- | --- | --- | --- | --- |
| T88 | OBS WebSocket v5 client | S | — | OB-4 | `connectors/src/obs.rs` |
| T89 | OBS connection, detection and status in Present | S | T88, T46 | OB-5 | `app/src-tauri/src/`, `app/src/routes/` |
| T90 | Service items switch the OBS scene | **O** | T88 | OB-4 | `core/src/service.rs`, `live/`, ARCHITECTURE.md |
| T91 | OBS scene buttons and audio mutes in Present | S | T89 | OB-8 | `app/src/routes/operate/` |
| T92 | Start and stop streaming and recording from the service order | S | T89 | OB-6 | `core/src/service.rs`, `live/`, `app/src/routes/` |
| T93 | One-click OBS setup | S | T86, T88 | OB-7 | `connectors/src/obs.rs`, Outputs view |
| T94 | Stream destination and virtual camera | S | T89 | OB-9 | `app/src-tauri/src/`, `app/src/routes/` |

**T88. OBS WebSocket client.** A client for obs-websocket protocol v5 (OBS 28 and later): `Hello`, `Identify` with the SHA-256 challenge, `Request` by id, and event subscriptions (scene, stream, record, virtual camera, input mute, `ExitStarted`). Reconnect with backoff, and expose a status stream (disconnected, connecting, connected, auth failed) plus typed calls for what T89 to T93 need. Behind a `obs` feature like `net`. Done when tests against a mock server cover auth, a wrong password, a dropped connection that reconnects, and an event arriving while a request is in flight.

**T89. Connection and status.** Setup gets an OBS section: host (default `localhost`), port (default 4455), password and a "Test connection" button. The password lives in device-local config with T79's permissions, never in a synced document. Present shows a status pill for connected, streaming and recording, with timecodes and dropped frames, and a clear warning when OBS disconnects. Projection never waits on OBS.

Tell people why OBS isn't connected. A refused connection on `localhost` means OBS isn't running, or its WebSocket server is off (it is off by default in OBS 28 and later, under Tools › WebSocket Server Settings). So check whether OBS is installed (macOS `/Applications/OBS.app`; Windows Program Files or the registry; Linux `obs` on `PATH` or the flatpak `com.obsproject.Studio`) and whether it is running (a process check). Then Present and readiness (T36) say one of:

- "OBS isn't running", with an **Open OBS** button that launches it with `--minimize-to-tray` (and `--collection Lampstand` once T93 has made that collection);
- "OBS isn't installed", with a **Get OBS** link to obsproject.com;
- "OBS is running but its WebSocket server is off", with step-by-step instructions to turn it on and copy the password.

Only say this when the service has a stream output or items with an OBS scene, so churches that don't stream never see it. Don't edit OBS's own config files (to switch the server on, say) without asking the user first. Done when a test drives the status through every state with the mock server, tests cover each detection outcome, and a manual OBS 30 check is noted in the PR.

**T90. Scene per service item (Opus).** Add an optional OBS scene name to a service item (a contract change to `core`; record it in ARCHITECTURE.md). When the item goes live, call `SetCurrentProgramScene`. If OBS isn't connected, or the scene no longer exists, show a warning in Present and carry on. Readiness (T36) flags items whose scene OBS doesn't have, when OBS is connected. Decide whether Previous also switches the scene back, and what happens while the operator holds a manual scene from T91. Done when tests cover go-live, a missing scene, a disconnected OBS and a manual hold.

**T91. Run OBS from Present.** Show OBS's scenes as buttons, with the program scene marked and following OBS's own events. Clicking a button switches the scene. Show a mute toggle for each audio input that follows `InputMuteStateChanged`. No volume faders: mixing stays in OBS or on the desk. Done when the UI follows changes made in OBS itself as well as in Lampstand.

**T92. Stream and record from the service order.** A service item (typically the pre-service countdown) can start streaming and/or recording when it goes live. Stopping is never automatic: Present has a stop button that asks for confirmation. Done when tests cover starting when already streaming, OBS refusing (no stream key), and a disconnected OBS.

**T93. One-click setup.** A "Set up OBS" button creates a scene collection named "Lampstand" (if it doesn't exist), with starter scenes ("Slides", "Worship", "Pastor and slides"). Each gets a browser source named "Lampstand stream" set to the T86 URL at the output's size, with "shutdown source when not visible" off. Running it again updates only the URL and size of sources it created, found by name, and never edits or removes anything else. Rotating the key (T86) updates the URL in OBS when connected. Done when tests against the mock server show a fresh setup, a re-run that changes nothing, and a re-run after a key rotation.

**T94. Stream destination and virtual camera.** T88 has the calls: `stream_destination` and `set_stream_destination` (OBS's listed services such as "YouTube - RTMPS" or "Facebook Live", or a custom RTMP or RTMPS server), and starting, stopping and reading the virtual camera, with its `VirtualCamStateChanged` event. Setup's OBS section gets the destination: a service picker plus stream key, or a custom server and key. Present gets a virtual camera toggle that follows OBS's own events, for churches that send the program into Zoom or Teams. The stream key is a secret: keep it device-local with T79's permissions, never synced, and never logged. OBS refuses to change the destination while it is streaming, so the UI says so instead of failing silently. OBS sends to one destination; streaming to several at once needs a plugin (such as obs-multi-rtmp) or a restream service, and the UI says that too rather than offering a list. Done when tests against the mock server cover reading and setting each kind of destination, a refusal while streaming, and the virtual camera toggling from either side.

### Tablet remote

[Decision 0006](decisions/0006-tablet-remote-in-the-browser.md): an operator remote in the tablet's browser, served over plain HTTP by the local server. A tablet opens it by QR code and pairs with a fresh PIN, and a paired tablet can drive the service but nothing else. A second Lampstand machine, paired as a sync peer, can drive the service too (T98), through the same gate.

| ID | Title | Model | Needs | PRD | Files |
| --- | --- | --- | --- | --- | --- |
| T95 | Remote pairing, sessions and the remote socket | **O** | T42 | LV-5, NFR Security | `server/src/remote/`, `app/src-tauri/src/sync.rs` |
| T96 | Operator remote page | S | T95 | LV-5 | `app/src/remote/`, `app/vite.config.ts` |
| T97 | Connect a tablet: QR code, PIN and paired tablets | S | T95 | LV-5 | `app/src/routes/operate/`, `app/src/routes/devices/` |
| T98 | Run the service from another Lampstand machine | **O** | T95 | LV-5, CD-2 | `sync/src/net/wire.rs`, `sync/src/net/session.rs`, `app/src-tauri/src/sync.rs`, `app/src/routes/operate/` |

**T95. Remote pairing and sessions (Opus).** Add `server::remote`. Starting pairing makes a fresh six-digit PIN that lasts five minutes. Five wrong tries void it, and only one pairing runs at a time, as in device pairing. `POST /remote/pair` with the right PIN sets a random 256-bit session token in an `HttpOnly`, `SameSite=Strict` cookie scoped to `/remote`. Keep only each token's hash, with the tablet's name and when it was paired and last seen, in a device-local `remotes.json` beside `paired.json`, never in a synced document. `WS /remote/ws` needs a valid session. It accepts `Request::Live` and nothing else: any other request is refused and logged. Put that check in one gate on the app side, not in the server's socket handler, so T98's sync peers go through the same one. It sends the current `Event::Live` and `Event::Loaded` on connect and then each change. Revoking a session closes its socket at once. Add `TeamRequest` variants (or a new `RemoteRequest` area) to start and stop pairing, list sessions and revoke one. The PIN goes over plain HTTP, so rate-limit `/remote/pair` per address as well as per PIN. Done when tests cover a right and a wrong PIN, expiry, the fifth wrong try, a refused non-`Live` request, a revoked session's socket closing, and a session surviving a restart. Measure command to `Event::Live` over the socket (PRD target: under 150 ms on local Wi-Fi) and note it in ARCHITECTURE.md.

**T96. Operator remote page.** Add a second Vite entry served at `GET /remote`. It reuses `app/src/lib/kit/` and the generated API types, with a transport that speaks the T95 socket instead of Tauri IPC. Without a session it shows the PIN form. With one it shows the loaded service's order and slides, live and preview marked, and large Next and Previous buttons, plus Black, Logo and Clear. Tapping a slide sends `Show`. It must work in portrait and landscape on a tablet, with touch targets at least 44 pt, and on Safari 16 and Chrome on Android. Slide thumbnails come from the server's WebP frame path where a slide is a deck frame, and are drawn from the scene otherwise. Keep the screen awake with a muted, looping, inline video while the page is open (no Wake Lock: plain HTTP isn't a secure context). Reconnect with backoff when the socket drops, and show that it's offline. Check the bundle in like `web/obs/dist`, so cargo needs no Node. Done when a Playwright test pairs, advances a slide and sees the live marker follow, and when a change made on the laptop shows on the page.

**T97. Connect a tablet.** Present gets a "Connect a tablet" button that starts T95 pairing and shows a QR code for `http://<lan-ip>:<port>/remote`, using the address and port the server actually has, with the PIN in large type beside it and a countdown. Pairing a tablet closes the dialog and names it. Beside the code, a "not loading?" note covers the common causes: the tablet on another network, guest Wi-Fi that isolates devices, and the OS firewall prompt. It recommends a separate Wi-Fi or VLAN for the AV team. Setup → Devices lists paired tablets, with when each was last seen and Revoke. Done when the dialog shows the right address with more than one network interface, and revoking a tablet on the Devices screen disconnects its open page.

**T98. Remote control from a peer (Opus).** Add two frames to `sync::net`: `LiveCommand`, carrying a live `Command` from a controlling peer, and `LiveState`, carrying what `Event::Live` carries, sent to every connected peer on each change and once on connect. Old peers can't read them, so bump `PROTOCOL` to 3 and check that a version-2 peer is refused with a readable reason. The presenting machine passes each `LiveCommand` through T95's gate. Until T18 any paired peer passes it. Leave a `// T18:` hook so only operators and admins do afterwards. In the app, Present gets a "Control <machine>" choice when a paired peer is presenting. While it's on, Present shows that peer's live state, sends its commands there, and draws slides from the synced documents. Its own live engine and outputs stay idle. When the link drops, Present says so and stops sending. The presenting machine carries on, and its operator can take over at once. Two machines must never both drive the same outputs. Done when a two-process test sends Next from one machine and sees the other's live state change and come back, a version-2 peer is refused, a dropped link leaves the presenting machine where it was, and the command-to-state time on a LAN is measured and noted in ARCHITECTURE.md (PRD target: under 150 ms).

## Not code: start now

These sit on the critical path, but no session can do them:

- Apply for CCLI SongSelect API access and Bible publisher licences (PRD risk table).
- Start Google OAuth app verification for the `drive.file` scope and the Picker.
- Assemble the 200-deck fidelity corpus with permission from the churches who supply them, plus PowerPoint PNG exports of each.
- Set up an Apple Developer ID now: the early-adopter release (T59) needs it.
- Get a Windows code-signing certificate and a Flathub account before launch (T88).
- Decide the open PRD questions: the public relay (CD-4) and the product name.

## Parallelism at a glance

- **Wave 0:** 11 tickets plus 10 importers, all independent. T16 (Opus) is the longest; start it first.
- **Wave 1:** T24 and T29 (Opus) join T16 on the critical path; start them as soon as T20 and T28 land.
- **Waves 2 and 3:** split cleanly by crate.
- **Wave 4:** fans out once T46 lands.
