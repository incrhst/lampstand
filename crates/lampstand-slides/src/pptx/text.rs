//! Text bodies to text blocks: every paragraph and run property resolved
//! through the placeholder chain and the master's text styles.
//!
//! Sizes follow the file's saved autofit (`normAutofit fontScale` and
//! `lnSpcReduction`), which is what PowerPoint draws when it opens a deck;
//! nothing is measured here. A line is 1.2 × the font size at 100% line
//! spacing, the ratio DeckCraft and LibreOffice use for "single".

use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::text::{
    Align, Anchor, AutoFit, BodyProps, Bullet, Caps, RunKind, Spacing, Strike, TextBody, TextDir,
};
use deckcraft_model::{Fill, Shape};
use lampstand_core::geom::Rect;
use lampstand_core::scene::{HAlign, Outline, Paragraph, Shadow, TextBlock, TextRun, VAlign};

use super::paint;
use super::SlideCx;

/// What the text block needs from its shape beyond the body itself.
pub(crate) struct Frame {
    /// The text rectangle in the node's local space, before insets.
    pub rect: Rect,
    /// A shadow for the text when the shape has no fill or line to cast
    /// it (PowerPoint then shadows the glyphs).
    pub shadow: Option<Shadow>,
}

/// A laid-out text block and the rotation (degrees clockwise about the
/// text rectangle's centre) it is drawn at: `bodyPr rot` plus vertical
/// text's quarter turn.
pub(crate) struct Placed {
    pub block: TextBlock,
    pub rotation: f32,
}

