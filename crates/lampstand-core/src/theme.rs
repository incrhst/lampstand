//! Themes: how text items (lyrics, scripture, announcements) look.
//!
//! PRD: LV-4, TS-7.

use serde::{Deserialize, Serialize};

use crate::geom::{Color, Rect};
use crate::id::{BlobHash, Id};
use crate::scene::{HAlign, Outline, Shadow, Transition, VAlign};

pub type ThemeId = Id<Theme>;

/// Theme sizes are in pixels on a 1080-pixel-tall output and scale with
/// the output's height, so one theme works on every screen (TS-9).
pub const REFERENCE_HEIGHT: f32 = 1080.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub struct Theme {
    pub id: ThemeId,
    pub name: String,
    pub text: TextStyle,
    pub background: Background,
    /// Where text goes, as fractions of the output's safe area. A full
    /// screen theme is (0, 0, 1, 1); a lower third is (0, 0.7, 1, 0.3).
    pub text_area: Rect,
    /// Lines of lyrics per slide when songs are split into slides.
    pub max_lines: u8,
    pub transition: Transition,
    /// Show the song's copyright and CCLI line at the foot of the first
    /// slide (most CCLI licences require it).
    pub show_copyright: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub struct TextStyle {
    pub font_family: String,
    /// Preferred size; text shrinks from here to fit.
    pub size: f32,
    /// Smallest size text may shrink to. Below this the slide is flagged
    /// before the service (TS-7).
    pub min_size: f32,
    pub weight: u16,
    pub italic: bool,
    pub color: Color,
    pub align: HAlign,
    pub valign: VAlign,
    pub line_spacing: f32,
    pub uppercase: bool,
    pub shadow: Option<Shadow>,
    pub outline: Option<Outline>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "automerge",
    derive(autosurgeon::Hydrate, autosurgeon::Reconcile)
)]
pub enum Background {
    /// Nothing: transparent on the stream output, black elsewhere.
    None,
    Color(Color),
    Image(BlobHash),
    Video(BlobHash),
}
