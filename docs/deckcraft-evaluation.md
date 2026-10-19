# DeckCraft evaluation for the PPTX reader

Date: 2026-10-18. Covers PRD requirements PS-1 to PS-14
([docs/PRD.md](PRD.md); live copy:
[PRD: Church Projection Platform](https://claude.ai/artifact/2XEV6f8sbqfTfWPcgSabkU)).

**Recommendation: depend on DeckCraft's `deckcraft-pptx`, `deckcraft-model` and
`deckcraft-anim` crates, pinned to a commit, and fix what we need upstream.** Do
not fork. Do not build our own reader. The licence fits, the reader works as a
library with no UI, it opened every valid deck in a 95-deck public corpus
without a panic, and it played 38 of 42 animated slides with exactly the right
clicks. On visual fidelity, 91% of slides are within 0.20 RMSE of LibreOffice,
and most of the worst are charts. The gaps found are specific and fixable:
one animation-import bug (paragraph builds), chart labels and fills, and
font substitution for fonts that are not installed. Four real Google Slides
exports, three of them church decks, did even better: every slide opened,
all 404 clicks matched, and DeckCraft used the embedded fonts that
LibreOffice ignored. Church decks made in PowerPoint and a check against
PowerPoint itself are still open (see [Still to test](#still-to-test)).

## What was evaluated

[storytold/deckcraft](https://github.com/storytold/deckcraft) at commit
`22db926` (2026-10-17), version 0.8.0. It needs Rust 1.95 (edition 2024). Our
workspace declares `rust-version = "1.80"`, so adding the dependency means
raising that to 1.95 and updating local toolchains (this Mac's default is
1.91).

The test corpus is the 95 `.pptx` files in Apache POI's
[`test-data/slideshow`](https://github.com/apache/poi/tree/trunk/test-data/slideshow).
Most were made in PowerPoint for Windows or Mac, a few in LibreOffice, and
several are real presentations collected from the web. None came from Google
Slides and none is a church deck.

## Licence: compatible

The workspace licence is `MIT OR Apache-2.0` (`LICENSE-MIT` and
`LICENSE-APACHE` in the repo). Either can be combined into a
GPL-3.0-or-later program. The one copyleft dependency found is `symphonia`
for audio decoding (MPL-2.0), which is also GPL-3.0 compatible.

## Library use without the UI: yes

`deckcraft-pptx` depends only on `deckcraft-model`, `deckcraft-geom` and
`deckcraft-color`, plus `quick-xml`, `zip`, `thiserror` and `log`. It has no
UI or GPU dependency. `deckcraft-anim` depends only on the model. The
workspace bans `unsafe` code.

This was tested, not just read off `Cargo.toml`: a 50-line probe crate that
depends on `deckcraft-pptx` and `deckcraft-anim` alone imports each deck with
`deckcraft_pptx::import(&bytes)` and walks it with `deckcraft_anim::ShowState`
(`next()`, `step_done()`), the same click-by-click state machine the
DeckCraft app uses. It built and ran with no other DeckCraft crate.

Two ways to draw the result in Lampstand:

- **Our own wgpu renderer** draws DeckCraft's model and animation timeline.
  Best fit for the PRD (one renderer for every output, safe-area scaling,
  under 50 ms click to frame) but the most work.
- **DeckCraft's `deckcraft-render`** (a CPU rasteriser on `vello_cpu`) draws
  each frame and we upload it as a texture. Fastest path to a working import;
  costs CPU per frame during transitions and animations.

Start with the second for PS-1 and move hot paths to wgpu as needed.
`deckcraft-media` has pure-Rust H.264, HEVC, VP9 and AV1 decoders, but the PRD
picks GStreamer for media, so we would use DeckCraft only to find the media
parts and their timing.

## Real decks: robust parsing

Per-deck results: [eval/deckcraft-poi-run.tsv](eval/deckcraft-poi-run.tsv).

- 84 of 95 decks opened and rendered every slide to PNG, in 0.0 to 1.0 s per
  deck at 1 px per point (release build, Apple Silicon Mac).
- All 11 failures are corrupt files: 10 fuzzing test cases and
  `Divino_Revelado.pptx`, a truncated zip. Each failed cleanly with "not a
  presentation DeckCraft can read". There were no panics.
- Speaker notes, transitions and embedded audio and video were read on the
  decks checked by hand (`keyframes`, `2411-Performance_Up`, an aascu.org deck
  and `EmbeddedVideo`).

## Builds advance one click at a time: yes, with one bug

Per-slide results: [eval/deckcraft-builds.tsv](eval/deckcraft-builds.tsv).

For each visible slide, the number of clicks in the slide XML's main
animation sequence was compared with the number of clicks DeckCraft's show
took before moving to the next slide. A click step is a top-level step in the
main sequence that waits for a click; a step that starts on its own
(`onBegin`, "after previous" at slide start) is not a click.

- 547 visible slides; 42 of them, in 13 decks, have click builds, with 119
  clicks in all.
- **38 of 42 slides match exactly.** Every visible slide was reached and no
  hidden slide was shown.
- **4 slides differ, all paragraph builds** (`bldP build="p"`):

  | Deck | Slide | Clicks in PowerPoint | Clicks in DeckCraft |
  | --- | --- | --- | --- |
  | `customGeo` | 18 | 5 | 0 |
  | `HowWeRefactor` | 9 | 1 | 4 |
  | `HowWeRefactor` | 21 | 2 | 1 |
  | `HowWeRefactor` | 27 | 6 | 18 |

**Cause.** On import, `crates/pptx/src/read/timing.rs` collapses every
paragraph-by-paragraph build into one "by paragraph" animation. That keeps the
start type of the first paragraph only, then the timeline re-expands it to one
step per paragraph of the shape. So when the first paragraph comes in on its
own and the rest on clicks, every paragraph comes in on its own (`customGeo`
18). When several paragraphs share a click, each gets its own click
(`HowWeRefactor` 9).

This matters for us: sermon points revealed one at a time are the most common
build in church decks. The fix is contained: keep the per-paragraph steps as
written in the file, or keep the collapse only when the steps follow the
default pattern (one click per paragraph). It belongs upstream, with a test
built from these POI decks.

## Visual fidelity

Per-slide scores: [eval/deckcraft-lo-fidelity.tsv](eval/deckcraft-lo-fidelity.tsv).

**Method.** Each visible slide was rendered by DeckCraft at 1 px per point and
compared with the same slide exported to PDF by LibreOffice 25.8 and
rasterised at the same size, using ImageMagick's normalised RMSE (0 is
identical). LibreOffice was the reference because PowerPoint's AppleScript
export stalled; LibreOffice is itself imperfect, so the score is used to sort
slides and the worst ones were judged by eye. All 547 visible slides in the 84
readable decks were compared.

| RMSE against LibreOffice | Slides | Share |
| --- | --- | --- |
| 0.05 or less | 121 | 22% |
| 0.10 or less | 281 | 51% |
| 0.15 or less | 426 | 78% |
| 0.20 or less | 500 | 91% |
| 0.30 or less | 537 | 98% |

Median 0.098. Looked at side by side, slides up to about 0.15 are the same
slide with small font-metric, shadow and anti-aliasing differences; nobody in
a congregation would notice.

**The 12 worst slides, judged by eye:**

- **Charts (5 of 12): DeckCraft is worse.** Category labels are not rotated or
  wrapped, so they overlap; data labels are missing; a picture fill behind
  the plot area is not drawn (`chart-picture-bg`); a markers-only series is
  drawn as a line; legend placement differs. Charts are DeckCraft's weakest
  area. They matter less for church decks than for business decks, but
  announcement slides do use them.
- **Text set wider (4 of 12): neither is right.** These decks ask for fonts
  that are not installed on this Mac, such as Lucida Sans Unicode. DeckCraft
  and LibreOffice pick different substitutes, and DeckCraft's sets wider, so
  bullet text wraps onto more lines. Where a metric-compatible substitute
  exists DeckCraft uses it (Carlito for Calibri, Liberation for Arial and
  Times New Roman). This is what the PS-4 fidelity report must flag, and why
  church decks with custom fonts need testing.
- **Title size (2 of 12): DeckCraft is probably right.** On `customGeo` the
  title asks for 90% of the layout's size (`normAutofit fontScale="90000"`).
  DeckCraft draws that; LibreOffice shrinks it much further on its own.
- **ActiveX player (1 of 12): DeckCraft is reasonable.** `keyframes` slide 26
  holds a Windows Media Player ActiveX control, not embedded video.
  DeckCraft draws nothing there; LibreOffice draws a black player box.
  PowerPoint for Mac can't run ActiveX either. DeckCraft's title fits where
  LibreOffice's overflows.

Two LibreOffice faults also showed up in this set (a stray drawing artifact on
a chart, a title overflowing its box), which is why PowerPoint should be the
reference before launch.

## Google Slides exports and church decks: the best results

Per-slide results: [eval/deckcraft-google-slides.tsv](eval/deckcraft-google-slides.tsv).

Four real decks exported from Google Slides with File › Download ›
Microsoft PowerPoint, three of them from one church. Names are withheld; they
are a sermon deck (51 slides, Bible text on a textured background), weekly
notices (28 slides, photo backgrounds), a church slideshow (26 slides, photos
and an animated GIF) and an app walkthrough (12 slides, diagrams and speaker
notes).

| Check | Result |
| --- | --- |
| Open and render | 4 of 4 decks, all 117 slides |
| Speaker notes | Read on the one deck that has them (12 of 12 slides) |
| Hidden slides | 9, all skipped by the show |
| Click builds | 42 slides, 404 clicks: **every slide matches the file exactly** |
| Visuals against LibreOffice | Median RMSE 0.144; 94% of slides within 0.20; worst 0.221 |

The visual scores are higher than on the POI corpus, but here the difference
is LibreOffice's fault. Google Slides embeds the deck's fonts in the export
(Be Vietnam Pro, Lato, Poppins and others, in `ppt/fonts/`). DeckCraft reads
the embedded fonts (`embeddedFontLst`), so text sets as designed and fits
its boxes. LibreOffice ignores them and substitutes wider fonts, so titles
wrap and body text runs out of its boxes. On the slideshow deck DeckCraft also
draws an animated GIF (it decodes every frame with its timing) that
LibreOffice's PDF leaves out. No slide in these decks looked wrong in
DeckCraft.

## Still to test

- **Church decks made in PowerPoint.** The church decks tested so far came
  from Google Slides. Decks made in PowerPoint, with embedded video and
  paragraph builds, are still to test. The PRD's fidelity target (95% of 200
  real church decks) needs this corpus anyway; collect it now and contribute
  the ones we may share upstream.
- **PowerPoint as the reference.** LibreOffice was used because PowerPoint's
  AppleScript export stalled on a "file extension has changed" dialog for a
  valid `.pptx`. Exporting by hand from PowerPoint for a dozen slides would
  confirm the LibreOffice results. Click counts and 14 of these slides are
  now checked against PowerPoint; see the
  [addendum](#addendum-powerpoint-as-the-reference-2026-10-19).

## Against the PRD

| ID | What DeckCraft gives us | Gap |
| --- | --- | --- |
| PS-1 | PPTX import and rendering with no PowerPoint | Wrap in our import UI |
| PS-2 | Reads Google Slides PPTX exports, embedded fonts included | Drive picker and Slides API export are ours |
| PS-3 | Text, shapes, images, masters, media parts, transitions, animations, click builds | Paragraph-build bug; chart labels; fonts |
| PS-4 | Import failures are clean errors | Fidelity report is ours to build: missing fonts and unsupported effects need hooks in the reader |
| PS-7 | Speaker notes | None |
| PS-10 | Hidden slides respected by the show; custom shows in the model | Per-service overrides are ours |
| PS-11 | Slide text is in the model | Search is ours |
| PS-14 | DeckCraft also reads PDF and exports PPTX and PDF | Keynote is not covered |

## Decision

Depend, not fork, not build. Pin `deckcraft-pptx`, `deckcraft-model` and
`deckcraft-anim` to a commit when `lampstand-slides` work starts, raise
`rust-version` to 1.95, and send the paragraph-build fix and our test decks
upstream. Revisit if PowerPoint-made church decks show gaps that upstream
won't take. Not added as a dependency in this PR.

## Addendum: PowerPoint as the reference (2026-10-19)

Per-slide results: [eval/deckcraft-powerpoint-clicks.tsv](eval/deckcraft-powerpoint-clicks.tsv)
and [eval/deckcraft-powerpoint-visual.tsv](eval/deckcraft-powerpoint-visual.tsv).

This addendum takes up the two items under [Still to test](#still-to-test).
The test machine has PowerPoint for Mac 16.108 installed but unlicensed.

What the unlicensed PowerPoint can and can't do:

- **It can:** open and play decks.
- **It can't:**
  - edit, save or export ("Subscription Required to Edit and Save");
  - read `keyframes.pptx` at all ("can't read"). This is behind the "file
    extension has changed" alert that stalled the earlier export.
- **It warns:** on `ca.ubc…HowWeRefactor` it reports "a problem with
  content" and offers to repair it. DeckCraft and LibreOffice open both files
  without complaint.

### Church decks made in PowerPoint: blocked

Without a licence PowerPoint cannot create or save a deck, so none could be
made here. The options are a signed-in PowerPoint, or real church decks
collected from churches. The PRD's 200-deck corpus needs the real decks
anyway.

DeckCraft's contribution rules also bar committing files produced by
PowerPoint (its `AGENTS.md`, §1.1). Only synthetic reproductions of failures
can go upstream, not the church decks themselves.

### Click counts: PowerPoint plays what the XML says, not what DeckCraft plays

[eval/tools/pptclicks.py](eval/tools/pptclicks.py) plays each deck in
PowerPoint through AppleScript and counts the presses each slide takes;
`go to next slide` is a clicker press.

All 13 decks in [eval/deckcraft-builds.tsv](eval/deckcraft-builds.tsv) were
played except `keyframes`, which PowerPoint can't read. Each ran with 1.5
seconds between presses. The ones that disagreed were run again with 5
seconds.

A short pause can only *add* presses: a press during a transition, an
automatic effect or a timed advance just finishes it. So the 1.5-second
count is an upper bound, and the 5-second count is the one to compare.

| Slides | Result |
| --- | --- |
| 37 of #2's build slides where XML and DeckCraft agree (the 38th is in `keyframes`) | PowerPoint agrees on 36; the exception is `ae.ac.uaeu…GeomLec1` slide 14 |
| `ca.ubc…HowWeRefactor` slide 9 | XML 1, DeckCraft 4, **PowerPoint 1** |
| `ca.ubc…HowWeRefactor` slide 21 | XML 2, DeckCraft 1, **PowerPoint 2** |
| `ca.ubc…HowWeRefactor` slide 27 | XML 6, DeckCraft 18, **PowerPoint 6** |
| `customGeo` slide 18 | XML 5, DeckCraft 0, **PowerPoint 5** |

All four paragraph-build slides that #2 found wrong are wrong against
PowerPoint itself. PowerPoint plays exactly what the XML describes. The bug
is DeckCraft's (`crates/pptx/src/read/timing.rs`), not an artefact of
counting clicks from the XML.

Small notes from the run:

- **GeomLec1 slide 14:** PowerPoint takes 0 clicks where the XML counter and
  DeckCraft both count 1. The effect is a `nodePh="1"` placeholder node that
  ends as it begins, so PowerPoint never waits for a click. That is a second,
  minor DeckCraft discrepancy, and `xmlclicks.py` should skip such nodes too.
- **Timing artefacts that the 5-second runs settled:**
  - `45541_Header` slide 11 and `at.ecodesign…` slides 22, 24 and 25.
  - 2411 slide 2: its effect starts by itself after 5 seconds, so even a
    5-second press triggers it.
  - UBC: at 1.5 seconds most slides took one press too many; at 5 seconds
    every one matched the XML.
  - Six UBC slides with 0 clicks advance by themselves after 1 second and
    were passed during the 5-second pauses.
- **2411 slide 23** took 2 presses at 5 seconds after 1 at 1.5 seconds. That
  is unexplained; the lower count matches the XML.

### Visual check: PowerPoint sides with DeckCraft on text, with LibreOffice on charts

The 12 slides that scored worst against LibreOffice, plus 4 typical ones,
were shown in PowerPoint's slide show and captured. They were then compared
by eye with #2's DeckCraft and LibreOffice renders, at the same commit.

- `keyframes` slide 26 is out: PowerPoint can't read the file.
- UBC slide 3 is out: its slide show could not be captured.

That leaves 14 slides.

A plain RMSE against the captures is not usable for text. They are JPEGs of
the screen, so a one- or two-pixel offset alone scores 0.25 to 0.3 even
where the layouts match line for line.

| Slides | Closest to PowerPoint | What differs |
| --- | --- | --- |
| 5 charts | **LibreOffice** | DeckCraft leaves out a picture fill behind the plot, overlaps category labels instead of rotating them, drops data labels, draws a markers-only series as a line, and gives each bar its own colour |
| 6 text slides | **DeckCraft** | DeckCraft breaks every line where PowerPoint does. LibreOffice sets text narrower, wraps differently and shrinks two titles |
| 3 typical slides | all three match | |

This sharpens #2's judgement of the worst slides:

- **Charts:** #2 said "DeckCraft is worse", and PowerPoint confirms it.
  Chart rendering is ours to build anyway (T24).
- **Text:** #2 said "neither is right", but against PowerPoint DeckCraft's
  text layout is right and LibreOffice is the outlier. The `customGeo` title
  size is also confirmed: DeckCraft matches PowerPoint.

The fonts on this Mac include the ones PowerPoint for Mac installs. A church
laptop without them will still need the PS-4 substitution report.

### What this means for [ARCHITECTURE.md](ARCHITECTURE.md)

It confirms the plan: depend on `deckcraft-pptx`, `deckcraft-model` and
`deckcraft-anim`, pinned, with our own mapping into `ParsedDeck`.

The paragraph-build fold in `read/timing.rs` must be fixed upstream or in
our `[patch]` fork before T29's real-deck fidelity check. The fix:
- fold only runs of on-click effects on consecutive paragraphs;
- keep the rest as per-paragraph animations, which the model already
  supports;
- skip `nodePh` placeholder nodes.

The same fold also breaks Keynote exports. Keynote writes a paragraph build
as a background fade with paragraph 1, then one paragraph per click, and
DeckCraft plays that as a single click.

For T32, which plans PowerPoint-exported reference images:
- An unlicensed PowerPoint cannot export. A licensed copy is needed, or
  screen captures of the slide show, as here.
- PowerPoint's sandbox asks for file access per file outside its known
  folders. Keeping the corpus in `~/Downloads` avoided the prompts here.
- Some corpus decks trigger PowerPoint's own Repair prompt, which needs a
  person.

## Reproducing

The scripts are in [eval/tools](eval/tools). Clone DeckCraft and the POI
decks next to this repo, outside it, and treat the decks as untrusted:

```bash
git clone https://github.com/storytold/deckcraft.git ../deckcraft-eval/deckcraft
```

```bash
git -C ../deckcraft-eval/deckcraft checkout 22db926
```

```bash
cargo +1.95 build --release -p deckcraft-cli --manifest-path ../deckcraft-eval/deckcraft/Cargo.toml
```

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/apache/poi.git ../deckcraft-eval/poi
```

```bash
git -C ../deckcraft-eval/poi sparse-checkout set test-data/slideshow
```

Then:

- **Builds:** `cargo +1.95 run --release --manifest-path docs/eval/tools/probe/Cargo.toml -- DECKS/*.pptx`
  prints DeckCraft's clicks per slide; `python3 -I docs/eval/tools/xmlclicks.py DECKS/*.pptx`
  prints the clicks in the XML.
- **Visuals:** export the decks to PDF with
  `soffice --headless --convert-to pdf --outdir OUT/ref DECKS/*.pptx`, run
  `compare.sh DECKS OUT existing` to render and rasterise, then
  `rescore.sh OUT XMLCLICKS.tsv` to score visible slides only (LibreOffice
  leaves hidden slides out of the PDF).
- **PowerPoint click counts:** copy the decks to `~/Downloads` (PowerPoint's
  sandbox reads it without asking), then
  `python3 -I docs/eval/tools/pptclicks.py --delay 5 ~/Downloads/DECKS/*.pptx`.
  It drives PowerPoint for Mac through AppleScript and only plays decks, so
  an unlicensed PowerPoint works.
