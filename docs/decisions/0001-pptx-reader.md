# 0001. PPTX reader: borrow from DeckCraft, own the mapping

Status: proposed (T28 spike). Date: 2026-10-18. Feeds T29.

## Recommendation

**Borrow parts.** Use DeckCraft's `deckcraft-pptx` reader (with its `model`, `geom` and `color` crates) as the base for the XML layer, pinned to a commit. Write our own mapping from its `Presentation` to `ParsedDeck` inside `lampstand-slides`, so nothing DeckCraft leaks past that crate's API. Do not adopt the rest of DeckCraft (engine, renderer, UI, codecs, writer), and do not track its `main`.

The reader parsed all five test decks correctly, the licence is clean for a GPL-3.0 project, and it depends only on `quick-xml`, `zip`, `serde` and `kurbo`. The risk is the project's age. It is eleven days old and moves hundreds of commits a week. A pinned copy gets us the working parser and shields us from that churn.

Fall back to writing our own parser (the ARCHITECTURE.md default) if T29 finds the pinned reader loses fidelity on real decks. The `ParsedDeck` boundary keeps that switch inside `slides`.

## What was checked

- Source: `github.com/storytold/deckcraft` at `cb78440a1618e6a2c1df55dd7bff74719685518c` (2026-10-18), the repo the ArtCraft site links to. Cloned into an empty scratch directory outside this repo. A search shows many same-named forks (a dozen turned up in one search) under other accounts; none of those were used, and none should be.
- Built a 40-line harness crate outside the repo that depends on `deckcraft-pptx` and `deckcraft-model` by path, calls `import_checked` and prints the result. I did not build or run DeckCraft's app, CLI or MCP server, and I did not run its test suite. The pptx, model, geom and color crates have no `build.rs`.
- Generated five decks with python-pptx 1.0.2 (plus hand-written timing XML). Python-pptx has no animation API, so deck 5 injects raw `p:timing` XML.

## 1. Licence, file by file

| Part | Licence | Notes |
| --- | --- | --- |
| All Rust source, build files, docs, packaging scripts | MIT OR Apache-2.0 (workspace `license` field, `LICENSE-MIT`, `LICENSE-APACHE`) | No per-file SPDX headers; the licence is set once at the workspace level. Both options are compatible with GPL-3.0. Apache-2.0 is one-way compatible with GPLv3, and MIT is too. |
| `docs/brand/*` (ArtCraft logos and marks) | ArtCraft trademark, not open source (`docs/brand/LICENSE-brand.txt`) | Never copy these. They are outside the code licence. |
| Test fixtures (`crates/media/tests/fixtures/*`, `colr-squares.ttf`, `legacy-*.ttf`) | CC0-1.0 | Synthesised by the authors. |
| `assets/app-icon/*`, DMG background, README screenshots | MIT OR Apache-2.0 | Contributor-original. |
| `crates/engine/src/icon_data.rs` (Lucide paths) | ISC; MIT for the Feather-derived icons | Notice text is in `NOTICE`. |
| Fonts (via `epaint_default_fonts`, and `craft-fonts` at build time) | Ubuntu Font Licence 1.0; MIT, Bitstream Vera, OFL-1.1; OFL-1.1 for the craft-fonts families | Dependency or build-time only, outside the crates we want. |
| Dependencies | `craft-pdf` and `craft-libs` come from the same owner via git (`storytold/craft-libs`) | Used by the PDF and codec crates, not by `pptx`, `model`, `geom` or `color`. |

Provenance: `AGENTS.md` sets a clean-room policy (no Microsoft assets, written from ECMA-376), and `ATTRIBUTION.md` lists every non-code asset with author and licence. `cargo xtask assets` enforces that table. The code itself has no per-file provenance, and the repo's own roadmap talks about "Opus 5.5 agent hours", so the code looks largely agent-written. That does not affect the licence, but it is a reason to read what we vendor.

**Obligations if we vendor:** keep `LICENSE-MIT` or `LICENSE-APACHE`, the `NOTICE` text for the copied crates, and a note of the upstream commit. Pick one of the two licences and record which.

## 2. Reader quality on five synthetic decks

All five imported with no skipped slides and no errors, in 3 to 5 ms each (release build, Apple silicon). The deck files and generator stay in the scratchpad, not in the repo. T29 should regenerate them as committed fixtures.

| Deck | Content | Result |
| --- | --- | --- |
| d1 master and placeholders | Custom master (background, moved title), title and content layouts, a hidden blank slide, notes on slide 2 | Pass. Placeholders come through as `Title`, `Obj`, `CtrTitle` and `SubTitle` with `xfrm: None` (inherited). Hidden flag, layout ids and notes text were read correctly. |
| d2 groups and shapes | Group containing a nested group, outer group rescaled so child space differs from its box; star; rotated cloud | Pass. Groups keep both their own box and the child coordinate space (`Group { children, child }`), and rotation is read (15°). Flattening to slide coordinates is left to us. |
| d3 table and autofit | 4×3 table with a merged header, text box with `normAutofit fontScale=62.5%` | Pass. `gridSpan` and `hMerge` are set on the merged cells. Autofit arrives as `{kind: shrink, font_scale: 0.625}`. |
| d4 images | Two cropped and plain PNGs; a slide with a solid colour background | Pass. Three media items with content types, crop as `[0.25, 0.1, 0, 0]`, background as a solid fill. |
| d5 animations and transitions | Two click-triggered entrance fades (one on a body placeholder, one on a shape); fade transition; push transition with 3 s auto-advance; notes | Pass. Two `Entrance/fade/OnClick` animations in order, transition kind `fade` and `push` with `advance_after_ms = 3000`. |

