# DeckCraft as the base for Lampstand's PPTX reader

Oct 19, 2026 · Evaluated: DeckCraft v0.8.0, commit `010941e` (2026-10-18), github.com/storytold/deckcraft

## Recommendation

**Adopt DeckCraft's reader as the base, as a pinned dependency, not a hard fork.** Depend on
`deckcraft-pptx` and `deckcraft-model` at a fixed git revision, convert the DeckCraft model into
Lampstand's own scene format inside `lampstand-slides`, and send fixes upstream. Keep a fork ready
(a Cargo `[patch]` to our own branch) for any fix upstream does not take within a week.

The PRD's question is whether `lampstand-slides` takes months or a quarter. With DeckCraft, the
**reader** is mostly done. What is left is our own work in either case: the conversion to our scene
format, the wgpu renderer, the build and transition player, the fidelity report (PS-4) and the
media pipeline.

Evidence, in short:

- **Licence:** MIT OR Apache-2.0, confirmed in the repository. Every transitive dependency of
  the reader is permissive. All of it is compatible with GPL-3.0-or-later.
- **Fit:** the reader is a small library crate with no UI or rendering dependencies (43 crates in
  its tree, builds in about 20 s). Its model is rich: masters, layouts, placeholders, themes, text
  runs, pictures, media, transitions, animations and notes.
- **Fidelity on 49 decks (339 slides):**
  - Parsing: every valid deck parses, with no panics. The one failure is a corrupt zip that
    Python's `zipfile` also rejects.
  - Content: text, speaker notes, pictures, video, audio, masters, layouts and transitions match
    the package contents on all 48 decks that parse.
  - Builds: one real gap. The reader folds per-paragraph effects into one "by paragraph" build,
    so 7 of 39 slides with click builds play a different number of clicks. The fix is small and
    contained (see Fidelity).
- **Upstream health:**
  - Very active: 957 commits in 13 days, 52 authors, median PR merge in 20 hours.
  - Very young: 2 weeks old, alpha.
  - Moves fast, and much of the code is written by AI agents.
  - A bot triages issues, and auto-closed the one community report of a real-world corpus run.
  - It needs Rust 1.95; Lampstand's minimum is 1.90.

  All of this argues for pinning a revision, not tracking `main`.

## 1. Licence

- `Cargo.toml` declares `license = "MIT OR Apache-2.0"`. The repository ships `LICENSE-MIT` and
  `LICENSE-APACHE`, and `NOTICE` says "Licensed under the MIT License or the Apache License,
  Version 2.0, at your option". GitHub's licence detection shows only Apache-2.0 because it
  reads one file. The real licence is the dual one.
- Both options can be combined into a GPL-3.0-or-later program. MIT is GPL-compatible. Apache-2.0
  is compatible with GPLv3 (not v2), which is the version Lampstand uses.
- Transitive dependencies of `deckcraft-pptx` + `deckcraft-model` + `deckcraft-anim`
  (`cargo tree`):
  - 35 are MIT OR Apache-2.0.
  - 4 are MIT only.
  - `memchr` is Unlicense OR MIT.
  - `zlib-rs` is Zlib.
  - `zopfli` is Apache-2.0 only.
  - One crate is (MIT OR Apache-2.0) AND Unicode-3.0.

  None is copyleft and none is GPL-incompatible.
- The media crate (not needed for the reader) uses `symphonia` under MPL-2.0, which is also
  GPL-3.0-compatible.
- Not covered by the code licence: the ArtCraft name and logos in `docs/brand/` (trademarks).
  We would not ship those anyway.
- **Contributing back:** patches we send upstream must be licensed MIT OR Apache-2.0. Code we
  write inside Lampstand is GPL and cannot flow back unless the author relicenses it. DeckCraft's
  rules also bar committing files produced by PowerPoint or other vendor products (`AGENTS.md`
  §1.1: "Never commit files produced by PowerPoint"). The PRD's plan to "contribute our
  church-deck test corpus upstream" therefore has to change: upstream accepts only synthetic
  reproductions of each failure, not the real decks.

## 2. Fit

