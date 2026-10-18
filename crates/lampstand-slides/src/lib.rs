//! Slide engine: parses PPTX (including Google Slides exports) into the
//! Lampstand scene format and plays builds, transitions and media.
//!
//! PRD: PS-1 to PS-14. v0.1 design: `docs/v0.1.md`.
//!
//! v0.1 shows a deck as a series of still images, one per click step. Step 0
//! is the slide as it first appears (any animation that starts on its own has
//! played); step *n* is the slide after the *n*th click. Transitions and
//! animation motion between steps come later.
//!
//! The reader and the animation timeline are DeckCraft's (`deckcraft-pptx`,
//! `deckcraft-anim`); see `docs/deckcraft-evaluation.md`.

use std::path::Path;

/// What the show needs to know about a deck without drawing it.
#[derive(Debug, Clone, PartialEq)]
pub struct DeckInfo {
    /// Width over height of the slide size, e.g. 16/9.
    pub aspect: f32,
    /// Every slide in file order, hidden ones included.
    pub slides: Vec<SlideInfo>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SlideInfo {
    /// The title placeholder's text, if the slide has one.
    pub title: Option<String>,
    /// Hidden slides are skipped in the show.
    pub hidden: bool,
    /// Click steps after the first view: 0 for a slide with no click builds.
    pub clicks: usize,
    /// Speaker notes as plain text.
    pub notes: Option<String>,
    /// All the slide's text, for search (PS-11).
    pub text: String,
}

/// An RGBA8 image, row-major, not premultiplied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(Debug)]
pub enum DeckError {
    /// The file is not a presentation we can read (corrupt, encrypted, not PPTX).
    Unreadable(String),
    Io(std::io::Error),
    /// A slide index or step past the end.
    OutOfRange,
}

impl std::fmt::Display for DeckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeckError::Unreadable(why) => write!(f, "not a presentation Lampstand can read: {why}"),
            DeckError::Io(e) => write!(f, "{e}"),
            DeckError::OutOfRange => write!(f, "no such slide or step"),
        }
    }
}

impl std::error::Error for DeckError {}

impl From<std::io::Error> for DeckError {
    fn from(e: std::io::Error) -> Self {
        DeckError::Io(e)
    }
}

/// An opened deck. Cheap to query; rendering a step takes tens of milliseconds,
/// so callers cache the images.
pub struct Deck {
    info: DeckInfo,
}

impl Deck {
    pub fn open(path: &Path) -> Result<Deck, DeckError> {
        Deck::from_bytes(&std::fs::read(path)?)
    }

    pub fn from_bytes(_bytes: &[u8]) -> Result<Deck, DeckError> {
        Err(DeckError::Unreadable("PPTX import is not built yet".into()))
    }

    pub fn info(&self) -> &DeckInfo {
        &self.info
    }

    /// Draw `slide` (file order, 0-based) after `step` clicks, `width` pixels wide;
    /// the height follows the deck's aspect ratio.
    pub fn render(&self, slide: usize, step: usize, _width: u32) -> Result<Rgba, DeckError> {
        let s = self.info.slides.get(slide).ok_or(DeckError::OutOfRange)?;
        if step > s.clicks {
            return Err(DeckError::OutOfRange);
        }
        Err(DeckError::Unreadable("PPTX rendering is not built yet".into()))
    }
}
