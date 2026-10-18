//! What is on air: the contract between the live engine
//! (`lampstand-live`), which decides it, and the compositor
//! (`lampstand-layout`), which draws it for each output.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::deck::DeckId;
use crate::id::{BlobHash, Timestamp};
use crate::scene::Scene;
use crate::song::SongId;
use crate::theme::{Background, ThemeId};

/// One slide of text, already split: songs by the theme's line count,
/// scripture by what fits the primary output. Every output shows the same
/// `SlideText`, each fitting it to its own size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlideText {
    /// Section label ("V1") or reference ("John 3:16–17"), for the operator
    /// and stage display. Not projected.
    pub label: Option<String>,
    pub lines: Vec<String>,
    /// A smaller line at the foot: song copyright and CCLI number, or the
    /// scripture reference and translation.
    pub footer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Content {
    Lyrics {
        song: SongId,
        slide: SlideText,
    },
    Scripture {
        slide: SlideText,
    },
    Text {
        slide: SlideText,
    },
    /// A deck slide at a build step. `scene` is the slide in slide points,
    /// shared with the parsed deck. Step 0 is before any click.
    DeckSlide {
        deck: DeckId,
        index: usize,
        step: usize,
        scene: Arc<Scene>,
        notes: String,
    },
    Image {
        blob: BlobHash,
    },
    Video {
        blob: BlobHash,
        looped: bool,
    },
    Countdown {
        target: Timestamp,
        label: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Overlay {
    #[default]
    None,
    Black,
    Logo,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Program {
    pub background: Option<Background>,
    pub content: Option<Content>,
    /// Covers everything when set. Toggles that persist across slides.
    pub overlay: Overlay,
    /// The slide after this one, for the stage display.
    pub next: Option<Content>,
    pub theme: Option<ThemeId>,
}