**Library use apart from the UI: yes.** The workspace is split by layer (`AGENTS.md` §4).

| Crate | What it is | Needed for the reader |
|---|---|---|
| `deckcraft-pptx` | OOXML package reader and writer (3,900 lines read, 4,900 write) | yes |
| `deckcraft-model` | Presentation model: masters, layouts, slides, shapes, text, themes, inheritance | yes |
| `deckcraft-geom`, `deckcraft-color` | Units, transforms, preset shape geometry (on `kurbo`), colours | yes, via the model |
| `deckcraft-anim` | Timeline (click steps, by-paragraph expansion), show navigation, transition and morph geometry | optional, useful for PS-3 |
| `deckcraft-fonts`, `deckcraft-text`, `deckcraft-render` | Font database and substitution, text layout, CPU renderer (`vello_cpu`) | optional |
| `deckcraft-media` + codec crates | Pure-Rust H.264, HEVC, VP9, AV1, Opus decoders; `symphonia` for audio | no |
| `deckcraft-engine`, `deckcraft-ui-egui`, apps | Command engine, egui UI, desktop app | no |

- **Public API:** `deckcraft_pptx::import(&[u8]) -> Result<Presentation, PptxError>`, and
  `import_checked`, which also returns the slides it had to skip. The reader is lenient by
  design: unknown parts are skipped, malformed parts are logged and ignored, and sizes and
  recursion depths are bounded. `unwrap`, `expect` and `panic` are denied in all production
  crates, and `unsafe` is forbidden.
- **Model it parses into (`deckcraft_model::Presentation`):**
  - Slide size.
  - Masters (theme, colour map, text styles, shapes, layouts).
  - Slides, each holding:
    - layout reference;
    - `Vec<Shape>`;
    - background;
    - `Option<Transition>`;
    - `Vec<Animation>`, a flat Animation Pane list: class, effect id, start, paragraph, text
      build, trigger, motion path, timing;
    - notes as a formatted `TextBody`;
    - comments.
  - Media items (bytes or external link).
  - Sections, custom shows, show settings, embedded fonts, table styles.
  - Raw XML kept for round trip.
- **Shape kinds:** autoshape or text box, picture, group, connector, table, chart, media clip
  (with trim, fade, volume, loop, autoplay, bookmarks), ink, 3-D model, and `Opaque` (SmartArt
  without a drawing, OLE). `Opaque` keeps its XML and fallback picture.
- **What a lyric or sermon deck needs is all there.** Placeholders keep their inheritance chain
  (`xfrm: None` means "inherit from layout or master"). A renderer therefore has to resolve that
  chain, and DeckCraft's model has helpers for it (`resolve.rs`).
- **Direct dependencies of the reader:** `quick-xml` 0.41, `zip` 3 (deflate), `thiserror` 2,
  `log`, `serde`, `serde_json`, `unicode-segmentation`, `kurbo` 0.13. That is 43 crates in all,
  and a release build from scratch takes about 20 s.
- **Rust version:** `rust-version = "1.95"`, edition 2024. Lampstand's workspace says 1.90 and
  edition 2021. Editions are per crate, so 2024 is no problem. Adopting DeckCraft raises
  Lampstand's minimum Rust to 1.95 and the CI `msrv` job to match. Stable today is 1.99.
- **wgpu: no.** Rendering is CPU-only through `vello_cpu`; only the egui app and the web app
  pull in wgpu. We render with our own wgpu engine, as the PRD plans. One option worth keeping:
  `deckcraft-render` can rasterise a slide or a single shape layer. At 1920×1080 on an Apple M1 it
  takes 20 to 90 ms per slide, including PNG encoding. That is fast enough to pre-render static
  layers into textures before a service. It would be a quick path to fidelity for content our
  own renderer does not draw yet (charts, SmartArt, WordArt, 3-D bevels).

## 3. Fidelity

### Method

The harness is in [`tools/deckcraft-eval`](../../tools/deckcraft-eval). It is a scratch crate:
not a workspace member and not built by CI. For each deck it does three things:

