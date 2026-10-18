//! Shaping text blocks with cosmic-text, the same way layout measures them.
//!
//! The rules here must match `lampstand_layout::fit_text`, or a line that
//! fits in layout could wrap differently on screen:
//!
//! - families go through `Fonts`' substitutions (Calibri → Carlito...);
//! - line height is `size * line_spacing`;
//! - wrapping is `Wrap::WordOrGlyph` at the paragraph's width;
//! - shaping is `Shaping::Advanced`, hinting is off (the default).
//!
//! Shaping happens in scene units; glyphon rasterises at the output scale.
//! Each paragraph is its own buffer, because alignment, indent and spacing
//! are per paragraph. The paragraph's width is the block's width minus its
//! indent. A bullet hangs in the indent: it starts at the left edge of the
//! block and sits on the first line's baseline, in the first run's style.

use cosmic_text::{
    Align, Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, UnderlineStyle, Weight, Wrap,
};
use lampstand_core::geom::{Color, Rect};
use lampstand_core::scene::{HAlign, Paragraph, TextBlock, TextRun, VAlign};
use lampstand_core::{FontLookup, FontMatch};
use lampstand_layout::Fonts;

/// One laid-out paragraph or bullet: a shaped buffer and where its top-left
/// sits in the block's local space.
pub(crate) struct Placed {
    pub buffer: Buffer,
    pub x: f32,
    pub y: f32,
}

pub(crate) struct LaidOut {
    pub pieces: Vec<Placed>,
    /// Everything the glyphs could cover, in local space: the block's rect
    /// and any overflow.
    pub bounds: Rect,
}

pub(crate) fn lay_out(fonts: &mut Fonts, block: &TextBlock) -> LaidOut {
    struct Para {
        text: Buffer,
        bullet: Option<Buffer>,
        x: f32,
        top: f32,
    }
    let mut paras = Vec::new();
    let mut y = 0.0;
    for p in &block.paragraphs {
        y += p.space_before;
        let width = (block.rect.width - p.indent).max(0.0);
        let Some(text) = shape(fonts, p, &p.runs, Some(width), align(p.align)) else {
            y += p.space_after;
            continue;
        };
        let height: f32 = text.layout_runs().map(|run| run.line_height).sum();
        let bullet = match (&p.bullet, p.runs.first()) {
            (Some(bullet), Some(first)) => {
                let run = TextRun {
                    text: bullet.clone(),
                    underline: false,
                    strike: false,
                    ..first.clone()
                };
                shape(fonts, p, std::slice::from_ref(&run), None, None)
            }
            _ => None,
        };
        paras.push(Para {
            text,
            bullet,
            x: block.rect.x + p.indent,
            top: y,
        });
        y += height + p.space_after;
    }
    let total = y;
    let offset = block.rect.y
        + match block.valign {
            VAlign::Top => 0.0,
            VAlign::Middle => (block.rect.height - total) / 2.0,
            VAlign::Bottom => block.rect.height - total,
        };

    let mut pieces = Vec::new();
    let mut bounds = block.rect;
    for para in paras {
        let top = offset + para.top;
        if let Some(bullet) = para.bullet {
            let baseline = |b: &Buffer| b.layout_runs().next().map_or(0.0, |r| r.line_y);
            let y = top + baseline(&para.text) - baseline(&bullet);
            pieces.push(Placed {
                buffer: bullet,
                x: block.rect.x,
                y,
            });
        }
        for run in para.text.layout_runs() {
            bounds = union(
                bounds,
                Rect {
                    x: para.x,
                    y: top + run.line_top,
                    width: run.line_w,
                    height: run.line_height,
                },
            );
        }
        pieces.push(Placed {
            buffer: para.text,
            x: para.x,
            y: top,
        });
    }
    // Ascenders and descenders can poke out of the line box a little.
    let slack = block
        .paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .map(|r| r.size)
        .fold(0.0, f32::max)
        * 0.5;
    LaidOut {
        pieces,
        bounds: Rect {
            x: bounds.x - slack,
            y: bounds.y - slack,
            width: bounds.width + 2.0 * slack,
            height: bounds.height + 2.0 * slack,
        },
    }
}

