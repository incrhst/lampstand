//! Fitting text to a box, shrinking to the theme's minimum (T22).

use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Style, Weight, Wrap};
use lampstand_core::theme::TextStyle;
use lampstand_core::{FontLookup, FontMatch};

use crate::Fonts;

/// Granularity of the search, in output pixels.
const STEP: f32 = 0.5;

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
///
/// Each entry of `lines` is a paragraph that wraps on its own. Line height
/// is `size * style.line_spacing`. The candidate sizes are `size`,
/// `size - 0.5`, … down to `min_size`, which is always the last candidate
/// even when it is off the half-pixel grid. If even `min_size` overflows,
/// the result is `min_size` with `below_minimum` set. Empty input fits at
/// `size`.
pub fn fit_text(
    lines: &[String],
    style: &TextStyle,
    size: f32,
    min_size: f32,
    width: f32,
    height: f32,
    fonts: &mut Fonts,
) -> Fitted {
    let min_size = min_size.min(size);
    if lines.is_empty() {
        return Fitted {
            size,
            below_minimum: false,
        };
    }
    let family = match fonts.lookup(&style.font_family) {
        FontMatch::Substitute(to) => to,
        _ => style.font_family.clone(),
    };
    let text = lines.join("\n");
    let text = if style.uppercase {
        text.to_uppercase()
    } else {
        text
    };

    // Candidate k is `size - k * STEP`, with the last one clamped to
    // `min_size`. Fitting is monotone in size, so binary search for the
    // smallest k that fits.
    let last = ((size - min_size) / STEP).ceil() as u32;
    let candidate = |k: u32| (size - k as f32 * STEP).max(min_size);
    let fits = |k: u32, fonts: &mut Fonts| {
        text_height(&text, &family, style, candidate(k), width, fonts) <= height
    };
    if !fits(last, fonts) {
        return Fitted {
            size: min_size,
            below_minimum: true,
        };
    }
    let (mut lo, mut hi) = (0, last);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if fits(mid, fonts) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    Fitted {
        size: candidate(lo),
        below_minimum: false,
    }
}