1. Counts what the package actually holds by reading its XML directly:
   - slides (in `sldIdLst` order), text characters, shapes, pictures, video and audio;
   - masters and layouts;
   - transitions that have an effect or an auto-advance time;
   - animation effects and main-sequence click effects per slide;
   - speaker-notes body text.

   It skips `mc:Fallback` branches so that alternate content is not counted twice.
2. Imports the deck with `deckcraft_pptx::import_checked` inside `catch_unwind`, and counts the
   same things in the model.
3. Asks DeckCraft's own player (`deckcraft_anim::ShowState::timeline`) how many clicks each
   slide takes, and compares that with the file.

That gives an operator's view: does slide 7 still take three clicks?

### Corpus

There are 49 decks and 339 slides; none are committed (see "Corpus sources" below).

| Group | Decks | What they cover |
|---|---:|---|
| Church-service decks, generated | 4 | Two lyric decks: a full-bleed picture per slide, white centred text with shadow, fades, push and cut, background picture and logo on the master. A sermon deck: title layout, scripture, a bullet build one paragraph per click, a picture with fly-in, grow and fade-out, notes on every slide. A pre-service announcements loop: auto-advance, embedded MP4 video with poster frame, paragraph builds. |
| Same four, exported by Keynote 14.1 | 4 | Real Keynote-written PPTX |
| Same four, exported by LibreOffice 25.8 | 4 | Real LibreOffice-written PPTX |
| Apache POI test data | 20 | Real-world decks scraped from the web by the POI project (lectures, conference talks, leadership summit, PowerPoint 2007 to 365), embedded video and audio, themes, masters. Includes one corrupt file. |
| LibreOffice test data | 17 | PowerPoint-written feature decks: transitions (including the PowerPoint 2013 airplane), narration audio, triggers, effect order, grow and swivel, SmartArt animation, sections, notes, many masters and layouts |

Gaps in the corpus:

- **No deck saved by PowerPoint itself.** The PowerPoint on the test machine (16.108) has no
  licence and opens read-only. 28 of the public decks were written by PowerPoint 2007 to 365, so
  PowerPoint output is still well represented.
- **No Google Slides export.** Google's export URL needs a signed-in Google account or a public
  deck, and none suitable was found.
- **No real church decks.** That needs the 200-deck church corpus the PRD plans for CI; it should
  be collected from churches with permission.

### Results

| What | Package holds | DeckCraft kept | |
|---|---:|---:|---|
| Decks that parse | 49 | 48 | The failure, `Divino_Revelado.pptx`, is a corrupt zip (no end-of-central-directory record); Python's `zipfile` rejects it too. DeckCraft returns a clean error. |
| Panics | | 0 | Every import ran inside `catch_unwind` |
| Slides | 339 | 339 | |
| Text characters | 58,013 | 58,194 | All text kept. The surplus is text DeckCraft reads from SmartArt drawings and equation fallbacks that the XML count skips. |
| Shapes | 3,190 | 3,204 | Same reason |
| Pictures | 145 | 145 | |
| Video / audio clips | 4 / 2 | 4 / 2 | Poster frames, trim and playback options are in the model |
| Masters / layouts | 70 / 651 | 70 / 651 | |
| Slides with a transition | 112 | 112 | 14 kinds seen, all mapped: fade, wipe, push, clock, cover, split, blinds, random bars, shape, cut, comb, dissolve, zoom, airplane. Unmapped kinds keep their XML. |
| Animation effects | 223 | 197 | The 26 "missing" are per-paragraph effects folded into one by-paragraph build (below) |
| Slides with speaker notes | 83 | 83 | 9,529 of 9,529 characters, formatting kept |
| Decks whose click count matches on every slide | 48 | 43 | 7 of the 39 slides with click builds differ (below) |
| Import time | | 0.2 to 128 ms per deck | The largest is 28 slides and 2,223 shapes |

Per-deck results are in the appendix.

### What it drops or gets wrong

