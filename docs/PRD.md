# PRD: Church Projection Platform

Oct 18, 2026 · @David Bain

## Summary

We will build a free, open-source church projection platform that replaces OpenSong and matches the core of ProPresenter, EasyWorship and FreeShow. It will run on any laptop, work fully offline on Sunday morning, and let a volunteer with 10 minutes of training run a service.

The current market splits two ways. Free, open-source tools (OpenSong, OpenLP, Quelea, FreeShow) are capable but dated, hard to learn, or thin on collaboration. Commercial tools (ProPresenter, EasyWorship, MediaShout, Proclaim) are polished but cost USD 100 to 500 a year or more, are tied to one operating system, or need an always-on cloud connection.

The replacement keeps what churches rely on (song library, Bible, service order, operator and stage screens, CCLI reporting), and adds what they keep asking for: planning shared across the team, phone control, lyrics and lower thirds sent to OBS for the live stream, per-screen title-safe calibration, clean import from the tool they use today, and first-class PowerPoint and Google Slides support. Slides from a guest speaker or the youth team must play with their animations, video and speaker notes intact, without anyone opening PowerPoint on the projection laptop.

## Market scan

Every platform covers the same core, and none handles PowerPoint and Google Slides well. Most flatten slides to images (losing animation, video and builds) or remote-control a separately installed PowerPoint, which breaks when windows lose focus. None reads Google Slides directly: teams download a PPTX or PDF on Sunday morning.

| Platform | Licence and price (approx.) | OS | Known for | PowerPoint / Google Slides | Main gap |
| --- | --- | --- | --- | --- | --- |
| OpenSong | Free, GPL | Windows, macOS, Linux | Song library with chords, sets, Bible, printable chord sheets | No native support; slides exported as images | Dated UI, little active development, no remote or stage app |
| OpenLP | Free, GPL | Windows, macOS, Linux | Songs, Bibles, custom slides, web and mobile remote, OpenLyrics | Drives an installed PowerPoint or LibreOffice; PDF import | Presentation control is fragile; theming is basic |
| Quelea | Free, GPL | Windows, macOS, Linux | Simple song and Bible display, mobile lyrics | Converts via LibreOffice to images | Small community; limited media and stage features |
| FreeShow | Free, open source | Windows, macOS, Linux | Modern UI, stage display, remote, NDI output | Imports PPTX and PDF, mostly as images | No shared team planning; young plugin ecosystem |
| ProPresenter 7 | Paid, roughly USD 400 a year | Windows, macOS | Multi-screen, props, masks, NDI, timecode, MIDI, Planning Center | Imports PowerPoint and Keynote into editable slides; layout and animation often lost | Price, steep learning curve, heavy hardware needs |
| EasyWorship 7 | Paid, roughly USD 500 plus support | Windows only | Easy schedule building, SongSelect, Bible versions | PowerPoint imported as static images | Windows only; no shared planning |
| MediaShout 7 | Paid | Windows only | Cue-based scripting, Bible tools | Controls PowerPoint | Windows only; small user base |
| Proclaim | Subscription, roughly USD 30 a month | Windows, macOS | Cloud planning shared across a team, Logos Bible integration | PowerPoint imported as images | Needs internet to sync; subscription cost |

Shared baseline across all of them:

- Song library with verse, chorus and bridge sections, arrangement order, CCLI number and copyright line
- Bible lookup in several translations, with verse splitting across slides
- A service order (set, schedule or playlist) built ahead and run live
- Operator view with preview and live panes, plus a separate output screen
- Themes for fonts, backgrounds and text position, with video and image backgrounds
- Blank, black and logo screens, announcements loop, countdown timers
- Stage display for musicians: current and next slide, clock, notes

Compiled from product knowledge as of mid-2026. Prices are approximate and were not checked against vendor pages.

## Users and jobs to be done

The operator on Sunday is usually a volunteer who runs the system twice a month. Design for them first.

| Role | Typical person | Jobs to be done |
| --- | --- | --- |
| Operator | Volunteer, 15 to 70 years old, trained in under an hour | Follow the service, advance lyrics in time with the band, recover from mistakes without the congregation noticing |
| Worship leader | Plans songs each week | Pick songs and keys, set arrangement order, see the next lyric on a stage screen |
| Pastor or speaker | Builds sermon slides in PowerPoint or Google Slides | Hand over a deck late on Saturday and trust it to look the same on Sunday, with notes on a confidence monitor |
| Church admin | Runs the office, keeps licences | Manage the song library, file CCLI usage reports, keep announcements current |
| Media or stream lead | Runs the live stream | Send lower thirds and lyrics to the stream with a transparent background |
| Tech director | Larger churches, several screens and rooms | Map outputs to projectors, LED walls and stage screens, trigger lights and audio cues |