/// Height of `text` wrapped to `width` at `size`: wrapped lines times the
/// line height. Blank lines count as one line each.
fn text_height(
    text: &str,
    family: &str,
    style: &TextStyle,
    size: f32,
    width: f32,
    fonts: &mut Fonts,
) -> f32 {
    let line_height = size * style.line_spacing;
    let system = fonts.system();
    let mut buffer = Buffer::new(system, Metrics::new(size, line_height));
    buffer.set_wrap(Wrap::WordOrGlyph);
    buffer.set_size(Some(width), None);
    let attrs = Attrs::new()
        .family(Family::Name(family))
        .weight(Weight(style.weight))
        .style(if style.italic {
            Style::Italic
        } else {
            Style::Normal
        });
    buffer.set_text(text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(system, false);
    buffer.layout_runs().count() as f32 * line_height
}

#[cfg(test)]
mod tests {
    use super::*;
    use lampstand_core::geom::Color;
    use lampstand_core::scene::{HAlign, VAlign};

    fn style() -> TextStyle {
        TextStyle {
            font_family: "Noto Sans".into(),
            size: 60.0,
            min_size: 20.0,
            weight: 400,
            italic: false,
            color: Color::rgb(255, 255, 255),
            align: HAlign::Center,
            valign: VAlign::Middle,
            line_spacing: 1.2,
            uppercase: false,
            shadow: None,
            outline: None,
        }
    }

    fn lines(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| s.to_string()).collect()
    }

    const VERSE: &str = "The Lord is my shepherd; I shall not want. He maketh me to lie down in green pastures: he leadeth me beside the still waters.";

    #[test]
    fn fits_at_preferred_size() {
        let mut fonts = Fonts::bundled_only();
        let got = fit_text(
            &lines(&["Amazing grace"]),
            &style(),
            40.0,
            20.0,
            1000.0,
            200.0,
            &mut fonts,
        );
        assert_eq!(
            got,
            Fitted {
                size: 40.0,
                below_minimum: false
            }
        );
    }

    #[test]
    fn shrinks_to_fit() {
        let mut fonts = Fonts::bundled_only();
        let l = lines(&[VERSE]);
        let got = fit_text(&l, &style(), 60.0, 12.0, 600.0, 200.0, &mut fonts);
        assert!(got.size < 60.0 && got.size >= 12.0, "{got:?}");
        assert!(!got.below_minimum);
        assert_eq!(got.size % 0.5, 0.0);
    }

    #[test]
    fn below_minimum_is_flagged() {
        let mut fonts = Fonts::bundled_only();
        let l = lines(&[VERSE, VERSE, VERSE]);
        let got = fit_text(&l, &style(), 60.0, 30.0, 400.0, 100.0, &mut fonts);
        assert_eq!(
            got,
            Fitted {
                size: 30.0,
                below_minimum: true
            }
        );
    }

    #[test]
    fn long_lines_wrap_to_width() {
        let mut fonts = Fonts::bundled_only();
        let l = lines(&[VERSE]);
        let height = 150.0;
        let wide = fit_text(&l, &style(), 40.0, 10.0, 3000.0, height, &mut fonts);
        let narrow = fit_text(&l, &style(), 40.0, 10.0, 500.0, height, &mut fonts);
        assert_eq!(wide.size, 40.0);
        assert!(narrow.size < wide.size, "{narrow:?}");
    }

    #[test]
    fn unbreakable_word_wraps_by_glyph() {
        let mut fonts = Fonts::bundled_only();
        let l = lines(&["Supercalifragilisticexpialidocious"]);
        let got = fit_text(&l, &style(), 40.0, 10.0, 150.0, 400.0, &mut fonts);
        assert!(!got.below_minimum);
    }

    #[test]
    fn empty_input_fits_at_preferred_size() {
        let mut fonts = Fonts::bundled_only();
        let got = fit_text(&[], &style(), 44.0, 20.0, 100.0, 0.0, &mut fonts);
        assert_eq!(
            got,
            Fitted {
                size: 44.0,
                below_minimum: false
            }
        );
    }

    #[test]
    fn blank_lines_take_space() {
        let mut fonts = Fonts::bundled_only();
        let tight = fit_text(
            &lines(&["a", "b"]),
            &style(),
            40.0,
            10.0,
            500.0,
            100.0,
            &mut fonts,
        );
        let spaced = fit_text(
            &lines(&["a", "", "b"]),
            &style(),
            40.0,
            10.0,
            500.0,
            100.0,
            &mut fonts,
        );
        assert!(spaced.size < tight.size);
    }

    #[test]
    fn style_affects_the_result() {
        let mut fonts = Fonts::bundled_only();
        let l = lines(&["Holy holy holy Lord God almighty early in the morning"]);
        let base = fit_text(&l, &style(), 60.0, 10.0, 500.0, 120.0, &mut fonts).size;
        let upper = TextStyle {
            uppercase: true,
            ..style()
        };
        let loose = TextStyle {
            line_spacing: 2.0,
            ..style()
        };
        let bold = TextStyle {
            weight: 700,
            ..style()
        };
        assert!(fit_text(&l, &upper, 60.0, 10.0, 500.0, 120.0, &mut fonts).size < base);
        assert!(fit_text(&l, &loose, 60.0, 10.0, 500.0, 120.0, &mut fonts).size < base);
        assert!(fit_text(&l, &bold, 60.0, 10.0, 500.0, 120.0, &mut fonts).size <= base);
    }

    #[test]
    fn office_family_uses_its_substitute() {
        let mut fonts = Fonts::bundled_only();
        let calibri = TextStyle {
            font_family: "Calibri".into(),
            ..style()
        };
        let carlito = TextStyle {
            font_family: "Carlito".into(),
            ..style()
        };
        let l = lines(&[VERSE]);
        assert_eq!(
            fit_text(&l, &calibri, 60.0, 10.0, 600.0, 200.0, &mut fonts),
            fit_text(&l, &carlito, 60.0, 10.0, 600.0, 200.0, &mut fonts)
        );
    }

    #[test]
    fn chosen_size_fits_and_next_step_up_does_not() {
        let mut fonts = Fonts::bundled_only();
        let s = style();
        let (width, height) = (600.0, 200.0);
        let text = VERSE.to_string();
        let got = fit_text(&lines(&[VERSE]), &s, 60.0, 12.0, width, height, &mut fonts);
        assert!(!got.below_minimum && got.size < 60.0);

        // Measured directly with cosmic-text, not through `text_height`.
        let mut direct = |size: f32| {
            let system = fonts.system();
            let lh = size * s.line_spacing;
            let mut b = Buffer::new(system, Metrics::new(size, lh));
            b.set_wrap(Wrap::WordOrGlyph);
            b.set_size(Some(width), None);
            b.set_text(
                &text,
                &Attrs::new().family(Family::Name("Noto Sans")),
                Shaping::Advanced,
                None,
            );
            b.shape_until_scroll(system, false);
            b.layout_runs().map(|r| r.line_height).sum::<f32>()
        };
        assert!(direct(got.size) <= height, "{} too tall", got.size);
        assert!(
            direct(got.size + 0.5) > height,
            "{} would fit",
            got.size + 0.5
        );
    }
}
