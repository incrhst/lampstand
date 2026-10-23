//! A church's brand: its name, logo, colours and font. Lampstand turns it
//! into a starting theme (`lampstand_library::theme_from_brand`) and shows
//! the logo when the Logo overlay is on.
//!
//! Like outputs, the brand belongs to one machine's settings and is not
//! synced; the theme made from it is a library document and does sync.

use serde::{Deserialize, Serialize};

use crate::geom::Color;
use crate::id::BlobHash;

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Brand {
    /// The church's name; the theme made from it is named after it.
    pub name: String,
    /// A picture in the blob store, shown by the Logo overlay.
    pub logo: Option<BlobHash>,
    /// The main colour: the background of the theme and of the logo screen.
    pub primary: Color,
    /// The colour of lyrics and scripture. `None` picks black or white,
    /// whichever reads better on `primary`.
    pub text: Option<Color>,
    /// A third colour, for the ring drawn when there is no logo yet.
    pub accent: Color,
    /// A font family name. Empty means the starter theme's.
    pub font_family: String,
}

impl Brand {
    /// The colour lyrics and scripture are drawn in.
    pub fn text_color(&self) -> Color {
        self.text.unwrap_or_else(|| readable_on(self.primary))
    }
}

/// Black or white, whichever has more contrast against `background`.
pub fn readable_on(background: Color) -> Color {
    if luminance(background) > 0.4 {
        Color::BLACK
    } else {
        Color::WHITE
    }
}

/// Relative luminance (WCAG), 0 for black to 1 for white.
pub fn luminance(c: Color) -> f32 {
    fn channel(v: u8) -> f32 {
        let v = f32::from(v) / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
}

/// The contrast ratio between two colours, 1 to 21 (WCAG).
pub fn contrast(a: Color, b: Color) -> f32 {
    let (hi, lo) = {
        let (x, y) = (luminance(a), luminance(b));
        if x > y {
            (x, y)
        } else {
            (y, x)
        }
    };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_readable_on_dark_and_light() {
        assert_eq!(readable_on(Color::rgb(20, 30, 90)), Color::WHITE);
        assert_eq!(readable_on(Color::rgb(250, 240, 200)), Color::BLACK);
    }

    #[test]
    fn contrast_of_black_on_white_is_21() {
        assert!((contrast(Color::BLACK, Color::WHITE) - 21.0).abs() < 0.01);
    }
}
