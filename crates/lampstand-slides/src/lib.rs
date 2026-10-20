//! Slide engine: parses PPTX (including Google Slides exports) into the
//! Lampstand scene format, with builds, transitions, media and notes.
//!
//! PRD: PS-1, PS-3, PS-4, PS-7, TS-4.
//!
//! The parser resolves everything PowerPoint resolves at render time:
//! master → layout → slide inheritance, placeholders, theme colours and
//! fonts, preset geometries (to paths), `normAutofit` font scaling, and
//! group transforms. The result needs no PPTX knowledge to draw.
//!
//! Scenes are in points (1/72 inch); a 16:9 deck is 960 × 540.

mod anim;
mod pptx;

use std::collections::BTreeMap;

use lampstand_core::id::BlobHash;
use lampstand_core::scene::{Animation, Scene, Transition};
use lampstand_core::FontLookup;

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedDeck {
    pub width: f32,
    pub height: f32,
    pub slides: Vec<DeckSlide>,
    /// Images, video and audio the slides refer to, by hash. The caller
    /// stores them in the blob store.
    pub media: BTreeMap<BlobHash, MediaPart>,
    pub report: FidelityReport,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaPart {
    pub content_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeckSlide {
    /// The slide as it arrives, before `entry` plays: nodes that an
    /// entrance build reveals have `visible: false`.
    pub scene: Scene,
    /// Animations that play by themselves as the slide arrives (PowerPoint
    /// effects that start "with" or "after previous" before any click).
    /// Empty for most slides.
    pub entry: BuildStep,
    /// One entry per click, after `entry`.
    pub builds: Vec<BuildStep>,
    /// How this slide arrives.
    pub transition: Transition,
    pub notes: String,
    /// Hidden in the source deck. Skipped when playing.
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct BuildStep {
    /// Each starts `delay_ms` after the click. A text paragraph that builds
    /// on its own is its own node (a copy of its block showing only that
    /// paragraph), so it can be targeted like a shape.
    pub animations: Vec<Animation>,
}

impl DeckSlide {
    /// The slide as it looks after `entry` and `step` clicks with every
    /// animation finished: for jumping straight to a step, for thumbnails
    /// and recomposing, and for crash recovery. Steps past the last are
    /// the last.
    pub fn scene_at_step(&self, step: usize) -> Scene {
        let clicks = self.builds.iter().take(step);
        anim::after(
            &self.scene,
            std::iter::once(&self.entry)
                .chain(clicks)
                .map(|b| b.animations.as_slice()),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FidelityReport {
    pub issues: Vec<Issue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    /// 0-based slide index; `None` for deck-wide issues.
    pub slide: Option<usize>,
    pub kind: IssueKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IssueKind {
    MissingFont {
        requested: String,
        substitute: Option<String>,
    },
    /// An animation or transition shown as the closest supported effect.
    ApproximatedEffect {
        name: String,
        used: String,
    },
    UnsupportedEffect {
        name: String,
    },
    /// SmartArt, charts, 3D models, equations without a fallback image.
    UnsupportedContent {
        what: String,
    },
    /// Linked (not embedded) media that could not be fetched.
    MissingLinkedMedia {
        target: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    NotAPresentation,
    /// Password-protected PPTX (an encrypted OLE container).
    Encrypted,
    Corrupt(String),
    /// The deck would cost too much memory to parse: `found` of `what`
    /// ("unpacked bytes", "XML elements in ppt/slides/slide3.xml"...) where
    /// at most `limit` are allowed. Real decks stay far below the limits;
    /// embedded video this large should be linked or added as a media item.
    TooLarge {
        what: String,
        found: u64,
        limit: u64,
    },
}

/// Parses a PPTX file (PowerPoint, Keynote or Google Slides export).
///
/// Built on DeckCraft's reader (`docs/decisions/0001-pptx-reader.md`); see
/// `pptx` for how its model maps onto scenes. Fonts the deck asks for
/// that `fonts` lacks, shapes and objects drawn approximately or not at
/// all, effects and transitions shown approximately or left out, and
/// linked media are in `ParsedDeck::report`. Builds and transitions come
/// from `p:timing` and `p:transition` (see `pptx::anim`).
pub fn import_pptx(bytes: &[u8], fonts: &dyn FontLookup) -> Result<ParsedDeck, ImportError> {
    pptx::import(bytes, fonts)
}

/// `import_pptx` with the import budget (the parse estimate plus every
/// entry's inflated bytes twice) limited to `memory` bytes instead of its
/// 1 GiB. For `tests/memory.rs`, which
/// measures peak allocation against the budget at a size a debug build
/// parses quickly; the peak scales with the budget.
#[doc(hidden)]
pub fn import_pptx_with_budget(
    bytes: &[u8],
    fonts: &dyn FontLookup,
    memory: u64,
) -> Result<ParsedDeck, ImportError> {
    pptx::import_with(bytes, fonts, memory)
}
