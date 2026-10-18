//! Fitting text to a box, shrinking to the theme's minimum (T22).

use crate::Fonts;

#[derive(Debug, Clone, PartialEq)]
pub struct Fitted {
    /// Size used, in output pixels.
    pub size: f32,
    /// True when the text needed less than `min_size` to fit and was set
    /// at `min_size`, overflowing (TS-7). The editor and preview flag it.
    pub below_minimum: bool,
}

/// Largest size from `size` down to `min_size` (in 0.5 px steps) at which
/// the lines, wrapped to `width`, fit in `height`.
pub fn fit_text(
    lines: &[String],
    style: &lampstand_core::theme::TextStyle,
    size: f32,
    min_size: f32,
    width: f32,
    height: f32,
    fonts: &mut Fonts,
) -> Fitted {
    let _ = (lines, style, size, min_size, width, height, fonts);
    todo!("T22")
}