pub(crate) fn block(
    sc: &mut SlideCx,
    ctx: &Ctx,
    shape: &Shape,
    body: &TextBody,
    bp: &BodyProps,
    frame: Frame,
) -> Option<Placed> {
    if body.paragraphs.iter().all(|p| p.runs.is_empty()) {
        return None;
    }
    let (scale, reduce) = match bp.autofit {
        Some(AutoFit::Shrink {
            font_scale,
            line_reduction,
        }) => (
            font_scale
                .filter(|s| s.is_finite())
                .map_or(1.0, |s| s.clamp(0.1, 1.0)),
            if line_reduction.is_finite() {
                line_reduction.clamp(0.0, 0.2)
            } else {
                0.0
            },
        ),
        _ => (1.0, 0.0),
    };
    let rotation = match bp.vert.unwrap_or(TextDir::Horizontal) {
        TextDir::Horizontal => 0.0,
        TextDir::Vertical270 => 270.0,
        _ => 90.0,
    } + bp.rot.unwrap_or(0.0);
    let rotation = (rotation as f32).rem_euclid(360.0);
    // Vertical text is laid out in the rectangle turned a quarter turn about
    // its centre.
    let r = frame.rect;
    let r = if (rotation - 90.0).abs() < 0.01 || (rotation - 270.0).abs() < 0.01 {
        Rect {
            x: r.x + (r.width - r.height) / 2.0,
            y: r.y + (r.height - r.width) / 2.0,
            width: r.height,
            height: r.width,
        }
    } else {
        r
    };
    let inset = |v: Option<f64>, d: f64| v.filter(|v| v.is_finite()).unwrap_or(d) as f32;
    let (l, t) = (inset(bp.inset_l, 7.2), inset(bp.inset_t, 3.6));
    let (rr, b) = (inset(bp.inset_r, 7.2), inset(bp.inset_b, 3.6));
    let mut rect = Rect {
        x: r.x + l,
        y: r.y + t,
        width: (r.width - l - rr).max(0.0),
        height: (r.height - t - b).max(0.0),
    };

    let mut paragraphs = Vec::with_capacity(body.paragraphs.len());
    let mut numbering = Numbering::default();
    let mut shadow = None;
    let mut outline = None;
    for para in &body.paragraphs {
        let pp = resolve::para(ctx, shape, para);
        let mut runs = Vec::new();
        for run in &para.runs {
            let rp = resolve::run(ctx, shape, para, &run.props);
            let family = resolve::font_family(ctx, &rp);
            sc.deck.check_font(sc.index, &family);
            let mut text = match &run.kind {
                RunKind::Break => "\n".to_string(),
                RunKind::Field { field } if field == "slidenum" => {
                    sc.deck.pres.slide_number(sc.index).to_string()
                }
                _ => run.text.replace('\u{b}', "\n"),
            };
            if matches!(rp.caps, Some(Caps::All | Caps::Small)) {
                text = text.to_uppercase();
            }
            if shadow.is_none() {
                if let Some(sh) = &rp.shadow {
                    shadow =
                        paint::shadow(ctx, sh, None, lampstand_core::geom::Transform::IDENTITY);
                }
            }
            if outline.is_none() {
                outline = text_outline(ctx, &rp);
            }
            runs.push(TextRun {
                text,
                font_family: family,
                size: (rp.size.unwrap_or(18.0) * scale) as f32,
                weight: if rp.bold.unwrap_or(false) { 700 } else { 400 },
                italic: rp.italic.unwrap_or(false),
                underline: rp.underline.as_deref().is_some_and(|u| u != "none"),
                strike: matches!(rp.strike, Some(Strike::Single | Strike::Double)),
                color: paint::color(resolve::text_color(ctx, &rp)),
            });
        }
        // An empty paragraph still takes a line, at its end-of-paragraph size.
        let empty = runs.iter().all(|r| r.text.is_empty());
        if runs.is_empty() || empty {
            let rp = resolve::run(ctx, shape, para, &para.end_props);
            runs = vec![TextRun {
                text: " ".into(),
                font_family: resolve::font_family(ctx, &rp),
                size: (rp.size.unwrap_or(18.0) * scale) as f32,
                weight: 400,
                italic: false,
                underline: false,
                strike: false,
                color: paint::color(resolve::text_color(ctx, &rp)),
            }];
        }
        let size = runs.iter().map(|r| r.size).fold(0.0_f32, f32::max).max(1.0);
        let first = runs.first().map_or(size, |r| r.size);
        let line_spacing = match pp.line_spacing {
            Some(Spacing::Pct(f)) => 1.2 * ((f - reduce).max(f * 0.1).max(0.0)) as f32,
            Some(Spacing::Pts(v)) => (v * (1.0 - reduce)) as f32 / size,
            None => 1.2 * (1.0 - reduce) as f32,
        };
        let spacing = |s: Option<Spacing>| match s {
            Some(Spacing::Pct(f)) => (f as f32) * first * 1.2,
            Some(Spacing::Pts(v)) => v as f32,
            None => 0.0,
        };
        let margin = pp.margin_left.unwrap_or(0.0).max(0.0) as f32;
        let hang = pp.indent.unwrap_or(0.0) as f32;
        let bullet = if empty {
            numbering.skip(para.level);
            None
        } else {
            numbering.bullet(para.level, pp.bullet.as_ref())
        };
        let mut paragraph = Paragraph {
            align: match pp.align.unwrap_or(Align::Left) {
                Align::Left => HAlign::Left,
                Align::Center => HAlign::Center,
                Align::Right => HAlign::Right,
                Align::Justify | Align::JustLow | Align::Distributed => HAlign::Justify,
            },
            line_spacing,
            space_before: spacing(pp.space_before),
            space_after: spacing(pp.space_after),
            indent: margin,
            bullet,
            runs,
        };
        if paragraph.bullet.is_some() {
            hang_bullet(&mut paragraph, margin + hang);
        } else {
            // No bullet: PowerPoint starts the first line at `marL + indent`
            // and wraps the rest at `marL`. Our paragraphs have one indent;
            // the first line's is kept, since most paragraphs are one line.
            paragraph.indent = (margin + hang).max(0.0);
        }
        paragraphs.push(paragraph);
    }

    // `wrap="none"`: the text runs on past the box, anchored by alignment.
    if !bp.wrap.unwrap_or(true) {
        const WIDE: f32 = 10_000.0;
        let grow = WIDE - rect.width;
        match paragraphs.first().map(|p| p.align) {
            Some(HAlign::Center) => rect.x -= grow / 2.0,
            Some(HAlign::Right) => rect.x -= grow,
            _ => {}
        }
        rect.width = WIDE;
    }

    Some(Placed {
        block: TextBlock {
            rect,
            valign: match bp.anchor.unwrap_or(Anchor::Top) {
                Anchor::Top => VAlign::Top,
                Anchor::Middle | Anchor::Justified | Anchor::Distributed => VAlign::Middle,
                Anchor::Bottom => VAlign::Bottom,
            },
            paragraphs,
            shadow: shadow.or(frame.shadow),
            outline,
        },
        rotation,
    })
}

/// Our renderer hangs a bullet at the block's left edge. PowerPoint puts it
/// at `marL + indent`, which is the left edge for top-level bullets but
/// further in for nested ones. Those get the bullet folded into the text
/// at their own indent, followed by thin spaces to about where the text
/// starts (bullet glyphs taken as 0.55 em, thin spaces as 0.2 em).
fn hang_bullet(p: &mut Paragraph, bullet_at: f32) {
    if bullet_at <= 0.5 {
        return;
    }
    let gap = (p.indent - bullet_at).max(0.0);
    p.indent = bullet_at;
    let Some(bullet) = p.bullet.take() else {
        return;
    };
    let Some(first) = p.runs.first_mut() else {
        return;
    };
    let em = first.size.max(1.0);
    let room = gap - 0.55 * em * bullet.chars().count() as f32;
    let spaces = ((room / (0.2 * em)).round() as usize).max(1);
    first.text = format!("{bullet}{}{}", "\u{2009}".repeat(spaces), first.text);
}

fn text_outline(ctx: &Ctx, rp: &deckcraft_model::RunProps) -> Option<Outline> {
    let line = rp.outline.as_ref()?;
    let color = match line.fill.as_ref()? {
        Fill::Solid { color } => paint::color(ctx.color(color, None)),
        _ => return None,
    };
    let width = line.width.unwrap_or(0.75) as f32;
    // A DrawingML text outline is centred on the glyph edge.
    (width > 0.0).then_some(Outline {
        color,
        width: width / 2.0,
    })
}

