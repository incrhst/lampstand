//! The font set shared by layout and render (T20).

use lampstand_core::{FontLookup, FontMatch};

/// Bundled fonts (Noto Sans and Serif, plus the metric-compatible
/// substitutes Carlito, Caladea, Arimo, Tinos and Cousine for Calibri,
/// Cambria, Arial, Times New Roman and Courier New) and the system fonts.
pub struct Fonts {
    _private: (),
}

impl Fonts {
    /// Loads bundled and system fonts. Takes ~100 ms; do it once.
    pub fn load() -> Self {
        todo!("T20")
    }
}

impl FontLookup for Fonts {
    fn lookup(&self, family: &str) -> FontMatch {
        let _ = family;
        todo!("T20")
    }
}
