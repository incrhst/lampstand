//! Layout: fonts, splitting text into slides, fitting text, and composing
//! the final `Scene` for each output. CPU only and deterministic, so its
//! output can be tested without a GPU.
//!
//! PRD: LV-3, LV-4, TS-2 to TS-4, TS-7 to TS-9, OB-1, PS-7.
//!
//! Text stack: cosmic-text (shaping with rustybuzz, font fallback). The
//! renderer uses glyphon over the same cosmic-text `FontSystem` and the
//! same bundled fonts, so a line that fits here fits on screen.

mod compose;
mod fit;
mod fonts;
mod paginate;

pub use compose::{compose, Composed, FitWarning, OutputSize};
pub use fit::{fit_text, Fitted};
pub use fonts::Fonts;
pub use paginate::{paginate_passage, paginate_song};