/// Auto-number counters, one per outline level.
#[derive(Default)]
struct Numbering {
    levels: Vec<Option<(String, u32)>>,
}

impl Numbering {
    /// A paragraph that shows no bullet: deeper lists restart.
    fn skip(&mut self, level: u8) {
        self.levels.truncate(usize::from(level) + 1);
    }

    fn bullet(&mut self, level: u8, bullet: Option<&Bullet>) -> Option<String> {
        let level = usize::from(level);
        self.levels.truncate(level + 1);
        while self.levels.len() <= level {
            self.levels.push(None);
        }
        match bullet {
            Some(Bullet::Char { char }) => {
                self.levels[level] = None;
                Some(char.clone())
            }
            Some(Bullet::AutoNum { scheme, start_at }) => {
                let n = match &self.levels[level] {
                    Some((s, n)) if s == scheme => n + 1,
                    _ => (*start_at).max(1),
                };
                self.levels[level] = Some((scheme.clone(), n));
                Some(autonum(scheme, n))
            }
            // A picture bullet has no glyph; a plain dot stands in.
            Some(Bullet::Picture { .. }) => {
                self.levels[level] = None;
                Some("•".into())
            }
            Some(Bullet::None) | None => {
                self.levels[level] = None;
                None
            }
        }
    }
}

/// An auto-number bullet's text: `arabicPeriod` 3 → "3.". Latin-script
/// schemes only; the rest are numbered as `arabicPeriod`.
fn autonum(scheme: &str, n: u32) -> String {
    let core = if scheme.starts_with("romanUc") {
        roman(n).to_uppercase()
    } else if scheme.starts_with("romanLc") {
        roman(n)
    } else if scheme.starts_with("alphaUc") {
        alpha(n).to_uppercase()
    } else if scheme.starts_with("alphaLc") {
        alpha(n)
    } else {
        n.to_string()
    };
    if scheme.ends_with("ParenBoth") {
        format!("({core})")
    } else if scheme.ends_with("ParenR") {
        format!("{core})")
    } else if scheme.ends_with("Plain") {
        core
    } else if scheme.ends_with("Minus") {
        format!("- {core} -")
    } else {
        format!("{core}.")
    }
}

/// a, b, ..., z, aa, bb, ... as PowerPoint letters lists.
fn alpha(n: u32) -> String {
    let n = n.max(1) - 1;
    let letter = char::from(b'a' + (n % 26) as u8);
    std::iter::repeat_n(letter, (n / 26 + 1) as usize).collect()
}

fn roman(mut n: u32) -> String {
    const TABLE: [(u32, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (v, s) in TABLE {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_numbers_follow_powerpoint_schemes() {
        assert_eq!(autonum("arabicPeriod", 3), "3.");
        assert_eq!(autonum("arabicParenR", 2), "2)");
        assert_eq!(autonum("romanUcPeriod", 4), "IV.");
        assert_eq!(autonum("romanLcParenBoth", 9), "(ix)");
        assert_eq!(autonum("alphaLcParenR", 2), "b)");
        assert_eq!(autonum("alphaUcPeriod", 28), "BB.");
        assert_eq!(autonum("arabicPlain", 7), "7");
    }

    #[test]
    fn numbering_continues_past_deeper_levels_and_restarts_them() {
        let num = Bullet::AutoNum {
            scheme: "arabicPeriod".into(),
            start_at: 1,
        };
        let sub = Bullet::AutoNum {
            scheme: "alphaLcPeriod".into(),
            start_at: 1,
        };
        let mut n = Numbering::default();
        let got: Vec<Option<String>> = [(0, &num), (1, &sub), (1, &sub), (0, &num), (1, &sub)]
            .iter()
            .map(|(l, b)| n.bullet(*l, Some(b)))
            .collect();
        let want = ["1.", "a.", "b.", "2.", "a."].map(|s| Some(s.to_string()));
        assert_eq!(got, want);
    }

    #[test]
    fn a_nested_bullet_moves_into_the_text() {
        let mut p = Paragraph {
            align: HAlign::Left,
            line_spacing: 1.2,
            space_before: 0.0,
            space_after: 0.0,
            indent: 54.0,
            bullet: Some("–".into()),
            runs: vec![TextRun {
                text: "Point".into(),
                font_family: "Arial".into(),
                size: 20.0,
                weight: 400,
                italic: false,
                underline: false,
                strike: false,
                color: lampstand_core::geom::Color::BLACK,
            }],
        };
        hang_bullet(&mut p, 36.0);
        assert_eq!(p.indent, 36.0);
        assert_eq!(p.bullet, None);
        // 18 pt of room, less 11 pt for the dash: two 4 pt thin spaces.
        assert_eq!(p.runs[0].text, "–\u{2009}\u{2009}Point");
    }
}
