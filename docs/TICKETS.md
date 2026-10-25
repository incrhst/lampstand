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
| T01 | Scripture references, display and verse lookup | S | BI-1 | `core/src/scripture.rs` | Done ([#7](https://github.com/incrhst/lampstand/pull/7)) |
| T02 | Fractional positions and service ordering | S | PL-1 | `core/src/service.rs` | Done ([#7](https://github.com/incrhst/lampstand/pull/7)) |
| T03 | Section kinds and ChordPro helpers | S | SL-1 | `core/src/song.rs` | Done ([#7](https://github.com/incrhst/lampstand/pull/7)) |
| T15 | Song search index with typo tolerance | S | SL-3 | `library/src/search.rs` | Done ([#7](https://github.com/incrhst/lampstand/pull/7)) |
| T20 | Font set: bundled fonts, substitutions, `FontLookup` | S | PS-4, LV-4 | `layout/src/fonts.rs`, `assets/fonts/` | Done ([#7](https://github.com/incrhst/lampstand/pull/7)) |
| T28 | DeckCraft evaluation spike | S | PS-1 | `docs/decisions/0001-pptx-reader.md` | Done ([#2](https://github.com/incrhst/lampstand/pull/2), [#7](https://github.com/incrhst/lampstand/pull/7), [#11](https://github.com/incrhst/lampstand/pull/11)) |
| T33 | Live engine state machine | S | LV-1, LV-2, PS-8 | `live/src/lib.rs` | Done ([#7](https://github.com/incrhst/lampstand/pull/7)) |
| T39 | Share-link classification and download | S | PS-1, CD-9 | `connectors/src/links.rs` | Done ([#7](https://github.com/incrhst/lampstand/pull/7)) |
| T58 | CI on Windows, macOS and Linux | S | NFR Platforms | `.github/workflows/` | Done ([#8](https://github.com/incrhst/lampstand/pull/8)) |
| T16 | Sync repo: documents, storage, blob store | **O** | CD-1 | `sync/src/lib.rs`, `sync/src/repo/`, `core` (feature only) | Done ([#8](https://github.com/incrhst/lampstand/pull/8)) |
| T45 | API types to TypeScript | S | — | `api/`, `app/src/lib/api/` | Done ([#7](https://github.com/incrhst/lampstand/pull/7)) |
| T04–T13 | Importers (see below) | S | SL-4, BI-2 | one file each in `import/src/` | Done (T06 skipped; [#7](https://github.com/incrhst/lampstand/pull/7)) |

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
| T29 | PPTX parser: structure, text, shapes, images, notes | **O** | T20, T28 | PS-1, PS-3 | `slides/src/` | Done ([#15](https://github.com/incrhst/lampstand/pull/15)) |
| T14 | Bundled public-domain Bibles (KJV, WEB) | S | T11, T12 | BI-2 | `xtask/`, `assets/bibles/` | Done ([#25](https://github.com/incrhst/lampstand/pull/25); installed on first run) |
| T34 | Countdown timers and announcement loop | S | T33 | LV-2 | `live/src/lib.rs` (`tick`) | Done ([#12](https://github.com/incrhst/lampstand/pull/12)) |

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
| T18 | Teams, device keys, signed membership, invites | **O** | T16 | CD-6 | `sync/src/team.rs` | Partial: device keys, `Role` with its write rules and the `Invite` type are in `team.rs` (the device key is a file, not in the keychain; pairing by code, T17 and T75, stands in). Signed membership, `effective_role`, invites and filtering changes by role are not built |
| T23 | Compositor: audience, stage and stream layouts | S | T21, T22 | LV-3, TS-2–4, TS-9, OB-1, PS-7 | `layout/src/compose.rs` | Done |
| T25 | Output windows, display mapping, transitions, animations | **O** | T24 | LV-3, NFR | `render/src/output/` | Done |
| T26 | Calibration pattern and safe-area overlay | S | T24 | TS-1, TS-8 | `render/src/calibrate.rs` | Done ([#13](https://github.com/incrhst/lampstand/pull/13)) |
| T27 | Video playback (DeckCraft decoders) | S | T24 | LV-4, PS-3 | `media/`, `render/src/output/video.rs` | Done ([#29](https://github.com/incrhst/lampstand/pull/29)) |
| T30 | Animations, builds and transitions from PPTX | S | T29 | PS-3 | `slides/src/anim.rs` | Done ([#22](https://github.com/incrhst/lampstand/pull/22)) |
| T31 | Fidelity report | S | T29 | PS-4 | `slides/src/report.rs` | Partial ([#15](https://github.com/incrhst/lampstand/pull/15), [#22](https://github.com/incrhst/lampstand/pull/22)): the mapping records missing fonts, approximated and unsupported effects and unsupported content in `ParsedDeck::report`, tested on synthetic decks; no `report.rs`, no test for linked media, and nothing shows the report in the app |
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

**T27. Video.** Done, with DeckCraft's pure-Rust decoders (`deckcraft-media`) instead of GStreamer. That choice and its numbers are in ARCHITECTURE.md ("Media"). Covered: looping, seeking (by clock), Play build steps, and outputs that show the same file staying in step. Video nodes upload frames to a texture, and thumbnails show the first frame. `media/tests/play.rs` plays a bundled 2-second clip to the end and counts its frames. Audio followed in 0.2.0 (rehearsal window only).

**T30. Builds and transitions.** Map `p:timing` (click, with and after previous; delays) and `p:transition` to `BuildStep`s and `Transition`s, covering the effect set in `scene::Effect` and approximating the rest. Implement `DeckSlide::scene_at_step`. Done when synthetic decks with each effect produce the expected steps.

**T31. Fidelity report.** Report missing fonts (via `FontLookup`), approximated or unsupported effects, unsupported content (SmartArt without fallback, charts, OLE, 3D) and unreachable linked media. Done when each issue kind has a synthetic deck test.

**T35. Library facade.** Implement the `Library` methods over `Repo`, with a cached decoded `BibleText`, a search index kept current from `subscribe`, and defaults (a starter theme) on first run. Done when tests run against a temporary directory.

## Wave 3: features

| ID | Title | Model | Needs | PRD | Files | Status |
| --- | --- | --- | --- | --- | --- | --- |
| T19 | Relay binary and Docker image | S | T17, T18 | CD-3 | `relay/` | Not started |
| T32 | Fidelity corpus harness (SSIM, private corpus job) | S | T24, T29 | NFR fidelity | `slides/tests/corpus.rs`, CI | Not started |
| T36 | Readiness check and pre-service caching | S | T35, T23 | CD-10, PS-6 | `library/src/readiness.rs` | Not started |
| T68 | ChordPro file importer ([#245](https://github.com/incrhst/lampstand/issues/245)) | A song from a link reads ChordPro charts with it (T125). |
| T37 | Service packages | S | T35 | CD-8 | `library/src/package.rs` | Not started |
| T38 | CCLI usage log and report | S | T35 | RP-1 | `library/src/usage.rs` | Not started |
| T40 | Google sign-in, picker, Drive download and export | S | T39, T42 | PS-2 | `connectors/src/google.rs` | Not started |
| T41 | Linked deck updates: poll, pending, accept | S | T39, T40, T35, T87 | PS-5 | `library/src/decks.rs` | Not started |
| T42 | Local server skeleton | S | T16 | CD-2 | `server/` | Done (with T17, T43) |
| T43 | OBS browser source | S | T42, T23 | OB-1, OB-2 | `server/src/obs/`, `server/web/obs/` | Done ([#14](https://github.com/incrhst/lampstand/pull/14)) |
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
| T47 | Operator view | S | T46 | LV-1, LV-2, TS-8, PS-7 | `app/src/routes/operate/` | Partial ([#21](https://github.com/incrhst/lampstand/pull/21), [#41](https://github.com/incrhst/lampstand/pull/41), [#42](https://github.com/incrhst/lampstand/pull/42), [#47](https://github.com/incrhst/lampstand/pull/47), [#48](https://github.com/incrhst/lampstand/pull/48), [#49](https://github.com/incrhst/lampstand/pull/49), [#60](https://github.com/incrhst/lampstand/pull/60), [#63](https://github.com/incrhst/lampstand/pull/63), [#89](https://github.com/incrhst/lampstand/pull/89), [#93](https://github.com/incrhst/lampstand/pull/93)): Space and the clicker go live, Black, Logo and Clear, preview and live panes with the safe-area guide (TS-8), speaker notes, item jumps; no flags for text below the minimum size (TS-7), and no Playwright tests of the screen |
| T48 | Library and song editor | S | T46, T15 | SL-1, SL-3 | `app/src/routes/library/` | Partial ([#31](https://github.com/incrhst/lampstand/pull/31), [#57](https://github.com/incrhst/lampstand/pull/57), [#80](https://github.com/incrhst/lampstand/pull/80), [#81](https://github.com/incrhst/lampstand/pull/81)): the library panel (one search, drag into the order, file drops) and the song editor are in Plan, not a Library screen; no Playwright tests |
| T49 | Service planner, live collaborative | S | T46, T02 | PL-1, PS-8 | `app/src/routes/plan/` | Partial ([#19](https://github.com/incrhst/lampstand/pull/19), [#20](https://github.com/incrhst/lampstand/pull/20), [#31](https://github.com/incrhst/lampstand/pull/31), [#79](https://github.com/incrhst/lampstand/pull/79), [#85](https://github.com/incrhst/lampstand/pull/85), [#174](https://github.com/incrhst/lampstand/pull/174)): the service order with songs, scripture, decks, media, sound, text slides, headers and loops; no Playwright tests |
| T50 | Outputs setup and calibration wizard | S | T46, T26 | LV-3, TS-1, TS-2 | `app/src/routes/outputs/` | Partial ([#21](https://github.com/incrhst/lampstand/pull/21), [#44](https://github.com/incrhst/lampstand/pull/44), [#46](https://github.com/incrhst/lampstand/pull/46), [#54](https://github.com/incrhst/lampstand/pull/54), [#55](https://github.com/incrhst/lampstand/pull/55), [#56](https://github.com/incrhst/lampstand/pull/56)): the Screens page with display matching and a safe-area calibration view per screen; no Playwright tests |
| T51 | Theme editor | S | T46, T23 | LV-4 | `app/src/routes/themes/` | Partial ([#99](https://github.com/incrhst/lampstand/pull/99)): themes, church look, idle screen and brand files in Setup; no Playwright tests |
| T52 | Scripture panel | S | T46, T01, T21 | BI-1 | `app/src/routes/operate/scripture/` | Partial (planner scripture tab, [#19](https://github.com/incrhst/lampstand/pull/19); a Bible version typed after the reference, [#94](https://github.com/incrhst/lampstand/pull/94); no chapter bounds or suggestions, T71) |
| T53 | Deck import, fidelity report, updates | S | T46, T31, T41, T87 | PS-1, PS-4, PS-5 | `app/src/routes/decks/` | Partial (add decks from file or link, #20; no fidelity report or updates) |
| T54 | Team, invites with QR, sync status | S | T46, T18 | CD-6 | `app/src/routes/team/` | Partial ([#21](https://github.com/incrhst/lampstand/pull/21), [#107](https://github.com/incrhst/lampstand/pull/107), [#152](https://github.com/incrhst/lampstand/pull/152)): pairing by code, the sync status and the roster in Setup › Devices; no team, invites or QR code (T18) |
| T55 | Migration wizard | S | T46, T04–T13 | SL-4, BI-2 | `import/src/detect.rs`, `app/src/routes/welcome/` | Not started |
| T56 | Clicker mode and global shortcuts | S | T46 | PS-9 | `app/src-tauri/src/keys.rs` | Done ([#30](https://github.com/incrhst/lampstand/pull/30)) |
| T57 | Crash recovery journal | S | T46 | NFR Reliability | `app/src-tauri/src/journal.rs` | Done ([#27](https://github.com/incrhst/lampstand/pull/27)) |
| T59 | Early-adopter packaging: macOS, Linux, Windows preview, updater | S | T46 | CD-11 | `app/src-tauri/tauri.conf.json`, CI | Done ([#24](https://github.com/incrhst/lampstand/pull/24), [#61](https://github.com/incrhst/lampstand/pull/61), [#64](https://github.com/incrhst/lampstand/pull/64), [#66](https://github.com/incrhst/lampstand/pull/66), [#72](https://github.com/incrhst/lampstand/pull/72), [#74](https://github.com/incrhst/lampstand/pull/74)): a notarised DMG, AppImage, .deb and an unsigned NSIS installer from `release.yml` (docs/RELEASING.md), and the updater, which installs only at a quiet moment |
| T60 | Accessibility pass (WCAG 2.2 AA) | S | T47–T54 | NFR | `app/src/` | Not started |
| T61 | Performance and latency on a 2019 laptop | **O** | T47 | NFR | `docs/perf.md`, fixes anywhere | Not started |
| T95 | Launch packaging: Windows signing and Flathub | S | T59 | CD-11 | `app/src-tauri/tauri.conf.json`, CI, Flathub manifest | Partial ([#86](https://github.com/incrhst/lampstand/pull/86)): the SignPath application, privacy page and code signing policy are on the site; the Windows installer is not signed and there is no Flathub manifest |

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

**T95. Launch packaging.** For Phase 1: sign the Windows installer (MSI or NSIS) so SmartScreen accepts it, and publish a Flatpak on Flathub. Done when CI signs the Windows installer on tags, it installs on a clean Windows 10 machine without a SmartScreen block, and the Flathub manifest builds and is accepted.

**T60. Accessibility.** Run axe on each screen, check contrast, add focus rings, labels and shortcuts for every live action. Done when there are zero axe violations at AA and a keyboard-only mock service runs end to end.

**T61. Performance (Opus).** On 2019-class hardware (8 GB RAM, integrated graphics), a three-output service must hold 60 fps with a video background, and click-to-photon must stay under 50 ms. Profile, fix, and record the method and the numbers in `docs/perf.md`.

## Follow-ups

Tickets found while reviewing the first wave and the pull requests it superseded. They have no wave: each says what it needs.

Code from a closed pull request is still on GitHub. Fetch every pull request head once with `git fetch origin 'refs/pull/*/head:refs/remotes/pr/*'`, then read a file with `git show pr/N:path`. Port the behaviour and the tests, not the code as is: those pull requests used an older model (one library document, positional service items) that ARCHITECTURE.md replaced.

### Ported from closed PRs

| ID | Title | Model | Needs | Source | Files | Status |
| --- | --- | --- | --- | --- | --- | --- |
| T62 | Full TS-1 calibration design | S | T26 | #5 | `render/src/calibrate.rs` | Done ([#13](https://github.com/incrhst/lampstand/pull/13)) |
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
| T75 | Encrypted, authenticated sync sessions | **O** | — | CD-2, CD-3 | `sync/src/net/` | Done ([#16](https://github.com/incrhst/lampstand/pull/16)) |
| T76 | Blob transfers: size cap and per-session temp files | S | — | CD-2 | `sync/src/net/session.rs` | Not started |
| T77 | Bound documents created by unknown sync messages | **O** | — | CD-1 | `sync/src/repo/mod.rs` | Not started |
| T78 | Don't advance sync state before changes are durable | **O** | — | CD-1 | `sync/src/repo/mod.rs` | Not started |
| T79 | `device.key` permissions and one atomic-write helper | S | — | NFR Security | `app/src-tauri/src/config.rs`, `sync/src/repo/fsutil.rs` | Not started |
| T80 | Size caps on zip entries, deck downloads and Bible decompression | S | — | NFR Reliability | `import/src/songs/opensong.rs`, `connectors/src/links.rs`, `library/src/lib.rs` | Not started |
| T81 | Share links: `percent_decode` and filename sanitising | S | — | PS-1 | `connectors/src/links.rs` | Not started |
| T82 | Render thread: decode off-thread, one shaping helper | **O** | — | LV-3, NFR | `render/src/`, `layout/src/` | Not started |
| T83 | Render tests fail without a GPU when CI expects one | S | — | NFR | `render/tests/`, `.github/workflows/` | Not started |
| T84 | Contract decision: deck builds on recompose | **O** | — | PS-3 | `layout/src/compose.rs`, `slides/`, ARCHITECTURE.md | Done (with T30, [#22](https://github.com/incrhst/lampstand/pull/22)) |
| T85 | Mark and track the remaining render stubs | S | — | LV-4 | `render/src/frame.rs`, `layout/src/compose.rs` | Partial ([#99](https://github.com/incrhst/lampstand/pull/99), [#105](https://github.com/incrhst/lampstand/pull/105), [#106](https://github.com/incrhst/lampstand/pull/106)): the Logo overlay and logo bug use the church logo from the brand; the ring is still the unmarked placeholder when there is none, and Present does not say so |
| T86 | Wire the OBS browser source into the app | S | T43, T46 | OB-1, OB-2 | `app/src-tauri/src/`, Outputs view | Done ([#26](https://github.com/incrhst/lampstand/pull/26)) |
| T87 | Deck budget: price media, one ~1 GB peak | **O** | T29 | NFR Reliability, NFR Hardware | `slides/src/pptx/budget.rs`, `slides/tests/memory.rs`, ARCHITECTURE.md | Done ([#17](https://github.com/incrhst/lampstand/pull/17), [#20](https://github.com/incrhst/lampstand/pull/20)) |

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

**T85. Remaining stubs.** One stub is not marked with a ticket (the other, video nodes drawing nothing, was T27's and is done): the logo overlay is a placeholder ring (`layout/src/compose.rs`) until the contract has a logo asset. Mark it with a `todo`-style comment naming its ticket, add a logo asset to the theme or output contract, and make the operator view say when an overlay is a placeholder.

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

| ID | Title | Model | Needs | PRD | Files | Status |
| --- | --- | --- | --- | --- | --- | --- |
| T88 | OBS WebSocket v5 client | S | — | OB-4 | `connectors/src/obs.rs` | Done ([#28](https://github.com/incrhst/lampstand/pull/28)) |
| T89 | OBS connection, detection and status in Present | S | T88, T46 | OB-5 | `app/src-tauri/src/`, `app/src/routes/` | Done ([#33](https://github.com/incrhst/lampstand/pull/33)) |
| T90 | Service items switch the OBS scene | **O** | T88 | OB-4 | `core/src/service.rs`, `live/`, ARCHITECTURE.md | Done ([#35](https://github.com/incrhst/lampstand/pull/35)) |
| T91 | OBS scene buttons and audio mutes in Present | S | T89 | OB-8 | `app/src/routes/operate/` | Done ([#40](https://github.com/incrhst/lampstand/pull/40)) |
| T92 | Start and stop streaming and recording from the service order | S | T89 | OB-6 | `core/src/service.rs`, `live/`, `app/src/routes/` | Not started |
| T93 | One-click OBS setup | S | T86, T88 | OB-7 | `connectors/src/obs.rs`, Outputs view | Partial ([#141](https://github.com/incrhst/lampstand/pull/141)): "Set up OBS" makes the "Lampstand" collection and starter scenes, and running it again changes only its own sources; rotating the key does not update the URL in OBS (run Set up OBS again) |
| T94 | Stream destination and virtual camera | S | T89 | OB-9 | `app/src-tauri/src/`, `app/src/routes/` | Not started |
| T96 | Default OBS scene per kind of item | S | T90 | OB-10 | `app/src-tauri/src/`, `app/src/routes/` | Not started |

**T88. OBS WebSocket client.** A client for obs-websocket protocol v5 (OBS 28 and later): `Hello`, `Identify` with the SHA-256 challenge, `Request` by id, and event subscriptions (scene, stream, record, virtual camera, input mute, `ExitStarted`). Reconnect with backoff, and expose a status stream (disconnected, connecting, connected, auth failed) plus typed calls for what T89 to T93 need. Behind a `obs` feature like `net`. Done when tests against a mock server cover auth, a wrong password, a dropped connection that reconnects, and an event arriving while a request is in flight.

**T89. Connection and status.** Setup gets an OBS section: host (default `localhost`), port (default 4455), password and a "Test connection" button. The password lives in device-local config with T79's permissions, never in a synced document. Present shows a status pill for connected, streaming and recording, with timecodes and dropped frames, and a clear warning when OBS disconnects. Projection never waits on OBS.

Tell people why OBS isn't connected. A refused connection on `localhost` means OBS isn't running, or its WebSocket server is off (it is off by default in OBS 28 and later, under Tools › WebSocket Server Settings). So check whether OBS is installed (macOS `/Applications/OBS.app`; Windows Program Files or the registry; Linux `obs` on `PATH` or the flatpak `com.obsproject.Studio`) and whether it is running (a process check). Then Present and readiness (T36) say one of:

- "OBS isn't running", with an **Open OBS** button that launches it with `--minimize-to-tray` (and `--collection Lampstand` once T93 has made that collection);
- "OBS isn't installed", with a **Get OBS** link to obsproject.com;
- "OBS is running but its WebSocket server is off", with step-by-step instructions to turn it on and copy the password.

Only say this when the service has a stream output or items with an OBS scene, so churches that don't stream never see it. Don't edit OBS's own config files (to switch the server on, say) without asking the user first. Done when a test drives the status through every state with the mock server, tests cover each detection outcome, and a manual OBS 30 check is noted in the PR.

**T90. Scene per service item (Opus).** Add an optional OBS scene name to a service item (a contract change to `core`; record it in ARCHITECTURE.md). When the item goes live, call `SetCurrentProgramScene`. If OBS isn't connected, or the scene no longer exists, show a warning in Present and carry on. Readiness (T36) flags items whose scene OBS doesn't have, when OBS is connected. Decide whether Previous also switches the scene back, and what happens while the operator holds a manual scene from T91. Done when tests cover go-live, a missing scene, a disconnected OBS and a manual hold.

**T91. Run OBS from Present.** T90 already has `ObsRequest::SetScene` (a hand pick that holds until the next item), `Resume`, and the scene list and program scene in `ObsView`; T91 is their buttons, plus the mutes. One operator usually runs everything ([decision 0005](decisions/0005-manage-obs-not-replace.md), "One operator"), so keep it quiet: one strip in Present with the pill, the scene on air and the next item's scene ("Next: Pastor and slides"). Scene buttons and mutes open from the strip, not always on screen. Show OBS's scenes as buttons, with the program scene marked and following OBS's own events. Clicking a button switches the scene. Show a mute toggle for each audio input that follows `InputMuteStateChanged`. No volume faders: mixing stays in OBS or on the desk. The same panel must work at tablet width, for a second volunteer later. Done when the UI follows changes made in OBS itself as well as in Lampstand, and a service runs end to end with Next alone.

**T92. Stream and record from the service order.** A service item (typically the pre-service countdown) can start streaming and/or recording when it goes live. Stopping is never automatic: Present has a stop button that asks for confirmation. Done when tests cover starting when already streaming, OBS refusing (no stream key), and a disconnected OBS.

**T93. One-click setup.** A "Set up OBS" button creates a scene collection named "Lampstand" (if it doesn't exist), with starter scenes ("Slides", "Worship", "Pastor and slides"). Each gets a browser source named "Lampstand stream" set to the T86 URL at the output's size, with "shutdown source when not visible" off. Running it again updates only the URL and size of sources it created, found by name, and never edits or removes anything else. Rotating the key (T86) updates the URL in OBS when connected. Done when tests against the mock server show a fresh setup, a re-run that changes nothing, and a re-run after a key rotation.

**T94. Stream destination and virtual camera.** T88 has the calls: `stream_destination` and `set_stream_destination` (OBS's listed services such as "YouTube - RTMPS" or "Facebook Live", or a custom RTMP or RTMPS server), and starting, stopping and reading the virtual camera, with its `VirtualCamStateChanged` event. Setup's OBS section gets the destination: a service picker plus stream key, or a custom server and key. Present gets a virtual camera toggle that follows OBS's own events, for churches that send the program into Zoom or Teams. The stream key is a secret: keep it device-local with T79's permissions, never synced, and never logged. OBS refuses to change the destination while it is streaming, so the UI says so instead of failing silently. OBS sends to one destination; streaming to several at once needs a plugin (such as obs-multi-rtmp) or a restream service, and the UI says that too rather than offering a list. Done when tests against the mock server cover reading and setting each kind of destination, a refusal while streaming, and the virtual camera toggling from either side.

**T96. Default scene per kind of item.** Setup › OBS gets one scene per kind of item (song, scripture, deck, media, countdown, announcements), each optional. They must sync, so every planner's device agrees, but no synced settings document exists yet: put them in the team document or add a settings kind, and record the choice in ARCHITECTURE.md. When an item goes live, its own `obs_scene` wins, then its kind's default, then nothing. Plan shows the default faintly in the item's scene field, so overriding it is obvious. `SceneCue` carries the resolved scene, so `obs_control` doesn't change. Done when tests cover the order of precedence, an item clearing its override back to the default, and a kind with no default leaving OBS alone.

### Tablet remote

[Decision 0006](decisions/0006-tablet-remote-in-the-browser.md): an operator remote in the tablet's browser, served over plain HTTP by the local server. A tablet opens it by QR code and pairs with a fresh PIN, and a paired tablet can drive the service and its sound (the decision's amendment) but nothing else. A second Lampstand machine, paired as a sync peer, can drive the service too (T102), through the same gate. T99 to T101 are built as Lampstand Remote (phones and tablets): the phone-first page, `gate.rs` for both callers, Present › More › Connect a phone or tablet, and Remotes in Setup › Devices (ARCHITECTURE.md, "Local server").

| ID | Title | Model | Needs | PRD | Files |
| --- | --- | --- | --- | --- | --- |
| T99 | Remote pairing, sessions and the remote socket | **O** | T42 | LV-5, NFR Security | `server/src/remote/`, `app/src-tauri/src/sync.rs` |
| T100 | Operator remote page | S | T99 | LV-5 | `app/src/remote/`, `app/vite.config.ts` |
| T101 | Connect a tablet: QR code, PIN and paired tablets | S | T99 | LV-5 | `app/src/routes/operate/`, `app/src/routes/devices/` |
| T102 | Run the service from another Lampstand machine | **O** | T99 | LV-5, CD-2 | `sync/src/net/wire.rs`, `sync/src/net/session.rs`, `app/src-tauri/src/sync.rs`, `app/src/routes/operate/` |

**T99. Remote pairing and sessions (Opus).** Add `server::remote`. Starting pairing makes a fresh six-digit PIN that lasts five minutes. Five wrong tries void it, and only one pairing runs at a time, as in device pairing. `POST /remote/pair` with the right PIN sets a random 256-bit session token in an `HttpOnly`, `SameSite=Strict` cookie scoped to `/remote`. Keep only each token's hash, with the tablet's name and when it was paired and last seen, in a device-local `remotes.json` beside `paired.json`, never in a synced document. `WS /remote/ws` needs a valid session. It accepts `Request::Live` and nothing else: any other request is refused and logged. Put that check in one gate on the app side, not in the server's socket handler, so T102's sync peers go through the same one. It sends the current `Event::Live` and `Event::Loaded` on connect and then each change. Revoking a session closes its socket at once. Add `TeamRequest` variants (or a new `RemoteRequest` area) to start and stop pairing, list sessions and revoke one. The PIN goes over plain HTTP, so rate-limit `/remote/pair` per address as well as per PIN. Done when tests cover a right and a wrong PIN, expiry, the fifth wrong try, a refused non-`Live` request, a revoked session's socket closing, and a session surviving a restart. Measure command to `Event::Live` over the socket (PRD target: under 150 ms on local Wi-Fi) and note it in ARCHITECTURE.md.

**T100. Operator remote page.** Add a second Vite entry served at `GET /remote`. It reuses `app/src/lib/kit/` and the generated API types, with a transport that speaks the T99 socket instead of Tauri IPC. Without a session it shows the PIN form. With one it shows the loaded service's order and slides, live and preview marked, and large Next and Previous buttons, plus Black, Logo and Clear. Tapping a slide sends `Show`. It must work in portrait and landscape on a tablet, with touch targets at least 44 pt, and on Safari 16 and Chrome on Android. Slide thumbnails come from the server's WebP frame path where a slide is a deck frame, and are drawn from the scene otherwise. Keep the screen awake with a muted, looping, inline video while the page is open (no Wake Lock: plain HTTP isn't a secure context). Reconnect with backoff when the socket drops, and show that it's offline. Check the bundle in like `web/obs/dist`, so cargo needs no Node. Done when a Playwright test pairs, advances a slide and sees the live marker follow, and when a change made on the laptop shows on the page.

**T101. Connect a tablet.** Present gets a "Connect a tablet" button that starts T99 pairing and shows a QR code for `http://<lan-ip>:<port>/remote`, using the address and port the server actually has, with the PIN in large type beside it and a countdown. Pairing a tablet closes the dialog and names it. Beside the code, a "not loading?" note covers the common causes: the tablet on another network, guest Wi-Fi that isolates devices, and the OS firewall prompt. It recommends a separate Wi-Fi or VLAN for the AV team. Setup → Devices lists paired tablets, with when each was last seen and Revoke. Done when the dialog shows the right address with more than one network interface, and revoking a tablet on the Devices screen disconnects its open page.

**T102. Remote control from a peer (Opus).** Add two frames to `sync::net`: `LiveCommand`, carrying a live `Command` from a controlling peer, and `LiveState`, carrying what `Event::Live` carries, sent to every connected peer on each change and once on connect. Old peers can't read them, so bump `PROTOCOL` to 3 and check that a version-2 peer is refused with a readable reason. The presenting machine passes each `LiveCommand` through T99's gate. Until T18 any paired peer passes it. Leave a `// T18:` hook so only operators and admins do afterwards. In the app, Present gets a "Control <machine>" choice when a paired peer is presenting. While it's on, Present shows that peer's live state, sends its commands there, and draws slides from the synced documents. Its own live engine and outputs stay idle. When the link drops, Present says so and stops sending. The presenting machine carries on, and its operator can take over at once. Two machines must never both drive the same outputs. Done when a two-process test sends Next from one machine and sees the other's live state change and come back, a version-2 peer is refused, a dropped link leaves the presenting machine where it was, and the command-to-state time on a LAN is measured and noted in ARCHITECTURE.md (PRD target: under 150 ms).

### Plans from Notion

[Decision 0007](decisions/0007-plans-from-notion.md) and the [plan contract](plan-contract.md): a church that plans in Notion points Lampstand at its Services database, reviews the matched plan, and creates the service. Read both first. The contract is source-neutral, so T103, T106, T108 and T109 serve any planning tool; only T104, T105, T110 and part of T111 are Notion's. T106 is also the resolver decision 0003 needs.

| ID | Title | Model | Needs | PRD | Files | Status |
| --- | --- | --- | --- | --- | --- | --- |
| T103 | Plan contract types | S | — | PL-4 | `core/src/plan.rs` | Not started |
| T104 | Notion client | S | — | PL-4 | `connectors/src/notion.rs`, `connectors/Cargo.toml` | Not started |
| T105 | Notion to plan mapping | S | T103, T104 | PL-4 | `connectors/src/notion/plan.rs`, `connectors/tests/fixtures/notion/` | Not started |
| T106 | Song and passage resolver | **O** | T103, T15 | PL-4 | `library/src/plan_match.rs` | Not started |
| T107 | Service remembers its source | S | — | PL-4 | `core/src/service.rs` | Not started |
| T108 | Draft and create from a plan | S | T103, T105, T106, T107 | PL-4 | `api/src/`, `app/src-tauri/src/plan.rs` | Not started |
| T109 | Update a service from its plan | **O** | T107, T108 | PL-4 | `library/src/plan_update.rs`, `api/src/`, `app/src-tauri/src/plan.rs` | Not started |
| T110 | Setup › Connections: Notion | S | T104 | PL-4 | `app/src/routes/setup/`, `app/src-tauri/src/notion.rs` | Not started |
| T111 | Plan › From Notion: review and create | S | T108, T110 | PL-4 | `app/src/routes/plan/` | In review (#194) |
| T112 | Real responses and the church's plans | S | T105, T106 | PL-4 | `connectors/tests/fixtures/notion/`, `library/tests/` | Not started |

T103, T104 and T107 can start together. T105 and T106 follow. T108 joins everything; T110 can run beside T105 to T108.

**T103. Plan contract types.** Add `core::plan` with `Plan`, `PlanSource`, `PlanEntry`, `EntryType`, `SongHint` and `PlanError`, with serde (and ts-rs behind the `ts` feature) exactly as [plan-contract.md](plan-contract.md) lays them out. `Plan::from_json` checks the `contract` version (refuse a higher major with a readable reason), rejects an empty title, an entry without `order` or `source_id`, and duplicate `source_id`s, ignores unknown fields, reads an unknown `type` as `other` with a warning, and returns the warnings alongside the plan. `Plan::ordered` sorts by `order`, ties by `source_id`. Done when tests cover the example in the contract round-tripping, each refusal, an unknown field and type, a tie, and a plan with no entries.

**T104. Notion client.** Add `connectors::notion` behind a `notion` feature (reqwest and serde_json). It pins one `Notion-Version`, takes a token and a base URL (so tests can point it at wiremock), and offers: `data_source_schema`, `query` (with a filter and a cursor loop), `page` (one page by ID) and `find_databases` (from a Services database, follow its Items relation to Plan Items and a Plan Items Song relation to Songs). Errors are an enum: `Unauthorized` (bad token), `NotShared` (the integration was not given the page: Notion answers as if it did not exist, so say so), `RateLimited` (retry on 429 honouring `Retry-After`, three tries, then give up), `Network` and `Unexpected` (carries the status, never the token). The token is never logged or put in an error. Before writing it, read Notion's current developer documentation for the data-source split, filter syntax and limits, and correct decision 0007 where it differs. Done when wiremock tests cover paging across three pages, a 429 then success, a 401, a 404 reported as `NotShared`, and a token that never appears in a debug-formatted error.

**T105. Notion to plan mapping.** `connectors::notion::plan::to_plan(services_row, items, songs) -> (Plan, Vec<Warning>)`, pure functions over the JSON the client returns, mapping as decision 0007 lays out. Property names match ignoring case and spaces; extra properties are ignored; a missing required one is `SchemaError { database, missing, found }`. Rich text is joined to plain text; a select, status and relation each have one reader that tolerates an empty value. A Date with a time becomes `starts_at` with its offset or, with a separate Time Zone, in that zone; a date alone is a warning. A Song item with no relation uses its own title. Fixtures under `tests/fixtures/notion/` are hand-written to match the API's shape from the golden example's ten items until T112 replaces them with captured responses. Done when tests cover the golden example end to end into the contract's JSON, each missing property, extra columns, an empty Order, a Song item with no relation, a Date with and without a time, and a page with rich-text formatting in a title.

**T106. Song and passage resolver (Opus).** `library::plan_match::resolve(plan, &Library, context) -> ResolvedPlan`, where `context` carries the installed translations, the default translation and the remembered choices. One result per entry: `Matched { how }`, `Likely`, `Choose(candidates)`, `NotFound`, `Placeholder(reason)`, following the contract's matching rules in order (CCLI, normalised title with author as tie-break, then `search_songs` above a threshold with a margin over the runner-up). Normalising ignores case, punctuation, accents and a leading "the". CCLI lookup scans `Library::songs()`. Passages use `parse_reference` and map an ambiguous book to `Choose`. Arrangement tokens match section labels ignoring case; unknown tokens keep the song's own arrangement and warn. Key is written into the item notes as decision 0007 says. Remembered choices are a plain map from a source song ID to a `SongId`, supplied by the caller and ignored if the song no longer exists. Pick the threshold and margin from data, not by feel: build a fixture library from the bundled hymns plus a few dozen typical titles, test known near-misses ("10000 reasons" against "10,000 Reasons (Bless the Lord)", "Amazing Grace" against "Amazing Grace (My Chains Are Gone)"), and record the numbers in ARCHITECTURE.md; T112 re-checks them on the church's library. This is the resolver decision 0003 needs, so keep it free of anything specific to Notion. Done when tests cover each rule winning in turn, two songs with one CCLI number, a title tie broken by author, a likely match never counting as matched, an ambiguous book, a remembered choice for a deleted song, and the key note with and without a key difference.

**T107. Service remembers its source.** Add `Service::source: Option<PlanSource>` and `ServiceItem::source_id: Option<String>` and `import_hash: Option<String>`, with `autosurgeon(missing = "Option::default")` and `#[serde(default)]` as `obs_scene` has, so older documents read with `None`. Add `ServiceItem::content_hash()`, a stable hash of the fields an import writes (kind and notes), so T109 can tell an edited item from an untouched one. Reflect the change in ARCHITECTURE.md. Done when a document saved before the change loads, and a test shows an edit to notes changes the hash while moving the item does not.

**T108. Draft and create from a plan.** Add `PlanRequest::{NotionServices, DraftFromNotion { service }, CreateFromPlan { draft, choices }}` and their `Response`s to `lampstand-api`, with ts-rs types, and the handlers in `app/src-tauri/src/plan.rs`. `DraftFromNotion` reads through T104 and T105 and resolves with T106 using the device's remembered choices; it does not write. `CreateFromPlan` takes the person's final choice for each entry and writes the service with one `Repo::put`, setting each item's `source_id` and `import_hash` and the service's `source`, then remembers the choices. A placeholder is a header item with its reason in the notes. Creating a blank song from a catalog row writes that song first. Nothing in a plan can reach the live engine. Done when tests cover the golden example from fixtures through draft and create to a `Service` whose items match, a Draft that fails halfway writing nothing, creating a blank song, and the remembered choice used on the next draft.

**T109. Update a service from its plan (Opus).** `PlanRequest::{DraftUpdate { service }, UpdateFromPlan { draft, choices }}`. Match items by `source_id`, replace an item only when its `content_hash` still equals its `import_hash`, keep a locally edited one and mark it, leave hand-added items after the item they followed, insert new entries at the position `Order` gives (a `Position::between` the neighbours), and remove entries gone from the plan only when unedited here. The draft is a list of changes (added, changed, removed, kept, conflict) the review shows before anything is written. The update is one `Repo::put`. A service that is live or loaded into the live engine is refused with a readable reason. Done when tests cover an untouched item updating, an edited item surviving, a new entry landing in the right place beside hand-added items, a removed entry kept because it was edited, two updates in a row changing nothing the second time, and the refusal while live.

**T110. Setup › Connections: Notion.** A Connections section in Setup with a Notion card: a token field (write-only after saving) and the Services database link. The token goes in the OS keychain beside the Google tokens and never in a synced document or a log. "Test connection" calls `find_databases` and reports the three databases it found, the property names it could not find, and the number of services, or `NotShared` as "Share the Services page with your integration in Notion" with the steps. Disconnect removes the token. Done when a test shows the token never appears in config files, logs or API responses, and the card shows each error in plain words.

**T111. Plan › From Notion.** New service gets a "From Notion" choice that lists the Services rows, newest first, with Draft ones marked, and opens the review: one row per entry, in order, with its result and its candidates. The person can choose another song, search the library, create a blank song from the catalog row, or leave the placeholder, and Create stays disabled until no row is unconfirmed (a "likely" match counts as unconfirmed). Update uses the same dialog with the change list from T109. Warnings sit beside the entry they belong to. Done when a Playwright test against a mocked API reviews the golden example, changes one song, creates, and sees the service in Plan, and when a keyboard-only run works.

**T112. Real responses and the church's plans.** Not code to write alone: needs a Notion integration token for the golden example and, with the church's permission, a copy of two or three of its real plans. Capture the API's real responses for the golden example and replace T105's hand-written fixtures with them. Build the church's plans in the convention (or record exactly where it will not fit and update decision 0007), import the church's OpenSong library, and run T106's matcher over the real plans. Record how many songs matched by CCLI, by title, as likely and not at all, and set the threshold from that. Done when the numbers are in ARCHITECTURE.md and decision 0007's open questions 1 and 5 are answered.

### Sound from a link

A worship leader often keeps the week's backing tracks in a shared Google Drive or Dropbox folder. Pasting a share link should add the track to the service without downloading it to the projection laptop by hand first, the way a deck link already works (`LibraryRequest::AddDeckFromLink`).

Lampstand plays files the church is licensed to play: a track it bought, a multitrack or backing-track service's file, a royalty-free library, its own recording. Streaming pages are not files and their terms forbid taking the audio out of them (YouTube also forbids skipping its ads), so a YouTube, Spotify, Apple Music or SoundCloud link is refused with a reason before anything is fetched. A link on a song for the band to hear the arrangement is a separate idea, not this ticket.

| ID | Title | Model | Needs | PRD | Files | Status |
| --- | --- | --- | --- | --- | --- | --- |
| T113 | Add sound from a link | S | — | — | `connectors/src/links.rs`, `api/src/lib.rs`, `app/src-tauri/src/decks.rs`, `app/src-tauri/src/shell.rs`, `app/src/routes/plan/` | In review (#194) |

**T113. Add sound from a link.** Add `LibraryRequest::AddSoundFromLink { url }`, answered with `Response::SoundFile` as `AddSoundFile` is, so the planner adds it to the service as a music-only item (`soundItem` in `lib/sound.ts`).

- **Links it takes.** Google Drive file links and OneDrive links, which `links::classify` already knows. Dropbox share links, fetched as their direct download (`dl=1`). A plain `https://` link to a file whose name ends in a sound extension (`audioExtensions`). Refuse YouTube (`youtube.com`, `youtu.be`, `music.youtube.com`), Spotify, Apple Music and SoundCloud links before any request, with "Lampstand plays sound files your church is licensed to use. Streaming links can't be added: download the track from where you bought it, or share the file from Drive or Dropbox." Anything else says which links work.
- **The download.** Run it in the shell like `AddDeckFromLink` (async, with `ImportProgress` events: "Downloading the track", then "Added …" or the reason). Stream to a temporary file in the downloads folder rather than holding it in memory. Stop at 250 MB with a message saying so. Reuse `links`' status handling: private, not found, an HTML page instead of a file ("the link leads to a web page, not a sound file").
- **Checking it is sound.** Check with `lampstand_media::sound_length` on the downloaded file, as `add_sound_file` does, before anything goes in the library. Then add it through the same path as `AddSoundFile`, so the name and length come out the same. Take the name from `Content-Disposition`, else the link's last path part, without its extension.
- **The UI.** In Plan, the Add sound picker and Add media get a "From a link" choice: a paste field, then the track added to the service at the end, waiting for Play. Dropping or pasting a link on the library does the same when a service is open. Follow `addDeckLink` in `PlanView.svelte` and the deck-link paste in `Library.svelte`.
- **Not in this ticket.** Re-fetching when the shared file changes (T41 does that for decks). Folders. Anything that plays from the network during a service: the file is always copied into the library first, so the service plays offline.

Done when tests (wiremock, as `links` has) cover a Drive, Dropbox, OneDrive and direct link that add a track with its name and length; a private link and a missing one; an HTML page; a file that isn't sound; the size limit; and a YouTube, Spotify and SoundCloud link refused without a request. Done, too, when a track added from a Drive link plays in Present's Sound box.

## Not code: start now

These sit on the critical path, but no session can do them:

- Apply for CCLI SongSelect API access and Bible publisher licences (PRD risk table).
- Start Google OAuth app verification for the `drive.file` scope and the Picker.
- Assemble the 200-deck fidelity corpus with permission from the churches who supply them, plus PowerPoint PNG exports of each.
- Set up an Apple Developer ID now: the early-adopter release (T59) needs it.
- Get a Windows code-signing certificate and a Flathub account before launch (T95).
- Decide the open PRD questions: the public relay (CD-4) and the product name.

## 1.0.0

1.0.0 is the release a church can rely on every Sunday with no developer nearby. It is a church-ready cut of Phase 1, plus recording the sermon and adding a song from a link. It is not every P0 in the PRD: the parts that need a server or an outside approval move to 1.x (below).

**How it ships.** The GitHub milestone "1.0.0" holds everything 1.0.0 needs. We get there through 0.9.x releases: each new 0.9.x takes a few tickets out of the 1.0.0 milestone into its own milestone, and ships them. 1.0.0 is tagged when the 1.0.0 milestone is empty and the release checks below pass. "Send sound to the stream" (#125, #137, milestone 0.10.0) is part of 1.0.0 too.

**The order.** Safety first, because every later release carries it. Then what a church needs on its first Sunday: readiness, CCLI reporting, the migration wizard and service packages. Recording comes alongside: T117 and T119 can start at once. A song from a link (T125) follows Paste lyrics (T114) and the ChordPro importer (T68). Windows signing, accessibility and performance come last, because they check the finished screens.

### In 1.0.0, already ticketed

These tickets are described above. Each gets a 1.0.0 issue.

| ID | Title | Why it is in 1.0.0 |
| --- | --- | --- |
| T65, T76, T77, T78, T79, T80, T81 | Sync, file and download safety ([#217](https://github.com/incrhst/lampstand/issues/217), [#218](https://github.com/incrhst/lampstand/issues/218), [#219](https://github.com/incrhst/lampstand/issues/219), [#220](https://github.com/incrhst/lampstand/issues/220), [#221](https://github.com/incrhst/lampstand/issues/221), [#222](https://github.com/incrhst/lampstand/issues/222), [#223](https://github.com/incrhst/lampstand/issues/223)) | A Sunday must not lose a document, fill a disk or trust a bad file. |
| T36 | Readiness check and pre-service caching ([#224](https://github.com/incrhst/lampstand/issues/224)) | One green or red answer before the service (CD-10). It also checks recording (T122). |
| T38 | CCLI usage log and report ([#225](https://github.com/incrhst/lampstand/issues/225)) | Churches have to report what they show (RP-1), more so now that songs come in by paste. |
| T55 | Migration wizard ([#226](https://github.com/incrhst/lampstand/issues/226)) | Moving from another tool in under an hour (goal 4). |
| T37 | Service packages ([#227](https://github.com/incrhst/lampstand/issues/227)) | A laptop that can't join the team still gets the service (CD-8). |
| T95 | Windows signing and Flathub ([#228](https://github.com/incrhst/lampstand/issues/228)) | Most projection laptops run Windows; SmartScreen warnings stop at 1.0.0. |
| T60 | Accessibility pass ([#229](https://github.com/incrhst/lampstand/issues/229)) | WCAG 2.2 AA (NFR), checked on the finished screens. |
| T61 | Performance on a 2019 laptop ([#230](https://github.com/incrhst/lampstand/issues/230)) | The NFR budget, measured with OBS and recording running beside it. |

### After 1.0.0 (1.x)

Not rejected, but 1.0.0 does not wait for them: the sync relay and guest upload links (T19, T44, T64), Google Slides and linked deck updates (T40, T41, the rest of T53), the fidelity report and the deck corpus (the rest of T31, T32), Teams permissions (the rest of T18, T54), and the P1 and P2 tickets (T63, T66, T67, T69 to T74, T82, T83, T85, T92, T94, T96). Plans from Notion (T103 to T112), Paste lyrics and the library MCP server (T114 to T116) keep their own order and may ship in any 0.9.x; T125 needs T114 first.

### Recording the sermon

[Decision 0012](decisions/0012-recording-the-sermon.md): Lampstand records the sermon's audio from one input on the computer that runs the stream, and starts and stops OBS's own video recording if the church wants that. An item marked "Record this" starts and stops it, and so do buttons in Present. Recordings stay on that computer, where they are trimmed and exported as MP3. Read the decision first: the details marked **proposed** there need the owner's approval in the pull request that builds them.

| ID | Title | Model | Needs | PRD | Files | Status |
| --- | --- | --- | --- | --- | --- | --- |
| T117 | The recorder: inputs, a WAV that survives a crash, the level | **O** | — | RC-1 | `media/src/record.rs`, `media/src/lib.rs`, `media/Cargo.toml` | Not started ([#209](https://github.com/incrhst/lampstand/issues/209)) |
| T118 | Trim and export: MP3, WAV, evened-out level, tags | S | T117 | RC-4 | `media/src/export.rs`, `media/Cargo.toml`, `Cargo.toml`, `deny.toml` | Not started ([#210](https://github.com/incrhst/lampstand/issues/210)) |
| T119 | "Record this" on an item, and when recording starts and stops | **O** | — | RC-2 | `core/src/service.rs`, `live/src/`, `api/src/lib.rs`, ARCHITECTURE.md | Not started ([#211](https://github.com/incrhst/lampstand/issues/211)) |
| T120 | Recording cues to the Stream holder (protocol 10) | **O** | T119 | RC-1, RC-2 | `sync/src/net/record.rs`, `sync/src/net/wire.rs`, `sync/src/roles.rs`, `sync/src/wording.rs`, `app/src-tauri/src/record_cues.rs` | Not started ([#212](https://github.com/incrhst/lampstand/issues/212)) |
| T121 | The recording deck in the app, and OBS video | S | T117, T120 | RC-1, RC-3 | `app/src-tauri/src/recorder.rs`, `app/src-tauri/src/handler/record.rs`, `app/src-tauri/src/obs_control.rs`, `app/src-tauri/Info.plist`, `app/src-tauri/tauri.conf.json` | Not started ([#213](https://github.com/incrhst/lampstand/issues/213)) |
| T122 | Setup › Recording, Present's Record button and pill, Plan's "Record this" | S | T121 | RC-1 to RC-3 | `app/src/routes/setup/`, `app/src/routes/operate/`, `app/src/routes/plan/`, `app/src/lib/recording.ts` | Not started ([#214](https://github.com/incrhst/lampstand/issues/214)) |
| T123 | Recordings: waveform, trim, export, Move to Trash | S | T118, T121 | RC-4 | `app/src/routes/plan/recordings/`, `app/src-tauri/src/handler/record.rs` | Not started ([#215](https://github.com/incrhst/lampstand/issues/215)) |
| T124 | Documentation for recording | S | T122, T123 | — | `docs/roles.md`, `docs/ARCHITECTURE.md`, `docs/RELEASING.md`, `site/docs/` | Not started ([#216](https://github.com/incrhst/lampstand/issues/216)) |

**T117. The recorder (Opus).** Add `lampstand_media::record`. `input_devices()` lists cpal inputs by name with their channel counts, and `default_input()` gives the system's. `Recorder::start(device, channels, path)` opens the input on a thread of its own, as `Player` does for outputs (a cpal stream must stay on the thread that made it; the callback never waits or allocates), and writes 24-bit WAV at the input's own rate: one channel (which one is chosen) or a stereo pair. A writer thread takes the samples through a lock-free ring buffer, writes them, and rewrites the header's lengths every five seconds and on stop, so a file cut short plays up to the last few seconds. `repair(path)` fixes the header of a file whose lengths are stale and says how long it is. The recorder reports a peak level 10 times a second, how long it has run, and whether the input is opening, open, missing or failed (the same states as `OutputState`), with a watchdog like `Player`'s for a stalled input. It refuses to start with less than 2 GB free and reports when less than 10 minutes of space is left. Done when tests cover a recording written from a fake input (a sine wave in, the same samples out), a file cut short and repaired, the header rewritten during a long recording, a full disk (simulated), an input that stops calling back, and the 3-hour limit; and when `play_check` gains `--record` for the real-hardware check.

**T118. Trim and export.** Add `lampstand_media::export`. `peaks(path, buckets)` reads a WAV into a waveform for the screen without loading it all. `export(path, trim, Format, tags, even_out)` writes the part between the trim points as MP3 (mono 96 kbps, stereo 128 kbps, with ID3 title, album, artist and date) or as WAV. "Even out the level" measures loudness (EBU R128, with the `ebur128` crate) and sets the export to -16 LUFS with a true peak under -1 dB. Exporting runs on a thread and reports progress; it never changes the master. Use `mp3lame-encoder` (LAME 3.100), `id3` for the tags and `ebur128` for loudness, with a limiter so the export stays under -1 dBTP ([research](https://github.com/incrhst/lampstand/issues/235)). The crates declare `LGPL-3.0`: allow it in `deny.toml` for `mp3lame-encoder` and `mp3lame-sys` only, with a comment, as `webpki-root-certs` has. On macOS and Linux the build runs LAME's own `configure` and `make` (it needs `sh`, `make` and a C compiler, which the CI runners have); on Windows it uses MSVC through `cc`. Done when tests cover trimming at the exact sample, tags read back, a level evened out within 1 LU, an export of a two-hour file in less than a minute in release mode, and CI green on all three systems.

**T119. "Record this" and the recording rule (Opus).** Add `ServiceItem::record: bool` (a contract change to `core`: write no key when false, as `backing` writes none when absent, and record it in ARCHITECTURE.md), set with `ServiceRequest::SetRecord`. The live engine emits `Event::Record(Start { item } | Stop)` by the rules in decision 0012, point 4: start when a marked item goes live; stop after a 10-second tail when an unmarked item goes live; the same recording carries on if a marked item comes back within the tail; marked items in a row make one recording; going back after the tail starts part 2. The engine knows nothing about hand starts. Those are a flag the app passes in (`hand: bool`), and a hand start is never stopped by the engine. Previous, a loop or a reload of the open service never starts or stops anything by itself, except as above. Done when tests cover each rule, a marked last item (the service ends: stop after the tail), a marked item removed while it records (stop), and a service loaded with a marked item already live (no start until it goes live again).

**T120. Recording cues to the Stream holder (Opus).** Follow `net::cue` (OBS scene cues) closely. Add the frames `RecordCue { run, seq, cue: Start { service, item, title, part } | Stop }` (Projection holder to recorder), `RecordCueAck { run, seq }`, and `RecordNow { state }`, which the recorder sends to every connected device when its state changes, at most twice a second for the level, and when a device connects: idle, recording (since, part, level), or a problem (`NoInput`, `InputMissing`, `InputFailed`, `DiskFull`, `PermissionDenied`, never a path or free text). The recorder is decided by `SyncNode::route_record_cue` by the OBS duty's rule: the Stream holder; the Projection holder if nobody holds Stream; this device if nothing is paired. A cue is believed only from the device the recorder's own record names as Projection holder, and a hand start or stop only from the Projection or Operator holder. The newest cue waits for an absent recorder, as a scene cue does. Passing Stream while recording: `OnAir` gains `Recording` for Stream only while a recording runs, so the prompt says "Allow and stop the recording", and "Always allow this device" does not apply while it runs (`sync/src/roles.rs`). Bump `PROTOCOL` to 10, and check that a version-9 peer is refused with the usual message. Done when two-process tests cover a start and a stop reaching another device, a cue that waits for an offline Stream holder, a cue from a device that isn't the Projection holder ignored, Stream passed while recording (stopped, the new holder starts part 2 while the marked item is live), always-allow skipped while recording, and protocol 9 refused.

**T121. The recording deck in the app.** `app/src-tauri/src/recorder.rs` owns the `Recorder` on the device that records, as `audio.rs` owns the player. It takes cues from T120, and `Request::Record` (`Start`, `Stop`, `Status`, `Inputs`, `List`, `Export`, `Trim`, `Trash`) through `handler/record.rs`. Device-local settings in `settings.json`: the input and channels, the folder (default `Lampstand Recordings` in the system Music folder, from `dirs::audio_dir`), and "Also record video in OBS" (off by default). File names come from the service's title and date and the item's title (decision 0012, point 7), with characters no file system allows replaced, and ` (part 2)` for a second part. `recordings.json` beside `settings.json` lists each recording (service, item, start, length, trim, exports, OBS file), written atomically. On start, repair any file that was recording when the app stopped, and tell the person once (decision 0012, point 8). With "Also record video in OBS" on and OBS connected, start OBS's recording with the sermon and stop it with the sermon, but only if OBS was not already recording. Keep OBS's file path from `RecordStateChanged`. Add the microphone usage text to macOS's `Info.plist`, and ask for permission only from Setup. Done when tests cover a start and stop from a cue, naming and a second part, repair on start, and OBS left alone when it was already recording (mock server). Done, too, when a Mac and a Windows computer have recorded a sermon from a USB interface.

**T122. Setup › Recording and Present.** Setup › Recording (shown on every device; the input chooser is on the device that records, and other devices say "{name} records the sermon"): the input and channels with a live level meter, "Save recordings in" with Change and Show, "Also record video in OBS" (shown when OBS is set up), and the free space. Plan: a "Record this" switch on each item, with a red dot in the order on a marked item. Present: a red "Recording 12:31" pill with a small meter wherever recording matters (a service with a marked item, or a recording running), Record and Stop in the More menu and on the pill, and Stop asks "Stop recording? The sermon is still on." while a marked item is live. The role bar warns ahead of a marked item that cannot be recorded, naming the reason ("The sermon won't be recorded: no input is chosen on Sanctuary."), and the pill says "No sound is coming in to the recording. Check the feed from the mixer." after 30 seconds of near silence. Readiness (T36, if built) lists the same problems. All words are in one place (`lib/recording.ts`) and are proposed until the owner approves them. Done when component tests cover each state of the pill and the warning, and a service runs end to end with recording started by the sermon and stopped by the next item.

**T123. Recordings.** In Plan, on the device that records, a Recordings list, newest first: name, date, length, and whether it has been exported. Other devices say "Recordings are on {name}". Opening one shows the waveform (T118 `peaks`), play from a point, trim handles for the start and end (kept as points; "Reset" clears them), and Export (MP3 or WAV, with "Even out the level" ticked), with progress, then "Show in folder". An OBS video file beside the recording shows its name and "Show in folder" if it is on this computer. Move to Trash uses the system's Trash, never a permanent delete, and asks first. Done when component tests cover the list, trimming and an export's progress, and the keyboard reaches every control (T60's standard).

**T124. Documentation for recording.** In `roles.md`: Stream also records the sermon, the changed sentence about room sound (decision 0012, "What this changes"), and the hand-over prompt while recording. In ARCHITECTURE.md: a "Recording the sermon" section, the protocol 10 frames, and what syncs (nothing about recordings beyond `RecordNow`). In RELEASING.md: a real-hardware recording check (a USB interface on a Mac and on Windows, recording while OBS and Zoom read the same input, a power-cut recording repaired, an export played on a phone). On the website: a Recording page for volunteers, in the house style. Done when every **proposed** detail in decision 0012 is either approved or changed in the decision.

### A song from a link

[Decision 0013](decisions/0013-song-from-a-link.md): paste a link to the church's own shared document or song file, or to a public-domain hymn, and Lampstand reads it as a song for the person to check. Lyric sites and SongSelect links are refused before any request, with the way that works. Read the decision first.

| ID | Title | Model | Needs | PRD | Files | Status |
| --- | --- | --- | --- | --- | --- | --- |
| T125 | Add a song from a link | S | T114, T68 | SL-6, RP-1 | `connectors/src/links.rs`, `core/src/song.rs`, `api/src/lib.rs`, `app/src-tauri/src/handler.rs`, `app/src/routes/plan/PasteSong.svelte` | Not started ([#244](https://github.com/incrhst/lampstand/issues/244)) |

**T125. Add a song from a link.** Add `links::classify_song`, beside `classify_sound`, and `LibraryRequest::SongFromLink { url }`, answered with `Response::ParsedSong` as `ParseLyrics` is (T114), so the person checks the song in the Paste lyrics dialog before saving it with `PutSong`.

- **Links it takes** (decision 0013, point 1). A Google Doc, read through its plain-text export (`/export?format=txt`). Google Drive, Dropbox (`dl=1`) and OneDrive files, which `links` already handles for decks and sound. A plain `https://` link whose name ends in `.txt`, `.cho`, `.chopro`, `.chordpro`, `.xml` (OpenLyrics) or an OpenSong file. Public-domain hymn pages from the list in `links` (**proposed**: hymnary.org text pages, only if its terms allow it; check them and quote the clause in the PR, or leave the list empty).
- **Links it refuses before any request.** Lyric sites (Genius, AZLyrics, Musixmatch, Lyrics.com, LyricFind, SongLyrics, Lyrics Translate and others, kept in one list) with "Lampstand can't take words from lyric sites: their terms don't allow it. Copy the words from your church's own chart, or from SongSelect, and use Paste lyrics." SongSelect (`songselect.ccli.com`) with "Open the song in SongSelect, then copy the words into Paste lyrics, or download the file and add it with Import songs." Any other web page says which links work. Follow the YouTube refusals in `classify_sound`.
- **The download.** In the shell, as `AddSoundFromLink` (T113): async with `ImportProgress`, a 2 MB limit, and `links`' handling of a private, missing or HTML response ("the link leads to a web page, not a song file"). Read the file by its kind: text through `songs::paste`, ChordPro through T68, OpenLyrics and OpenSong through their importers. A file with more than one song takes the first and says how many it left.
- **Where it came from.** Add `Song::source: Option<SongSource>` (`Link { url }`, `Paste`, `Assistant`), written only when set, as a contract change recorded in ARCHITECTURE.md. T115 needs the same field; whichever lands first adds it.
- **CCLI** (decision 0013, point 5). If no CCLI number came with the song, the dialog asks for one, with "Public domain" as the other answer (it sets the copyright line to "Public domain"), and it can be left empty. The library shows "No CCLI number" on a song with neither. The note "Only add words your church has the right to use." stays until T38 is built.
- **The UI.** In Plan, the Paste lyrics dialog gets "From a link": a paste field, then the same check. Pasting or dropping a song link on the library does the same. All new words are proposed until the owner approves them.

Done when tests (wiremock, as `links` has) cover a Google Doc, a Dropbox ChordPro chart, a direct OpenLyrics file and a plain text file that each come back as a checked song; a private link, a missing one and an HTML page; the size limit; a lyric site, SongSelect and an unknown web page refused without a request; a song with no CCLI number; and `source` read back from a saved song. Done, too, when a song added from a shared Google Doc goes live in Present.

### Release checks for 1.0.0 (not code)

Tracked in [#231](https://github.com/incrhst/lampstand/issues/231).

- The real-hardware recording check in RELEASING.md (T124), on a Mac and a Windows computer, with the interface a church would use.
- A full mock service on the 2019-class laptop (T61), with three outputs, OBS streaming and the sermon recording.
- The 0.9.0 checks again (#186 sound, #179 phones and tablets) on the 1.0.0 build.
- Windows signing in place (T95), and the "Not code" items above that 1.0.0 still needs.

## Parallelism at a glance

- **Wave 0:** 11 tickets plus 10 importers, all independent. T16 (Opus) is the longest; start it first.
- **Wave 1:** T24 and T29 (Opus) join T16 on the critical path; start them as soon as T20 and T28 land.
- **Waves 2 and 3:** split cleanly by crate.
- **Wave 4:** fans out once T46 lands.