1. **Paragraph builds are folded, so click counts change.** In
   `crates/pptx/src/read/timing.rs:175`, any effect that targets a paragraph of a shape with a
   `bldP build="p"` entry is collapsed into one `TextBuild::ByParagraph` animation. The player then
   expands that build into one click per top-level paragraph. This ignores each effect's start
   type and which paragraphs it targets. Seen in the corpus:
   - `ubc_HowWeRefactor.pptx` (PowerPoint 2007), slide 9: three paragraphs appear on one click
     (one `clickEffect` and two `withEffect`). DeckCraft plays 4 clicks.
   - Same deck, slide 27: 6 clicks become 18. Slide 21: 2 clicks become 1.
   - Keynote exports: Keynote writes a paragraph build as "fade the shape's background with
     paragraph 1, then one paragraph per click". DeckCraft folds it into a whole-shape fade, so on
     the sermon deck all three points appear on the first click (3 clicks in the file, 1 in
     DeckCraft).
   - Our generated decks, which give each paragraph effect its own `grpId`, play 3 clicks as 5.

   For a church operator this is the worst kind of error: the clicker no longer matches the
   speaker's notes. The fix is contained. The model already has `Animation::paragraph`, so the
   reader only has to fold when every effect in the run is on-click, one per consecutive
   paragraph, and keep separate per-paragraph animations otherwise. Builds that are strictly
   one paragraph per click (`2411-Performance_Up`, `alterman_security`, LibreOffice exports)
   already match.
2. **By-word and by-letter text animation is lost.** The reader ignores `p:iterate`, so
   `TextBuild::ByWord` and `ByLetter` are never set on import. The effect plays on the whole
   paragraph instead.
3. **No structured report of what it skipped.** Drops are reported only through `log::warn!`,
   plus the list of unreadable slides from `import_checked`. `Opaque` shapes (one OLE Photoshop
   object in the corpus) and linked media (`MediaItem::link`) can be found by walking the model.
   There is no list of unsupported effects or missing fonts. PS-4 has to be built by us.