## Goals, non-goals and success metrics

Goals:

1. A trained volunteer can run a full service (songs, scripture, sermon deck, announcements) after 10 minutes of training.
2. PowerPoint and Google Slides decks play as faithfully as in their own apps, with no extra software installed.
3. A team plans the service together during the week and the projection laptop has it offline on Sunday.
4. Churches move from OpenSong, OpenLP, EasyWorship, ProPresenter or FreeShow in under an hour, songs and sets included.
5. Lyrics, scripture and lower thirds reach the live stream through OBS with no extra hardware.

Non-goals:

- **Replacing OBS.** We integrate with it. OBS is free, already used by most streaming churches, and owns encoding, audio mixing, camera switching and multi-platform streaming. Rebuilding that would take years and give churches nothing they lack. We send OBS clean graphics and cues; OBS stays the streaming engine.
- Editing PowerPoint or Google Slides content. We play decks and offer light changes (hide a slide, reorder); authoring stays in the source app.
- Video editing, audio mixing, or lighting desk replacement. We trigger external systems, we do not replace them.
- Church management (giving, membership, check-in). We integrate with Planning Center and similar tools.

| Metric | Target at 12 months |
| --- | --- |
| Time for a new operator to run a mock service unaided | Under 10 minutes of training |
| Imported decks that need no manual fix | 95% |
| Services with a visible projection error (wrong slide, desktop showing, freeze) | Under 1 in 200 |
| Churches migrated from another tool in under 1 hour | 80% |
| Streaming churches using the OBS integration | 60% |
| Weekly active churches | 2,000 |

## Functional requirements

P0 is needed for launch, P1 for the first year, P2 later. PowerPoint, Google Slides and OBS sit near the top because they set us apart from other tools.

### PowerPoint and Google Slides

| ID | Requirement | Priority |
| --- | --- | --- |
| PS-1 | Import PPTX by drag and drop, file picker or OneDrive link. Render it with our own engine: no PowerPoint install needed. | P0 |
| PS-2 | Connect a Google account and pick any file from Drive with the Drive picker: Google Slides, PowerPoint (PPT and PPTX), Keynote, PDF, images, video or audio. Google Slides come in through the Slides API export; every other file is downloaded as is and imported like a local file. The per-file drive.file scope covers all of these, so there is no restricted-scope security review. | P0 |
| PS-3 | Keep text, shapes, images, slide masters, embedded video and audio, slide transitions, and the common entrance, exit and emphasis animations. Builds advance one click at a time, as in PowerPoint. | P0 |
| PS-4 | On import, show a fidelity report: missing fonts (with the substitute used), unsupported effects, linked media that could not be fetched. One click shows each affected slide. | P0 |
| PS-5 | Linked decks stay in sync with the source. Changes made on Saturday night show as "updated version available"; the operator accepts them. A deck never changes while it is live. | P0 |
| PS-6 | Before each service, cache every linked deck locally so it plays with no internet. | P0 |
| PS-7 | Speaker notes go to the confidence monitor and stage display, with current and next slide thumbnails and a timer. | P0 |
| PS-8 | Deck slides sit in the service order next to songs and scripture, and use the same go-live, blank and logo controls. | P0 |
| PS-9 | Presenter clickers (page up and down, arrow keys, B for black) work whichever window has focus. | P0 |
| PS-10 | Hide, skip or reorder slides for this service without editing the source deck. | P1 |
| PS-11 | Slide text becomes searchable, and can be sent to the stream as a lower third (for example a sermon point). | P1 |
| PS-12 | Apply the church theme to a deck: swap fonts and backgrounds for consistency with the rest of the service. | P2 |
| PS-13 | Fallback mode: drive an installed PowerPoint or Keynote for decks the fidelity report flags as broken. | P2 |
| PS-14 | Import Keynote and PDF. Export a service to PPTX or PDF for sharing. | P1 |

### OBS and live stream

