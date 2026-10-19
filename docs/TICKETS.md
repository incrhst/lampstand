# Phase 1 tickets

Phase 1 is every P0 requirement in [PRD.md](PRD.md). Each ticket is sized for one working session and one pull request, and touches only the files it names, so tickets in the same wave can run side by side in separate worktrees. Read [ARCHITECTURE.md](ARCHITECTURE.md) first.

**Model.** **S** tickets are well specified and suit Sonnet. **O** tickets set architecture that other tickets build on, carry the main project risks, or need judgement the spec can't give; give them to Opus. Every pull request gets an Opus review before merge.

**Done means:**

- The ticket's acceptance tests pass, with their `#[ignore]` removed.
- New behaviour has tests.
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` are clean.
- No `todo!("Tnn")` for this ticket remains.
- Any contract change is reflected in ARCHITECTURE.md in the same pull request.

**Brief for a session.** The ticket text plus: "Read docs/ARCHITECTURE.md and the doc comments of the crate you're working in. Touch only the files this ticket names, unless the contract has to change, and then say why."

## Wave 0: foundations (no dependencies; all parallel)

Where tickets in one crate each add a module, each also adds one `mod` line to that crate's `lib.rs`. Those one-line conflicts are expected; resolve them by keeping both lines.

| ID | Title | Model | PRD | Files |
| --- | --- | --- | --- | --- |
| T01 | Scripture references, display and verse lookup | S | BI-1 | `core/src/scripture.rs` |
| T02 | Fractional positions and service ordering | S | PL-1 | `core/src/service.rs` |
| T03 | Section kinds and ChordPro helpers | S | SL-1 | `core/src/song.rs` |
| T15 | Song search index with typo tolerance | S | SL-3 | `library/src/search.rs` |
| T20 | Font set: bundled fonts, substitutions, `FontLookup` | S | PS-4, LV-4 | `layout/src/fonts.rs`, `assets/fonts/` |
| T28 | DeckCraft evaluation spike | S | PS-1 | `docs/decisions/0001-pptx-reader.md` |
| T33 | Live engine state machine | S | LV-1, LV-2, PS-8 | `live/src/lib.rs` |
| T39 | Share-link classification and download | S | PS-1, CD-9 | `connectors/src/links.rs` |
| T58 | CI on Windows, macOS and Linux | S | NFR Platforms | `.github/workflows/` |
| T16 | Sync repo: documents, storage, blob store | **O** | CD-1 | `sync/src/lib.rs`, `sync/src/repo/`, `core` (feature only) |
| T45 | API types to TypeScript | S | — | `api/`, `app/src/lib/api/` |
| T04–T13 | Importers (see below) | S | SL-4, BI-2 | one file each in `import/src/` |

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

| ID | Format | Notes |
| --- | --- | --- |
| T04 | OpenLyrics XML | Do first: T06 reuses it. Handles `<verse name="v1">`, `<lines>` with `<br/>` and `<chord name=""/>`, and `verseOrder`. |
| T05 | OpenSong | Chord lines (`.`) above lyric lines become inline ChordPro. Handles `[V1]`, `presentation` order and multi-verse lines (`1`, `2` prefixes). |
| T06 | OpenLP (skipped for now: the importer returns an error suggesting an OpenLyrics export) | Reads `songs.sqlite` with `rusqlite` (bundled feature). Lyrics are OpenLyrics: call T04's parser. Also reads authors, CCLI number and copyright. |
| T07 | SongSelect `.usr` and `.txt` | Converts `.usr` `Words=` with `/t` section separators and `Fields=` labels. Recognises `.txt` section headings. |
| T08 | FreeShow `.show` | JSON: `slides`, `layouts` (arrangement), `meta` (CCLI, author, copyright). |
| T09 | EasyWorship 6 and 7 | EW7 is SQLite with RTF lyrics; write a minimal RTF-to-text helper. EW6 is Paradox `.DB`: port only the reading logic from OpenLP's `easyworship.py`. GPL-2.0-or-later code is compatible with ours. |
| T10 | ProPresenter 6 and 7 | Pro6 is XML with base64 RTF. Pro7 is protobuf: first check the licence of the community `.proto` definitions and note it in the PR, then generate with `prost`. |
| T11 | OSIS and Zefania Bibles | Streaming `quick-xml`, because a full Bible is 4 to 6 MB. Must handle milestone `<verse sID/eID>` as well as container verses. |
| T12 | USFM Bibles | `\id`, `\c`, `\v`, and dropping footnotes `\f…\f*` and cross-references `\x…\x*`. Fixture: WEB from eBible.org (public domain). |
| T13 | OpenSong and OpenLP Bibles | OpenSong: `<b n><c n><v n>` XML. OpenLP: `.sqlite` `book` and `verse` tables. |

## Wave 1: needs wave 0

| ID | Title | Model | Needs | PRD | Files |
| --- | --- | --- | --- | --- | --- |
| T21 | Song and scripture pagination | S | T03, T20 | BI-1, SL-1 | `layout/src/paginate.rs` |
| T22 | Fit text with shrink-to-minimum | S | T20 | TS-7 | `layout/src/fit.rs` |
| T24 | wgpu renderer core and offscreen rendering | **O** | T20 | LV-3, NFR | `render/src/` |
| T29 | PPTX parser: structure, text, shapes, images, notes | **O** | T20, T28 | PS-1, PS-3 | `slides/src/` |
| T14 | Bundled public-domain Bibles (KJV, WEB) | S | T11, T12 | BI-2 | `xtask/`, `assets/bibles/` |
| T34 | Countdown timers and announcement loop | S | T33 | LV-2 | `live/src/lib.rs` (`tick`) |

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

| ID | Title | Model | Needs | PRD | Files |
| --- | --- | --- | --- | --- | --- |
| T17 | LAN discovery, WebSocket sync protocol, handshake | **O** | T16 | CD-2 | `sync/src/net/` |
| T18 | Teams, device keys, signed membership, invites | **O** | T16 | CD-6 | `sync/src/team.rs` |
| T23 | Compositor: audience, stage and stream layouts | S | T21, T22 | LV-3, TS-2–4, TS-9, OB-1, PS-7 | `layout/src/compose.rs` |
| T25 | Output windows, display mapping, transitions, animations | **O** | T24 | LV-3, NFR | `render/src/output/` |
| T26 | Calibration pattern and safe-area overlay | S | T24 | TS-1, TS-8 | `render/src/calibrate.rs` |
| T27 | GStreamer backend and video nodes | S | T24 | LV-4, PS-3 | `media/`, `render/src/video.rs` |
| T30 | Animations, builds and transitions from PPTX | S | T29 | PS-3 | `slides/src/anim.rs` |
| T31 | Fidelity report | S | T29 | PS-4 | `slides/src/report.rs` |
| T35 | Library facade over the repo | S | T16 | SL-1, BI-2 | `library/src/lib.rs` |

**T17. Network sync (Opus).** WebSocket transport (tokio-tungstenite), mDNS with `mdns-sd`, blob transfer by hash, reconnection with backoff, and the device-key challenge handshake. Done when two processes on one machine find each other and converge, a third joins through a relay-mode server, and a peer that is cut off and reconnected catches up.

The key scenario is a **service device** (a planner's laptop or the office PC) making a quick change before or during the service: a lyric fix, a reorder, an added song. It must reach the presenting machine fast. So local changes are pushed to connected peers as they happen (on `DocChanged { remote: false }`), and the full document sweep runs only on connect. Measure edit-to-reload on the presenting machine (target under 1 s on a LAN) and record it. Also offer a manual "connect to address" for networks that block mDNS. Until T18 lands, trust is a device-local paired-devices list set up with a short pairing code. Hand-over notes with the full gap analysis: the "Service device" section of the T46 notes.

**T18. Teams (Opus).** Ed25519 device keys stored in the OS keychain (`keyring`). Team document schema with signed membership entries, invite tokens and a pure `effective_role(team_doc, key, now)`. Change filtering by role at receive. Done when there are tests for forged membership, expired, reused and revoked invites, a removed member being refused, and a planner's theme edit being dropped when they are only an operator.

**T23. Compositor.** Implement `compose` to its doc comment, with insta snapshot tests of scenes for each role × {4:3, 16:9, 32:9} × {lyrics, scripture, deck, overlay black, overlay logo, nothing}. Done when the snapshots are reviewed, text always stays inside the safe area, and backgrounds bleed or fit as configured.

**T25. Outputs (Opus).** Borderless fullscreen windows on chosen displays with no webview and a hidden cursor. Match displays with `DisplayMatch`. Add the render thread, the frame loop, `OutputSink`, transitions between scenes, the animation timeline for build steps, video persistence across scenes, and panic recovery. Done when:

- A demo binary drives three windows from a script.
- Click-to-photon is under 50 ms, measured with a frame timestamp test.
- Unplugging a display never shows the desktop on the others.

**T26. Calibration.** Draw `CalibrationPattern` as a scene: numbered edges at 1% steps, corner markers and a bright outline of the current margins. Add a safe-area overlay for previews. Done when golden tests pass.

**T27. Video.** `GstBackend` behind the `gstreamer` feature: `playbin3` → `appsink` (RGBA), looping, muting, seeking. Video nodes in the renderer upload frames to a texture. Report the install size the GStreamer runtime adds on Windows and macOS. Done when a test plays a bundled 2-second clip to the end and counts frames, and the install-size numbers are in the PR.

**T30. Builds and transitions.** Map `p:timing` (click, with and after previous; delays) and `p:transition` to `BuildStep`s and `Transition`s, covering the effect set in `scene::Effect` and approximating the rest. Implement `DeckSlide::scene_at_step`. Done when synthetic decks with each effect produce the expected steps.

**T31. Fidelity report.** Report missing fonts (via `FontLookup`), approximated or unsupported effects, unsupported content (SmartArt without fallback, charts, OLE, 3D) and unreachable linked media. Done when each issue kind has a synthetic deck test.

**T35. Library facade.** Implement the `Library` methods over `Repo`, with a cached decoded `BibleText`, a search index kept current from `subscribe`, and defaults (a starter theme) on first run. Done when tests run against a temporary directory.

## Wave 3: features

| ID | Title | Model | Needs | PRD | Files |
| --- | --- | --- | --- | --- | --- |
| T19 | Relay binary and Docker image | S | T17, T18 | CD-3 | `relay/` |
| T32 | Fidelity corpus harness (SSIM, private corpus job) | S | T24, T29 | NFR fidelity | `slides/tests/corpus.rs`, CI |
| T36 | Readiness check and pre-service caching | S | T35, T23 | CD-10, PS-6 | `library/src/readiness.rs` |
| T37 | Service packages | S | T35 | CD-8 | `library/src/package.rs` |
| T38 | CCLI usage log and report | S | T35 | RP-1 | `library/src/usage.rs` |
| T40 | Google sign-in, picker, Drive download and export | S | T39, T42 | PS-2 | `connectors/src/google.rs` |
| T41 | Linked deck updates: poll, pending, accept | S | T39, T40, T35 | PS-5 | `library/src/decks.rs` |
| T42 | Local server skeleton | S | T16 | CD-2 | `server/` |
| T43 | OBS browser source | S | T42, T23 | OB-1, OB-2 | `server/src/obs/`, `server/web/obs/` |
| T44 | Guest upload links on the relay | S | T19 | CD-9 | `relay/src/upload.rs` |

**T19. Relay.** One binary built on axum: the sync endpoint from T17 with role checks from T18, a blob store with a size quota, and TOML configuration. Ship a Dockerfile and builds for arm64 and x86-64. Done when it runs on a Raspberry Pi 4 image in CI (QEMU) and uses under 50 MB of memory idle with 10 teams.

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

| ID | Title | Model | Needs | PRD | Files |
| --- | --- | --- | --- | --- | --- |
| T46 | Tauri 2 scaffold, API bridge, app state wiring | **O** | T25, T33, T35, T45 | — | `app/` |
| T47 | Operator view | S | T46 | LV-1, LV-2, TS-8, PS-7 | `app/src/routes/operate/` |
| T48 | Library and song editor | S | T46, T15 | SL-1, SL-3 | `app/src/routes/library/` |
| T49 | Service planner, live collaborative | S | T46, T02 | PL-1, PS-8 | `app/src/routes/plan/` |
| T50 | Outputs setup and calibration wizard | S | T46, T26 | LV-3, TS-1, TS-2 | `app/src/routes/outputs/` |
| T51 | Theme editor | S | T46, T23 | LV-4 | `app/src/routes/themes/` |
| T52 | Scripture panel | S | T46, T01, T21 | BI-1 | `app/src/routes/operate/scripture/` |
| T53 | Deck import, fidelity report, updates | S | T46, T31, T41 | PS-1, PS-4, PS-5 | `app/src/routes/decks/` |
| T54 | Team, invites with QR, sync status | S | T46, T18 | CD-6 | `app/src/routes/team/` |
| T55 | Migration wizard | S | T46, T04–T13 | SL-4, BI-2 | `import/src/detect.rs`, `app/src/routes/welcome/` |
| T56 | Clicker mode and global shortcuts | S | T46 | PS-9 | `app/src-tauri/src/keys.rs` |
| T57 | Crash recovery journal | S | T46 | NFR Reliability | `app/src-tauri/src/journal.rs` |
| T59 | Packaging, signing, updater | S | T46 | CD-11 | `app/src-tauri/tauri.conf.json`, CI |
| T60 | Accessibility pass (WCAG 2.2 AA) | S | T47–T54 | NFR | `app/src/` |
| T61 | Performance and latency on a 2019 laptop | **O** | T47 | NFR | `docs/perf.md`, fixes anywhere |

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

**T59. Packaging.** Build a signed MSI or NSIS installer for Windows, a signed and notarised DMG for macOS, an AppImage and a .deb, and a Flathub manifest. Gate the updater on `is_idle()` with no service starting within an hour. Done when CI produces artefacts on tags, using signing secrets as repository secrets, with steps documented.

**T60. Accessibility.** Run axe on each screen, check contrast, add focus rings, labels and shortcuts for every live action. Done when there are zero axe violations at AA and a keyboard-only mock service runs end to end.

**T61. Performance (Opus).** On 2019-class hardware (8 GB RAM, integrated graphics), a three-output service must hold 60 fps with a video background, and click-to-photon must stay under 50 ms. Profile, fix, and record the method and the numbers in `docs/perf.md`.

## Not code: start now

These sit on the critical path, but no session can do them:

- Apply for CCLI SongSelect API access and Bible publisher licences (PRD risk table).
- Start Google OAuth app verification for the `drive.file` scope and the Picker.
- Assemble the 200-deck fidelity corpus with permission from the churches who supply them, plus PowerPoint PNG exports of each.
- Set up Apple Developer ID, a Windows code-signing certificate and a Flathub account.
- Decide the open PRD questions: the public relay (CD-4) and the product name.

## Parallelism at a glance

- **Wave 0:** 11 tickets plus 10 importers, all independent. T16 (Opus) is the longest; start it first.
- **Wave 1:** T24 and T29 (Opus) join T16 on the critical path; start them as soon as T20 and T28 land.
- **Waves 2 and 3:** split cleanly by crate.
- **Wave 4:** fans out once T46 lands.