fn union(a: Rect, b: Rect) -> Rect {
    let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
    let (x1, y1) = (
        (a.x + a.width).max(b.x + b.width),
        (a.y + a.height).max(b.y + b.height),
    );
    Rect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}

fn align(align: HAlign) -> Option<Align> {
    Some(match align {
        HAlign::Left => Align::Left,
        HAlign::Center => Align::Center,
        HAlign::Right => Align::Right,
        HAlign::Justify => Align::Justified,
    })
}

/// The family cosmic-text is asked for: the bundled stand-in where there is
/// one, else the requested name (cosmic-text falls back from there).
pub(crate) fn resolve_family(fonts: &Fonts, family: &str) -> String {
    match fonts.lookup(family) {
        FontMatch::Substitute(to) => to,
        _ => family.to_string(),
    }
}

pub(crate) fn to_cosmic(color: Color) -> cosmic_text::Color {
    cosmic_text::Color::rgba(color.r, color.g, color.b, color.a)
}

fn shape(
    fonts: &mut Fonts,
    p: &Paragraph,
    runs: &[TextRun],
    width: Option<f32>,
    align: Option<Align>,
) -> Option<Buffer> {
    let first = runs.first()?;
    let metrics = |size: f32| {
        let size = size.max(0.01);
        Metrics::new(size, size * p.line_spacing.max(0.0))
    };
    let families: Vec<String> = runs
        .iter()
        .map(|r| resolve_family(fonts, &r.font_family))
        .collect();
    let attrs = |i: usize, r: &TextRun| {
        let mut a = Attrs::new()
            .family(Family::Name(&families[i]))
            .weight(Weight(r.weight))
            .style(if r.italic {
                Style::Italic
            } else {
                Style::Normal
            })
            .color(to_cosmic(r.color))
            .metrics(metrics(r.size))
            .metadata(i);
        if r.underline {
            a = a.underline(UnderlineStyle::Single);
        }
        if r.strike {
            a = a.strikethrough();
        }
        a
    };
    let system: &mut FontSystem = fonts.system();
    let mut buffer = Buffer::new(system, metrics(first.size));
    buffer.set_wrap(Wrap::WordOrGlyph);
    buffer.set_size(width, None);
    buffer.set_rich_text(
        runs.iter()
            .enumerate()
            .map(|(i, r)| (r.text.as_str(), attrs(i, r))),
        &attrs(0, first),
        Shaping::Advanced,
        align,
    );
    buffer.shape_until_scroll(system, false);
    Some(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, size: f32) -> TextRun {
        TextRun {
            text: text.into(),
            font_family: "Calibri".into(),
            size,
            weight: 400,
            italic: false,
            underline: false,
            strike: false,
            color: Color::WHITE,
        }
    }

    fn para(runs: Vec<TextRun>) -> Paragraph {
        Paragraph {
            align: HAlign::Left,
            line_spacing: 1.2,
            space_before: 0.0,
            space_after: 0.0,
            indent: 0.0,
            bullet: None,
            runs,
        }
    }

    fn block(valign: VAlign, paragraphs: Vec<Paragraph>) -> TextBlock {
        TextBlock {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 300.0,
            },
            valign,
            paragraphs,
            shadow: None,
            outline: None,
        }
    }

    #[test]
    fn line_height_is_size_times_spacing() {
        let mut fonts = Fonts::bundled_only();
        let laid = lay_out(
            &mut fonts,
            &block(VAlign::Top, vec![para(vec![run("Hi", 50.0)])]),
        );
        let lines: Vec<f32> = laid.pieces[0]
            .buffer
            .layout_runs()
            .map(|r| r.line_height)
            .collect();
        assert_eq!(lines.len(), 1);
        assert!((lines[0] - 60.0).abs() < 1e-3, "{lines:?}");
    }

    #[test]
    fn valign_moves_the_block() {
        let mut fonts = Fonts::bundled_only();
        let mut top =
            |v| lay_out(&mut fonts, &block(v, vec![para(vec![run("Hi", 50.0)])])).pieces[0].y;
        assert_eq!(top(VAlign::Top), 0.0);
        assert_eq!(top(VAlign::Middle), 120.0);
        assert_eq!(top(VAlign::Bottom), 240.0);
    }

    #[test]
    fn paragraphs_stack_with_spacing() {
        let mut fonts = Fonts::bundled_only();
        let mut second = para(vec![run("Two", 50.0)]);
        second.space_before = 10.0;
        let laid = lay_out(
            &mut fonts,
            &block(VAlign::Top, vec![para(vec![run("One", 50.0)]), second]),
        );
        assert_eq!(laid.pieces[1].y, 70.0);
    }

    /// Shapes the way `lampstand_layout::fit_text` measures: one style for
    /// the whole text, buffer metrics only.
    fn measured_like_layout(fonts: &mut Fonts, text: &str, size: f32, width: f32) -> Vec<Vec<f32>> {
        let family = resolve_family(fonts, "Calibri");
        let system = fonts.system();
        let mut buffer = Buffer::new(system, Metrics::new(size, size * 1.2));
        buffer.set_wrap(Wrap::WordOrGlyph);
        buffer.set_size(Some(width), None);
        let attrs = Attrs::new()
            .family(Family::Name(&family))
            .weight(Weight(400))
            .style(Style::Normal);
        buffer.set_text(text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(system, false);
        glyph_lines(&buffer)
    }

    fn glyph_lines(buffer: &Buffer) -> Vec<Vec<f32>> {
        buffer
            .layout_runs()
            .map(|r| r.glyphs.iter().map(|g| g.x).collect())
            .collect()
    }

    #[test]
    fn lines_match_what_layout_measures() {
        let mut fonts = Fonts::bundled_only();
        let verse = "The Lord is my shepherd; I shall not want. He maketh me to lie down \
                     in green pastures: he leadeth me beside the still waters.";
        for (size, width) in [(60.0, 600.0), (44.5, 900.0), (31.0, 333.0)] {
            let expected = measured_like_layout(&mut fonts, verse, size, width);
            let mut b = block(VAlign::Top, vec![para(vec![run(verse, size)])]);
            b.rect.width = width;
            let laid = lay_out(&mut fonts, &b);
            assert!(expected.len() > 1, "{size} {width} should wrap");
            assert_eq!(
                glyph_lines(&laid.pieces[0].buffer),
                expected,
                "{size} {width}"
            );
        }
    }

    #[test]
    fn bullet_hangs_in_the_indent() {
        let mut fonts = Fonts::bundled_only();
        let mut p = para(vec![run("Item", 40.0)]);
        p.indent = 50.0;
        p.bullet = Some("•".into());
        let laid = lay_out(&mut fonts, &block(VAlign::Top, vec![p]));
        assert_eq!(laid.pieces.len(), 2);
        assert_eq!((laid.pieces[0].x, laid.pieces[1].x), (0.0, 50.0));
    }

    #[test]
    fn wraps_at_width_minus_indent() {
        let mut fonts = Fonts::bundled_only();
        let words = "Amazing grace how sweet the sound that saved a wretch like me";
        let lines = |indent: f32, fonts: &mut Fonts| {
            let mut p = para(vec![run(words, 40.0)]);
            p.indent = indent;
            lay_out(fonts, &block(VAlign::Top, vec![p])).pieces[0]
                .buffer
                .layout_runs()
                .count()
        };
        assert!(lines(200.0, &mut fonts) > lines(0.0, &mut fonts));
    }
}