| ID | Requirement | Priority |
| --- | --- | --- |
| OB-1 | A stream output with its own layout (lyrics as a lower third, scripture bar, sermon slides) separate from the room projection. | P0 |
| OB-2 | Deliver that output to OBS as a browser source URL on the local network, with a transparent background. No plugin needed. | P0 |
| OB-3 | Also offer NDI output with alpha, and Spout (Windows) and Syphon (macOS), for churches using vMix, ATEM or other switchers. | P1 |
| OB-4 | Connect to OBS through its built-in WebSocket (OBS 28 and later). A service item can switch the OBS scene when it goes live, for example the sermon item moves OBS to the "Pastor and slides" scene. | P1 |
| OB-5 | Show OBS streaming and recording status, and a warning if OBS disconnects, in the operator view. | P1 |
| OB-6 | Start and stop OBS streaming and recording from the service order (pre-service countdown starts the stream). | P2 |

### Songs and library

| ID | Requirement | Priority |
| --- | --- | --- |
| SL-1 | Songs with sections (verse, chorus, bridge, tag), arrangement order, key, tempo, CCLI number, author and copyright. | P0 |
| SL-2 | Chords stored inline (ChordPro). Printable lead and chord sheets with transposition. | P1 |
| SL-3 | Full-text search across titles and lyrics, with typo tolerance. | P0 |
| SL-4 | Import from SongSelect (CCLI), OpenLyrics, OpenSong, OpenLP, EasyWorship, ProPresenter and FreeShow libraries. | P0 |
| SL-5 | Several languages per song, shown side by side or alternating. | P1 |

### Scripture

| ID | Requirement | Priority |
| --- | --- | --- |
| BI-1 | Type a reference ("Jn 3:16-18") and go live. Split long passages across slides automatically. | P0 |
| BI-2 | Import translations in OSIS, USFM, Zefania XML, OpenSong and OpenLP formats. The church holds the licence for what it imports; we bundle public domain texts only (for example KJV, WEB). Imported texts sync between the church's own devices but are left out of service packages. | P0 |
| BI-3 | Show two translations side by side. | P1 |

### Planning and collaboration

| ID | Requirement | Priority |
| --- | --- | --- |
| PL-1 | Build a service order in the browser or desktop app; the whole team sees changes live. | P0 |
| PL-2 | Templates for recurring services. Comments and assignments per item. | P1 |
| PL-3 | Two-way sync with Planning Center Services. | P1 |

### Collaboration and delivery

Free and open source means no company cloud that every church depends on. Collaboration is local-first: every device holds a full copy, and changes merge automatically over whatever link is available, with no server needed at all. Getting a service, a deck or the app itself onto the projection laptop must take one step.

| ID | Requirement | Priority |
| --- | --- | --- |
| CD-1 | Local-first data: every install holds the full library and services. Edits made offline on several devices merge without conflicts (CRDT, Automerge in Rust). | P0 |
| CD-2 | Peer-to-peer sync on the church network with no server: the office PC and projection laptop find each other and sync. | P0 |
| CD-3 | Self-hostable sync relay for sync between homes and church: one Rust binary or Docker image, small enough for a Raspberry Pi or USD 5 VPS. | P0 |
| CD-4 | Optional free public relay run by the project, so small churches need no server. | P1 |
| CD-5 | Browser planner served by the relay: build the order, pick songs and upload decks from a phone, no install. Same web UI as the desktop app. | P1 |
| CD-6 | Invite team members by link or QR code, with a role (admin, planner, operator, viewer). | P0 |
| CD-7 | Sync through a shared folder (Dropbox, Google Drive, OneDrive, Syncthing) as a no-server option. | P1 |
| CD-8 | Service package: one file holding the order, songs, decks and media. Send it by email, USB stick or chat; it opens with a double click. Licensed Bible text is left out and pulled from the receiver's own library. | P0 |
| CD-9 | Guest speakers deliver decks without an account: a per-service upload link, or a pasted Google Slides or OneDrive share link. | P0 |
| CD-10 | Readiness check on the projection laptop before the service: every item, deck and video downloaded and cached, shown as a single green or red status. | P0 |
| CD-11 | App delivery: signed Windows installer, signed and notarised macOS DMG, Linux AppImage, Flatpak (Flathub) and .deb. Auto-update, never during a live service. Early adopters get macOS and Linux first (see Phasing). | P0 |
| CD-12 | Portable mode: run from a USB stick with the library beside it. | P2 |

### Live operation and outputs