4. **Media playback is a separate risk.** The reader keeps video and audio clips and their
   options. DeckCraft's own pure-Rust video decoding has an open report of GIFs and videos
   playing only their first frames (#873). Lampstand should keep its own media pipeline.
5. **Not tested by this harness:** visual fidelity against PowerPoint renders. A spot render with
   `deckcraft-cli` of the lyric, sermon and IHEP conference decks looked right (backgrounds,
   master picture, centred text with shadows, bullets, charts) apart from minor chart artefacts.
   But rendering is ours to build, so the reader's model is what matters here.

Producer quirks found along the way, all outside DeckCraft:

- Keynote drops the first slide's transition on export.
- Keynote shifts auto-advance times by one slide.
- Keynote refuses to import python-pptx decks that have notes slides.
- PowerPoint writes empty `<p:transition/>` elements that mean "none".

Lampstand's own fidelity report will need to know these too.

## 4. Upstream health

Taken 2026-10-19.

| | |
|---|---|
| Age | First commit 2026-10-05; public 2026-10-07 |
| Commits | 957 in 13 days. 503 on one day (2026-10-14, a community PR wave); 10 to 31 a day since |
| Authors | 52. Brandon Thomas (ArtCraft) wrote 665 commits, Nikita Rakitin 183, the rest are community |
| Releases | v0.1.0 to v0.8.0 in 12 days (v0.2 skipped); installers for macOS, Windows, Linux, FreeBSD |
| Stars, forks | 3,848 and 2,178 |
| Issues | 346 filed, 185 open, 65 open with no comment; median 34 hours to close |
| Pull requests | 688: 411 merged, 222 open, 55 closed; median 20 hours from open to merge |
| PPTX issues | 52 mention PPTX or import; 30 are open (for example #810, text colours reset to black on open; #873, video plays only the first frames; #74, media options lost on save) |
| Process | `AGENTS.md` and `CLAUDE.md` drive AI coding agents, which wrote much of the code. A "Ticket Relay" bot files Discord bug reports and triages issues. It auto-closed #781, a community run of 12 real-world decks, two minutes after it was filed. |
| Own view of the gap | `docs/gaps.md` ranks "PPTX corpus is only decks we author (no PowerPoint reference renders)" as gap number 1 |

Maintainers respond quickly and merge a lot. The risks:

- churn (the model and reader change daily);
- review depth on a codebase growing by hundreds of commits a day;
- the reader's real-world testing is only starting, which this evaluation confirms.

Pinning a revision and bumping it deliberately, behind our own corpus run, handles the churn.

## Coverage of PS-1, PS-3, PS-4 and PS-7

| Req | What DeckCraft's reader covers | What Lampstand still builds | Coverage |
|---|---|---|---|
| PS-1: import PPTX, render with our own engine | Parses PowerPoint 2007 to 365, Keynote and LibreOffice exports. 48 of 48 valid decks, no panics, at most 128 ms. Masters, layouts, placeholder inheritance and themes are modelled. | Drag and drop, file picker, OneDrive link fetch. Conversion to our scene format. The wgpu renderer (DeckCraft has none; its CPU renderer is an optional stopgap). | Reader: high. Rendering: none. |
| PS-3: keep text, shapes, images, masters, video and audio, transitions, animations; one click per build step | Text, shapes, pictures, masters, media clips, transitions and effect types all kept. A timeline that groups effects into click steps (`deckcraft-anim`). | Fix the paragraph-build fold upstream (7 of 39 animated slides wrong today). Add by-word and by-letter. Playback of video and audio. Transition and effect rendering in wgpu. | Medium-high after the fix |
| PS-4: fidelity report (missing fonts and substitutes, unsupported effects, unfetchable linked media) | Building blocks only: skipped slides, `Opaque` shapes, unknown transitions kept as raw XML, `MediaItem::link`, and a font substitution table in `deckcraft-fonts`. | The report itself: walk the model, check fonts against what we bundle, try linked media, list unsupported effects per slide. | Low |
| PS-7: speaker notes to confidence monitor and stage display | Notes kept on all 83 slides that have them, as formatted text. `deckcraft-render` can draw slide thumbnails. | Stage display, thumbnails in our renderer, timer. | High for the data |

## Next steps

1. Open an upstream issue and PR for the paragraph-build fold, with a synthetic fixture (MIT OR
   Apache-2.0, as their rules require) reproducing the UBC slide 9 and Keynote patterns.
2. In `lampstand-slides`, add `deckcraft-pptx` and `deckcraft-model` pinned to a revision, raise
   the workspace `rust-version` to 1.95 and the CI `msrv` job with it. Write the model-to-scene
   conversion for text, pictures, backgrounds and masters first: that covers lyric decks.
3. Turn `tools/deckcraft-eval` into the CI corpus check the PRD calls for. Run it over the church
   corpus on each DeckCraft bump, and fail on any regression in counts or click steps.
4. Collect real church decks (lyric, sermon, announcements, guest-speaker decks with video) with
   permission, and add PowerPoint-saved and Google Slides exports to the corpus.

## Corpus sources

The decks are not committed: licences vary and many are scraped from the web.
`tools/deckcraft-eval/fetch_corpus.sh` downloads the public ones, and
`tools/deckcraft-eval/make_church_decks.py` regenerates the church decks.

- **Apache POI** `test-data/slideshow/` (github.com/apache/poi, trunk), 20 decks:
  `2411-Performance_Up`, `Divino_Revelado`, `EmbeddedAudio`, `EmbeddedVideo`, `KEY02`,
  `LIBRE_OFFICE-100610-0`, `SampleShow`, `WithMaster`, `aascu.org_hbcu_leadershipsummit_cooper_`,
  `ae.ac.uaeu.faculty_nafaachbili_GeomLec1`, `alterman_security`,
  `au.asn.aes.www_conferences_2011_presentations_Fri_20Room4Level4_20930_20Maloney`, `backgrounds`,
  `ca.ubc.cs.people_~emhill_presentations_HowWeRefactor`, `ececapstonespring2012`, `keyframes`,
  `present1`, `rain`, `themes`, `tika-2605`. The repository is Apache-2.0. Several of these files
  are real-world decks collected from the public web, and their own licences are unstated.
- **LibreOffice** `sd/qa/unit/data/pptx/` (github.com/LibreOffice/core, master; MPL-2.0
  repository; bug-report attachments), 17 decks: `airplane-transition`,
  `animation-sound-embedded`, `connector-shape-animations`, `effectOrder`, `loopNoPause`,
  `master-and-eleven-layouts`, `master-slides`, `media-placeholder`, `narration`,
  `pres-with-notes`, `slide-sections`, `subtitle-animation-save`, `tdf104792-smart-art-animation`,
  `tdf113818-swivel`, `tdf148478_transitionMusic`, `tdf98477grow`, `trigger`.
- **Church-service decks, generated for this evaluation** (python-pptx 1.0.2, our own script):
  `lyrics-amazing-grace`, `lyrics-holy-holy-holy`, `sermon-living-water`, `announcements-loop`.
  - Text: "Amazing Grace" (John Newton, 1779) and "Holy, Holy, Holy" (Reginald Heber, 1826), both
    public domain; John 4:13–14 from the World English Bible (public domain).
  - Images and video: generated with ffmpeg (gradient, Mandelbrot, test pattern with a tone).
  - Each was then exported again by Keynote 14.1 and by LibreOffice 25.8 (12 decks in all).

## Appendix: per-deck results

"m/t" means DeckCraft kept m where the package holds t. A single number means they match.
Clicks are main-sequence clicks summed over the deck; ⚠ marks a deck where at least one slide
differs.

| Deck | Written by | Parses | Slides | Text chars | Shapes | Pictures | Video / audio | Masters | Transitions | Effects | Clicks (DeckCraft/file) | Notes chars | Import ms |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `church-src/announcements-loop.pptx` | python-pptx 1.0.2 | yes | 3 | 82 | 6 | 2 | 1 / 0 | 1 | 3 | 2 | 3/2 ⚠ | 22 | 7.3 |
| `church-src/lyrics-amazing-grace.pptx` | python-pptx 1.0.2 | yes | 5 | 451 | 10 | 5 | 0 / 0 | 1 | 5 | 0 | 0/0 | 216 | 4.8 |
| `church-src/lyrics-holy-holy-holy.pptx` | python-pptx 1.0.2 | yes | 4 | 450 | 3 | 0 | 0 / 0 | 1 | 4 | 0 | 0/0 | 154 | 3.8 |
| `church-src/sermon-living-water.pptx` | python-pptx 1.0.2 | yes | 5 | 446 | 11 | 1 | 0 / 0 | 1 | 5 | 7 | 9/7 ⚠ | 226 | 5.1 |
| `church-keynote/announcements-loop.pptx` | Keynote 14.1 export | yes | 3 | 82 | 6 | 2 | 1 / 0 | 1 | 3 | 4/5 | 1/2 ⚠ | 22 | 6.8 |
| `church-keynote/lyrics-amazing-grace.pptx` | Keynote 14.1 export | yes | 5 | 451 | 10 | 5 | 0 / 0 | 1 | 4 | 0 | 0/0 | 216 | 4.9 |
| `church-keynote/lyrics-holy-holy-holy.pptx` | Keynote 14.1 export | yes | 4 | 450 | 3 | 0 | 0 / 0 | 1 | 3 | 0 | 0/0 | 154 | 4.7 |
| `church-keynote/sermon-living-water.pptx` | Keynote 14.1 export | yes | 5 | 446 | 11 | 1 | 0 / 0 | 1 | 4 | 6/8 | 5/7 ⚠ | 226 | 5.1 |
| `church-lo/announcements-loop.pptx` | LibreOffice 25.8 export | yes | 3 | 82 | 6 | 2 | 1 / 0 | 1 | 3 | 2 | 2/2 | 22 | 6.0 |
| `church-lo/lyrics-amazing-grace.pptx` | LibreOffice 25.8 export | yes | 5 | 451 | 10 | 5 | 0 / 0 | 1 | 5 | 0 | 0/0 | 216 | 3.6 |
| `church-lo/lyrics-holy-holy-holy.pptx` | LibreOffice 25.8 export | yes | 4 | 450 | 3 | 0 | 0 / 0 | 1 | 3 | 0 | 0/0 | 154 | 4.1 |
| `church-lo/sermon-living-water.pptx` | LibreOffice 25.8 export | yes | 5 | 446 | 11 | 1 | 0 / 0 | 1 | 5 | 7 | 7/7 | 226 | 3.9 |
| `poi/2411-Performance_Up.pptx` | PowerPoint 2007 | yes | 48 | 6064 | 144 | 8 | 0 / 0 | 1 | 0 | 5/11 | 10/10 | 1123 | 8.7 |
| `poi/Divino_Revelado.pptx` | ? | **no**: corrupt zip (no end of central directory) | | | | | | | | | | | |
| `poi/EmbeddedAudio.pptx` | PowerPoint (Mac) 2016+ | yes | 1 | 0 | 1 | 0 | 0 / 1 | 1 | 0 | 1 | 1/1 | 0 | 1.6 |
| `poi/EmbeddedVideo.pptx` | PowerPoint 2016+ | yes | 1 | 0 | 1 | 0 | 1 / 0 | 1 | 0 | 2 | 1/1 | 0 | 1.6 |
| `poi/KEY02.pptx` | PowerPoint (Mac) 2011 | yes | 1 | 101 | 2 | 0 | 0 / 0 | 2 | 1 | 0 | 0/0 | 0 | 3.2 |
| `poi/LIBRE_OFFICE-100610-0.pptx` | LibreOffice 5.1 | yes | 17 | 1118 | 48 | 5 | 0 / 0 | 2 | 0 | 0 | 0/0 | 0 | 5.8 |
| `poi/SampleShow.pptx` | PowerPoint 2016+ | yes | 2 | 178 | 4 | 0 | 0 / 0 | 1 | 0 | 0 | 0/0 | 69 | 1.8 |
| `poi/WithMaster.pptx` | PowerPoint 2010 | yes | 2 | 70 | 5 | 0 | 0 / 0 | 1 | 0 | 0 | 0/0 | 0 | 1.7 |
| `poi/aascu…cooper_.pptx` | PowerPoint 2007 | yes | 16 | 7486 | 99 | 7 | 0 / 0 | 9 | 16 | 0 | 0/0 | 0 | 21.6 |
| `poi/ae.ac.uaeu…GeomLec1.pptx` | PowerPoint 2007 | yes | 14 | 2283/2108 | 46/34 | 3 | 0 / 0 | 1 | 0 | 1 | 1/1 | 0 | 3.6 |
| `poi/alterman_security.pptx` | PowerPoint 2010 | yes | 23 | 7066 | 96 | 4 | 0 / 0 | 1 | 0 | 1/12 | 5/5 | 0 | 7.7 |
| `poi/au.asn.aes…Maloney.pptx` | PowerPoint 2010 | yes | 28 | 12707 | 94 | 7 | 0 / 0 | 1 | 0 | 0 | 0/0 | 4041 | 6.7 |
| `poi/backgrounds.pptx` | PowerPoint 2010 | yes | 4 | 128 | 16 | 0 | 0 / 0 | 1 | 0 | 0 | 0/0 | 0 | 1.8 |
| `poi/ca.ubc…HowWeRefactor.pptx` | PowerPoint 2007 | yes | 28 | 5400 | 2223 | 69 | 0 / 0 | 1 | 28 | 146/152 | 74/60 ⚠ | 1513 | 128.1 |
| `poi/ececapstonespring2012.pptx` | unknown | yes | 24 | 3224 | 49 | 3 | 0 / 0 | 2 | 0 | 0 | 0/0 | 0 | 4.2 |
| `poi/keyframes.pptx` | PowerPoint 2007 | yes | 28 | 5484 | 117 | 0 | 0 / 0 | 1 | 16 | 1 | 1/1 | 0 | 9.6 |
| `poi/present1.pptx` | PowerPoint 2007 | yes | 1 | 4 | 3 | 0 | 0 / 0 | 1 | 0 | 0 | 0/0 | 0 | 1.7 |
| `poi/rain.pptx` | PowerPoint (Mac) 2016+ | yes | 1 | 4 | 1 | 0 | 0 / 0 | 1 | 0 | 0 | 0/0 | 0 | 1.5 |
| `poi/themes.pptx` | PowerPoint 2010 | yes | 10 | 538 | 20 | 1 | 0 / 0 | 7 | 0 | 0 | 0/0 | 0 | 14.6 |
| `poi/tika-2605.pptx` | PowerPoint 2016+ | yes | 14 | 0 | 14 | 12 | 0 / 0 | 1 | 0 | 0 | 0/0 | 0 | 3.0 |
| `lo/airplane-transition.pptx` | PowerPoint 2016+ | yes | 1 | 0 | 2 | 0 | 0 / 0 | 1 | 1 | 0 | 0/0 | 0 | 1.5 |
| `lo/animation-sound-embedded.pptx` | unknown | yes | 1 | 0 | 0 | 0 | 0 / 0 | 1 | 0 | 0 | 0/0 | 0 | 0.2 |
| `lo/connector-shape-animations.pptx` | PowerPoint (Mac) 2016+ | yes | 1 | 0 | 3 | 0 | 0 / 0 | 1 | 0 | 2 | 1/1 | 348 | 1.1 |
| `lo/effectOrder.pptx` | PowerPoint 2013 | yes | 1 | 0 | 3 | 0 | 0 / 0 | 1 | 0 | 0 | 0/0 | 0 | 1.4 |
| `lo/loopNoPause.pptx` | PowerPoint 2016+ | yes | 1 | 0 | 2 | 0 | 0 / 0 | 1 | 0 | 0 | 0/0 | 0 | 1.8 |
| `lo/master-and-eleven-layouts.pptx` | PowerPoint 2016+ | yes | 1 | 0 | 2 | 0 | 0 / 0 | 1 | 0 | 0 | 0/0 | 0 | 1.7 |
| `lo/master-slides.pptx` | PowerPoint 2016+ | yes | 1 | 134 | 3 | 0 | 0 / 0 | 2 | 0 | 0 | 0/0 | 0 | 2.2 |
| `lo/media-placeholder.pptx` | PowerPoint 2016+ | yes | 1 | 0 | 1 | 0 | 0 / 0 | 1 | 0 | 0 | 0/0 | 0 | 1.4 |
| `lo/narration.pptx` | PowerPoint 2013 | yes | 1 | 0 | 1 | 0 | 0 / 1 | 1 | 1 | 1 | 0/0 | 0 | 1.4 |
| `lo/pres-with-notes.pptx` | LibreOffice 24.2 | yes | 2 | 0 | 4 | 0 | 0 / 0 | 2 | 0 | 0 | 0/0 | 0 | 0.9 |
| `lo/slide-sections.pptx` | PowerPoint 2016+ | yes | 7 | 1881 | 97 | 0 | 0 / 0 | 3 | 0 | 0 | 0/0 | 581 | 9.7 |
| `lo/subtitle-animation-save.pptx` | unknown | yes | 1 | 29 | 2 | 0 | 0 / 0 | 1 | 0 | 2 | 0/0 | 0 | 0.4 |
| `lo/tdf104792-smart-art-animation.pptx` | PowerPoint 2016+ | yes | 1 | 6/0 | 3/1 | 0 | 0 / 0 | 1 | 0 | 2 | 2/2 | 0 | 1.7 |
| `lo/tdf113818-swivel.pptx` | PowerPoint 2010 | yes | 1 | 2 | 1 | 0 | 0 / 0 | 1 | 0 | 1 | 1/1 | 0 | 1.5 |
| `lo/tdf148478_transitionMusic.pptx` | LibreOffice 26.2 alpha | yes | 2 | 0 | 2 | 2 | 0 / 0 | 2 | 2 | 0 | 0/0 | 0 | 1.9 |
| `lo/tdf98477grow.pptx` | PowerPoint 2013 | yes | 1 | 0 | 1 | 0 | 0 / 0 | 1 | 0 | 1 | 1/1 | 0 | 1.6 |
| `lo/trigger.pptx` | PowerPoint 2013 | yes | 1 | 0 | 4 | 0 | 0 / 0 | 1 | 0 | 3 | 0/0 | 0 | 1.5 |

The Keynote exports show one transition fewer than their sources because Keynote itself writes
the first slide's transition as empty; DeckCraft reads what Keynote wrote.
