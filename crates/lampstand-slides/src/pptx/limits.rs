//! Bounds for numbers read from a deck.
//!
//! Decks come from guest speakers, so every number from the XML is
//! untrusted. DeckCraft passes most of them through as read (EMU → points,
//! unclamped), and a few feed allocation or repetition here (a bullet's
//! padding, a dash pattern), where an absurd value would exhaust memory.
//! Each value is clamped to the range its DrawingML type allows (ECMA-376
//! part 1, §20.1.10 and §21.1.10) before it is used. A value outside that
//! range is not a valid file, so clamping changes nothing for real decks.

/// `ST_TextMargin` (`marL`, `marR`): 0 to 51,206,400 EMU.
pub(crate) const TEXT_MARGIN: f32 = 4032.0;
/// `ST_TextIndent` (`indent`): ±51,206,400 EMU.
pub(crate) const TEXT_INDENT: f32 = 4032.0;
/// `ST_TextFontSize`: 1 to 4,000 pt.
pub(crate) const FONT_SIZE: (f32, f32) = (1.0, 4000.0);
/// `ST_TextSpacingPoint`: 0 to 1,584 pt.
pub(crate) const SPACING_POINTS: f32 = 1584.0;
/// `ST_TextSpacingPercentOrPercentString`: 0 to 13,200%.
pub(crate) const SPACING_PERCENT: f32 = 132.0;
/// `ST_LineWidth`: 0 to 20,116,800 EMU.
pub(crate) const LINE_WIDTH: f32 = 1584.0;
/// Insets, shadow distances and blurs: no larger than the largest slide.
pub(crate) const DISTANCE: f32 = 4032.0;
/// `ST_SlideSizeCoordinate`: 914,400 to 51,206,400 EMU.
pub(crate) const SLIDE_SIDE: (f32, f32) = (72.0, 4032.0);
/// A shape's width or height on the slide. The schema allows far more
/// (`ST_PositiveCoordinate`), but nothing bigger than this can be seen.
pub(crate) const SHAPE_SIDE: f64 = 100_000.0;
/// Dash and gap lengths, in multiples of the line width.
pub(crate) const DASH: (f32, f32) = (0.1, 1000.0);
/// Dash and gap entries in one pattern.
pub(crate) const DASH_ENTRIES: usize = 32;
/// Thin spaces padding a folded bullet (see `text::hang_bullet`).
pub(crate) const BULLET_PAD: usize = 64;

/// `v` within `lo..=hi`; `fallback` when it is not a finite number.
pub(crate) fn clamp(v: f64, lo: f32, hi: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        (v as f32).clamp(lo, hi)
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_and_replaces_non_numbers() {
        assert_eq!(clamp(9.0e18, 0.0, TEXT_MARGIN, 0.0), TEXT_MARGIN);
        assert_eq!(clamp(-9.0e18, -TEXT_INDENT, TEXT_INDENT, 0.0), -TEXT_INDENT);
        assert_eq!(clamp(f64::NAN, 0.0, 1.0, 0.5), 0.5);
        assert_eq!(clamp(12.5, 0.0, 100.0, 0.0), 12.5);
    }
}