| ID | Requirement | Priority |
| --- | --- | --- |
| LV-1 | Operator view: service order, preview, live, next slide; one key to go live. | P0 |
| LV-2 | Blank, black, logo and clear-text controls; announcement loop; countdown timer. | P0 |
| LV-3 | Map any number of outputs (projector, TV, stage display, stream) with their own layout and theme. | P0 |
| LV-4 | Themes for fonts, text position, shadows and motion or still backgrounds. | P0 |
| LV-5 | Phone and tablet remote over local Wi-Fi, plus a lyric follow-along view for the congregation. | P1 |
| LV-6 | MIDI, OSC and HTTP triggers for lights, audio and other systems. | P2 |

### Title safety and output calibration

Many churches project onto screens they cannot adjust: a rented venue TV that crops the edges, a projector bolted to the ceiling with the remote lost, or a lower screen edge hidden behind the band. Each output gets its own safe area, set by a volunteer standing in the room, and nothing we render leaves it.

| ID | Requirement | Priority |
| --- | --- | --- |
| TS-1 | Calibration mode per output: shows a grid with numbered edges and corner markers on that screen. A volunteer reads off which numbers they can see and drags each edge in until it is fully visible. Takes under 2 minutes. | P0 |
| TS-2 | Per-output safe area (top, bottom, left, right margins). All text, scripture, lyrics and lower thirds are laid out inside it. | P0 |
| TS-3 | Backgrounds and video may bleed to the full screen while text stays inside the safe area. The operator chooses bleed or fit per output. | P0 |
| TS-4 | Imported PowerPoint and Google Slides decks scale to fit the safe area, keeping their aspect ratio. | P0 |
| TS-5 | Scale, offset and corner pin (keystone correction) per output, for projectors that cannot be adjusted at the hardware. | P1 |
| TS-6 | Blocked zones: mark a region of a screen (for example the bottom quarter, hidden by the stage) where text never goes. | P1 |
| TS-7 | Text that would leave the safe area shrinks to a set minimum size. Below that, the slide is flagged in the editor and preview before the service. | P0 |
| TS-8 | Preview and live panes show each output's safe area as an overlay, so the operator sees what the room sees. | P0 |
| TS-9 | Each output adapts its layout to its aspect ratio (4:3, 16:9, 16:10, ultra-wide LED walls), not one stretched layout. | P0 |
| TS-10 | Calibration profiles are saved per room and recognised by the display's hardware ID, so plugging in the same screen restores them. | P1 |

### Reporting

| ID | Requirement | Priority |
| --- | --- | --- |
| RP-1 | Log every song shown, streamed or printed; export CCLI usage reports. | P0 |

## Non-functional requirements

| Area | Requirement |
| --- | --- |
| Offline | Every live feature works with no internet. Sync catches up when the connection returns. |
| Latency | Operator click to output frame under 50 ms. Remote and phone control under 150 ms on local Wi-Fi. |
| Reliability | The output window never shows a desktop, dialog or cursor. A crash restores the live slide within 5 seconds. |
| Hardware | Runs a 3-screen service on a 2019 laptop with 8 GB RAM and integrated graphics. |
| Platforms | Windows 10+, macOS 13+, Linux (AppImage, Flatpak, .deb), all from launch. Same file format on all three. Early adopters start on macOS and Linux, with Windows as an unsigned preview (see Phasing). |
| Slide fidelity | Imported PowerPoint and Google Slides look the same as in their source apps on 95% of a test corpus of 200 real church decks. |
| Accessibility | Operator UI meets WCAG 2.2 AA. All live actions have keyboard shortcuts. |
| Data | Library, services and media export to open formats (OpenLyrics XML, PPTX, PDF, JSON). No lock-in. |
| Security | Team accounts with roles (admin, planner, operator, viewer). Remote control needs a pairing code. |
| Licensing | Shows CCLI licence and song copyright lines; logs every song shown for reporting. |

## Architecture, phasing, risks and open questions

**Architecture.** A desktop app for Windows, macOS and Linux holds the full library and every service locally, so Sunday never depends on the internet. Its main parts:

- **Render engine:** one GPU-accelerated renderer per output, applying that output's layout, theme and safe area.
- **Slide engine:** parses PPTX (including Google Slides exports) into our own scene format and plays builds, transitions and media.
- **Local server:** serves the OBS browser source, the phone remote, the stage display and the congregation lyric view on the local network; also sends NDI.
- **Sync (local-first):** every device holds the full data and merges changes over the church network, a self-hosted relay or a shared folder (CD-1 to CD-7). No central cloud is required.
- **Connectors:** Google Drive and Slides, OneDrive, CCLI SongSelect, Planning Center, OBS WebSocket.

**Technology: Rust core, web UI.** Build the core in Rust, inside a Tauri shell, with the operator and planning screens in a web UI that the browser planner shares.