What was not tested, and should be before T29 commits to the approach:

- Visual fidelity. I compared parsed values only, not rendered pixels. The T32 SSIM corpus is the real test.
- Paragraph-level builds, "with previous" and "after previous" timing, emphasis and exit effects, motion paths, embedded video and audio, linked media, charts and SmartArt fallbacks, and real-world decks (Google Slides exports, corporate templates).
- Robustness: no fuzzing or malformed or encrypted input was tried. The crate has `forbid(unsafe_code)` and denies `unwrap`, `expect` and `panic` in lints, which is a good sign.

Upstream says the same about itself: `docs/gaps.md` lists "PPTX corpus is only decks we author (no PowerPoint reference renders)" as its top file-format gap, and says behaviour on real-world decks is mostly unknown. Treat it as untested on real decks, same as our parser would be.

## 3. Mapping onto `ParsedDeck`

The reader's output is an editor document model (`Presentation`, `Slide`, `Shape`, `Animation`, serde-serialisable). It is **unresolved**: placeholders inherit position, fill and text style from layout and master at draw time, via `deckcraft_model::resolve` (`xfrm`, `fill`, `line`, `body`, `run`, `background`, `show_master_shapes`, with a `Ctx` built from the presentation).

| `ParsedDeck` need | DeckCraft provides | Work on our side |
| --- | --- | --- |
| `width`, `height` in points | `slide_size` in points (959.98 × 540 for 13.333 in; 960 × 540 after rounding) | Round to a sensible value. |
| `DeckSlide.scene` with master → layout → slide resolved | Raw shapes plus `resolve::*` helpers in `model` | Call the resolve helpers per shape and emit our `NodeKind`s. This is the largest piece. |
| Group transforms | Group box and child space | Flatten with our own transform maths. |
| Preset geometry to paths | `Geom` enum; `geom::preset` has 2.4k lines of preset shapes on `kurbo` | Reuse `geom::preset` as is, or port it. |
| `normAutofit` scaling | `Autofit::Shrink { font_scale, line_reduction }` | Feed to `fit_text`. |
| Tables | Cells with spans and borders | Map to our table nodes. |
| Images, `media` by hash | Media items with bytes and content type | Hash with BLAKE3 and fill `MediaPart`. |
| `builds: Vec<BuildStep>` | A flat `animations` list with `start` (`OnClick`, and so on), `class`, `effect`, `shape`, `paragraph` | Group into click steps, set `visible: false` for entrance targets, map effect ids to our `Animation`. Effects we cannot show go into the `FidelityReport`. |
| `Transition` | `kind`, `option`, `duration_ms`, `advance_after_ms` | Direct mapping. |
| `notes`, `hidden` | Both present | Direct. |
| `FidelityReport` | `import_checked` returns skipped slide part names; unmodelled XML is kept as `raw_ext`; `Opaque { label, preview }` marks SmartArt, OLE and similar | Derive `UnsupportedContent` and missing-font issues ourselves. |
| `ImportError::Encrypted` | `is_encrypted`, `sniff` | Direct. |

The model fits with a thin conversion, not a type-for-type match. The conversion drops everything the editor keeps for round-tripping (`raw_ext`, tags, guides, comments, the writer). DeckCraft's own layout, text shaping and renderer must not be used: ours are T20, T22 and T24.

## 4. Upstream activity

- Created 2026-10-07; last push 2026-10-18 (the day of this spike). 956 commits in 11 days, 874 of them in the last week, from 51 contributors. Mostly merged community pull requests, some of them agent-written.
- Version 0.8.0 with tags v0.4.0 to v0.8.0 inside eleven days. Self-described stage: alpha ("~55% ready for real work"). 3,763 stars, 2,131 forks, 385 open issues.
- Requires Rust 1.95 and edition 2024 (`rust-version = "1.95"`); the local toolchain here is 1.99.1, which builds it. This is a floor we must accept for the whole workspace if the crate is a dependency, so check it against T58 (CI).
- The pace is a hazard for tracking `main` and a benefit for finding bugs quickly. A pin avoids the first. The repo is controlled by a small team (ArtCraft / storytold), so there is a bus-factor and direction risk, and a real chance the API we depend on changes or the project stalls. Vendoring removes that dependency risk.

## Options considered

| Option | For | Against |
| --- | --- | --- |
| **Adopt** `deckcraft-pptx` as a dependency tracking upstream | Least work now | Alpha, daily API churn, 1.95 MSRV, editor model leaks into our design. |
| **Borrow** a pinned copy of the read side, own the mapping (recommended) | Working, tested XML layer and preset geometry on day one; licence is clean; fits `ParsedDeck` behind our boundary | We maintain a vendored copy; need to read it carefully; resolve helpers must be wired up by hand. |
| **Write our own** on `zip` and `quick-xml` | Full control, no provenance questions | Many weeks of OOXML detail (themes, inheritance, timing trees, tables, autofit) that DeckCraft already covers. |

## Follow-ups for T29

1. Vendor (or pin by git `rev`) `crates/{pptx,model,geom,color}` at `cb78440`, keeping licences and `NOTICE`. Drop the writer (`pptx/src/write`) to cut size. The reader side is about 7.3k lines of the crate's 19k.
2. Commit the five decks (with their python-pptx generator) as fixtures under `slides`, and add real or Google Slides exports for the T32 corpus.
3. Check the pinned reader against click-sequence timing (with/after previous, paragraph builds), video and audio, and a Google Slides export before building the mapping on it. If it fails on those, switch to our own parser.
4. Confirm the Rust 1.95 floor with T58.