- **Why Rust:** the output must never freeze or crash mid-service. Rust prevents whole classes of memory bugs and has no garbage-collector pauses. It also keeps the app small (tens of MB, not Electron's 150+) and light enough for an old church laptop, and the team already builds in it.
- **Outputs render natively, not in a webview.** Each projector, stage and stream output is a native window drawn with wgpu (Metal, DirectX 12, Vulkan). Tauri uses a different webview on each OS (WebView2, WebKit), and those differ in video codecs and frame timing, so the webview is used only for the operator UI.
- **Video and media** decode in process with DeckCraft's pure-Rust decoders (H.264, HEVC, VP9, AV1), so nothing has to be installed beside the app. NDI, Spout and Syphon bring their own SDKs in Phase 2.
- **Slide engine in Rust:** a PPTX parser into our own scene format, rendered by the same wgpu engine. This is the largest build item: no mature open-source Rust PPTX renderer exists. The closest is DeckCraft, from the ArtCraft team: a pure-Rust PowerPoint clone that opens and presents .pptx files. It is alpha (around v0.5), its reader is tested only on decks it made itself, and its code licence still needs confirming (reported as MIT or Apache-2.0, both usable in a GPL-3.0 project). Evaluate it first as the base for our reader, and contribute our church-deck test corpus upstream; budget to build our own if it does not fit.
- **Trade-offs:** a smaller hiring pool than TypeScript, and slower UI iteration on the Rust side. Keeping all UI in web code and only the engine in Rust limits both.

**Migration.** A first-run wizard finds OpenSong, OpenLP, EasyWorship, ProPresenter and FreeShow data on the machine and imports songs, sets, themes and media in one pass, with a report of anything it could not carry over.

**Phasing.**

Before Phase 1, an early-adopter release goes to a small, hand-picked group of churches. It ships on macOS (signed and notarised DMG) and Linux (AppImage and .deb). A Windows build is offered as an unsigned preview: SmartScreen warns on install, and churches that choose it know that. Windows code signing and Flathub follow by launch. This keeps the slowest step, getting a Windows signing certificate, off the early-adopter path; the code is already tested on Windows in CI. Most church projection laptops run Windows, so choose early adopters with this in mind.

1. Phase 1, launch: all P0 items. Windows, macOS and Linux; one to three outputs; LAN and relay sync; service packages; PowerPoint and Google Slides import, OBS browser source, title-safe calibration.
2. Phase 2: P1 items. Browser planner, shared-folder sync, Planning Center sync, NDI, OBS scene control, corner pin, phone remote, translations side by side.
3. Phase 3: P2 items. MIDI and OSC triggers, PowerPoint fallback mode, church-theme override for decks.

**Risks.**

| Risk | Impact | Mitigation |
| --- | --- | --- |
| PPTX rendering never reaches PowerPoint's fidelity | Speakers lose trust; churches keep PowerPoint | Test corpus of 200 real church decks in CI; fidelity report on every import; fallback mode (PS-13) |
| Google API changes, quotas or app review | Google Slides import breaks or is delayed | drive.file scope only; cache exports locally; start Google verification early |
| CCLI SongSelect and Bible publishers restrict API access | Weaker library on day one | Apply for partnerships before build; ship with public domain texts and file import |
| OBS WebSocket protocol changes | Scene control breaks | Browser source (OB-2) needs no protocol; pin to WebSocket v5 and test each OBS release |
| Volunteer hardware varies widely | Dropped frames, wrong screen goes live | Hardware check on first run; output mapping by display ID; performance budget in NFRs |

**Decisions.**

- **Free and open source.** No paid tier. Licensed GPL-3.0, matching OpenSong and OpenLP and keeping forks open. Development is funded by donations and sponsors.
- **Rust,** as recommended: Tauri shell, web UI for operator and planning screens, wgpu for outputs, GStreamer (gstreamer-rs) for media and NDI. GStreamer maintains the Rust bindings itself, and its Rust plugin set includes NDI.
- **Linux at launch,** as AppImage, Flatpak and .deb, alongside Windows and macOS.
- **Bible translations are imported by the church,** which holds the licence. We bundle public domain texts only (BI-2).
- **The desktop app runs the service.** Planning also works in a browser, served by the sync relay (CD-5), so volunteers can plan without installing anything.

**Open questions.**

- [ ] Does the project run a free public sync relay (CD-4), and who pays for it?
- [ ] Product name (working codename: Lampstand).
