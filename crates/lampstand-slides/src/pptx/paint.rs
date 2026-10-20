//! Colours, fills, outlines and shadows, resolved against the slide's
//! theme and colour map.

use deckcraft_color::Rgba;
use deckcraft_geom::preset::FillMode;
use deckcraft_model::resolve::Ctx;
use deckcraft_model::style::{Fill, GradientShape, Line};
use lampstand_core::geom::{Color, Point, Transform};
use lampstand_core::scene::{GradientStop, Paint, Shadow, Stroke};

use super::limits;

pub(crate) fn color(c: Rgba) -> Color {
    Color {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
}

/// Lightens or darkens a colour for preset sub-paths drawn as 3-D faces
/// (`lighten`, `darkenLess`...), by DeckCraft's amounts.
fn shade(c: Color, mode: FillMode) -> Color {
    let (towards, k) = match mode {
        FillMode::Lighten => (255.0, 0.4),
        FillMode::LightenLess => (255.0, 0.2),
        FillMode::Darken => (0.0, 0.4),
        FillMode::DarkenLess => (0.0, 0.2),
        FillMode::Norm | FillMode::None => return c,
    };
    let mix = |v: u8| (f32::from(v) + (towards - f32::from(v)) * k).round() as u8;
    Color {
        r: mix(c.r),
        g: mix(c.g),
        b: mix(c.b),
        a: c.a,
    }
}

pub(crate) fn shade_paint(p: Paint, mode: FillMode) -> Paint {
    match p {
        Paint::Solid(c) => Paint::Solid(shade(c, mode)),
        Paint::Linear { start, end, stops } => Paint::Linear {
            start,
            end,
            stops: stops
                .into_iter()
                .map(|s| GradientStop {
                    offset: s.offset,
                    color: shade(s.color, mode),
                })
                .collect(),
        },
        Paint::Radial {
            center,
            radius,
            stops,
        } => Paint::Radial {
            center,
            radius,
            stops: stops
                .into_iter()
                .map(|s| GradientStop {
                    offset: s.offset,
                    color: shade(s.color, mode),
                })
                .collect(),
        },
    }
}

/// The paint for a solid or gradient fill over a `w` × `h` box in local
/// space. `None` for no fill, and for pictures, which become image nodes.
pub(crate) fn paint(ctx: &Ctx, fill: &Fill, ph: Option<Rgba>, w: f32, h: f32) -> Option<Paint> {
    match fill {
        Fill::None | Fill::Group | Fill::Background | Fill::Picture(_) => None,
        Fill::Solid { color: c } => Some(Paint::Solid(color(ctx.color(c, ph)))),
        Fill::Pattern(p) => {
            // Drawn as its average colour: the foreground mixed into the
            // background by how much of the cell the pattern covers.
            let (fg, bg) = (color(ctx.color(&p.fg, ph)), color(ctx.color(&p.bg, ph)));
            Some(Paint::Solid(mix(bg, fg, pattern_cover(&p.preset))))
        }
        Fill::Gradient(g) => {
            if g.stops.is_empty() {
                return None;
            }
            let mut stops: Vec<GradientStop> = g
                .stops
                .iter()
                .map(|s| GradientStop {
                    offset: s.pos.clamp(0.0, 1.0) as f32,
                    color: color(ctx.color(&s.color, ph)),
                })
                .collect();
            stops.sort_by(|a, b| a.offset.total_cmp(&b.offset));
            // The tile rectangle (`a:tileRect`) moves the box a gradient
            // spans; insets are fractions of the shape's box.
            let [l, t, r, b] = g.tile.map(|v| if v.is_finite() { v as f32 } else { 0.0 });
            let (x0, y0, x1, y1) = (l * w, t * h, w - r * w, h - b * h);
            let (x0, y0, x1, y1) = if x1 - x0 > 1e-3 && y1 - y0 > 1e-3 {
                (x0, y0, x1, y1)
            } else {
                (0.0, 0.0, w, h)
            };
            let (cx, cy, tw, th) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, x1 - x0, y1 - y0);
            Some(match &g.shape {
                GradientShape::Linear { angle, .. } => {
                    // Clockwise from left → right, spanning the box along
                    // that direction, as DeckCraft and PowerPoint lay it out.
                    let a = (*angle as f32).to_radians();
                    let (dx, dy) = (a.cos(), a.sin());
                    let half = (tw * dx.abs() + th * dy.abs()) / 2.0;
                    Paint::Linear {
                        start: Point {
                            x: cx - dx * half,
                            y: cy - dy * half,
                        },
                        end: Point {
                            x: cx + dx * half,
                            y: cy + dy * half,
                        },
                        stops,
                    }
                }
                GradientShape::Path { focus, .. } => {
                    // A circle from the focus (the middle of `fillToRect`)
                    // out to the circle round the box. Ours has one centre,
                    // so it is the focus, and the radius reaches the box's
                    // farthest corner from it.
                    let fx = w * (focus[0] as f32 + 1.0 - focus[2] as f32) / 2.0;
                    let fy = h * (focus[1] as f32 + 1.0 - focus[3] as f32) / 2.0;
                    let (fx, fy) = if fx.is_finite() && fy.is_finite() {
                        (fx, fy)
                    } else {
                        (cx, cy)
                    };
                    let radius = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)]
                        .iter()
                        .map(|(x, y)| (x - fx).hypot(y - fy))
                        .fold(0.0_f32, f32::max)
                        .max(1e-3);
                    Paint::Radial {
                        center: Point { x: fx, y: fy },
                        radius,
                        stops,
                    }
                }
            })
        }
    }
}

/// How much of a pattern's cell its foreground covers: `pctN` says, the
/// light and dark variants are a quarter and a half, the rest about a third.
fn pattern_cover(preset: &str) -> f32 {
    if let Some(n) = preset
        .strip_prefix("pct")
        .and_then(|n| n.parse::<f32>().ok())
    {
        return (n / 100.0).clamp(0.0, 1.0);
    }
    if preset.starts_with("lt") || preset.starts_with("narrow") {
        0.25
    } else if preset.starts_with("dk") || preset.starts_with("wd") || preset.starts_with("solid") {
        0.5
    } else {
        0.33
    }
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color {
        r: m(a.r, b.r),
        g: m(a.g, b.g),
        b: m(a.b, b.b),
        a: m(a.a, b.a),
    }
}

/// An outline as a stroke, or `None` for no line. Widths and dashes are in
/// points: callers keep the node's local units in points.
pub(crate) fn stroke(ctx: &Ctx, line: &Line, ph: Option<Rgba>, w: f32, h: f32) -> Option<Stroke> {
    let fill = line.fill.as_ref()?;
    let paint = match fill {
        Fill::Solid { .. } | Fill::Gradient(_) | Fill::Pattern(_) => paint(ctx, fill, ph, w, h)?,
        _ => return None,
    };
    let width = limits::clamp(line.width.unwrap_or(0.75), 0.0, limits::LINE_WIDTH, 0.75);
    if width <= 0.0 {
        return None;
    }
    let dash = line
        .dash
        .as_ref()
        .map(|d| d.pattern())
        .filter(|p| !p.is_empty())
        .map(|p| dash_pattern(&p, width))
        .filter(|d| !d.is_empty());
    Some(Stroke { paint, width, dash })
}

/// Dash and gap lengths in points from a pattern in multiples of the line
/// width. A hostile custom pattern (thousands of entries, or lengths near
/// zero along a long line) would have the renderer cut millions of
/// segments, so entries and lengths are bounded.
fn dash_pattern(pattern: &[f64], width: f32) -> Vec<f32> {
    let (lo, hi) = limits::DASH;
    let mut out: Vec<f32> = pattern
        .iter()
        .take(limits::DASH_ENTRIES)
        .map(|v| limits::clamp(*v, lo, hi, 1.0) * width.max(0.1))
        .collect();
    // Dash, gap pairs.
    if out.len() % 2 == 1 {
        out.pop();
    }
    out
}

/// The colour a line is drawn in (for arrowheads): its solid colour, or a
/// gradient's first stop.
pub(crate) fn line_color(ctx: &Ctx, line: &Line, ph: Option<Rgba>) -> Option<Color> {
    match line.fill.as_ref()? {
        Fill::Solid { color: c } => Some(color(ctx.color(c, ph))),
        Fill::Gradient(g) => g.stops.first().map(|s| color(ctx.color(&s.color, ph))),
        _ => None,
    }
}

/// An outer shadow in a node's local space. DrawingML gives the offset on
/// the slide (unless it turns with the shape), so it is taken back through
/// the node's transform. `blurRad` is drawn as a Gaussian of a third of it
/// (DeckCraft's match to PowerPoint), and our `blur` is twice the sigma.
pub(crate) fn shadow(
    ctx: &Ctx,
    sh: &deckcraft_model::style::Shadow,
    ph: Option<Rgba>,
    t: Transform,
) -> Option<Shadow> {
    if sh.inner {
        return None;
    }
    let c = color(ctx.color(&sh.color, ph));
    let dir = limits::clamp(sh.dir, -360.0, 720.0, 0.0).to_radians();
    let dist = limits::clamp(sh.dist, 0.0, limits::DISTANCE, 0.0);
    let (dx, dy) = (dir.cos() * dist, dir.sin() * dist);
    let offset = if sh.rotate_with_shape {
        Point { x: dx, y: dy }
    } else {
        unrotate(t, dx, dy)
    };
    if !offset.x.is_finite() || !offset.y.is_finite() {
        return None;
    }
    Some(Shadow {
        color: c,
        offset,
        blur: limits::clamp(sh.blur, 0.0, limits::DISTANCE, 0.0) * 2.0 / 3.0,
    })
}

/// A slide-space vector in a node's local space (the inverse of the
/// transform's linear part).
fn unrotate(t: Transform, x: f32, y: f32) -> Point {
    let det = t.a * t.d - t.b * t.c;
    if det.abs() < 1e-9 {
        return Point { x, y };
    }
    Point {
        x: (t.d * x - t.c * y) / det,
        y: (-t.b * x + t.a * y) / det,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_offset_stays_on_the_slide_under_rotation() {
        let quarter = Transform {
            a: 0.0,
            b: 1.0,
            c: -1.0,
            d: 0.0,
            e: 0.0,
            f: 0.0,
        };
        // A slide-space offset of (10, 0) is (0, -10) in a node turned a
        // quarter turn clockwise.
        let p = unrotate(quarter, 10.0, 0.0);
        assert!((p.x).abs() < 1e-5 && (p.y + 10.0).abs() < 1e-5, "{p:?}");
    }

    #[test]
    fn patterns_mix_by_their_cover() {
        assert_eq!(pattern_cover("pct20"), 0.2);
        assert_eq!(pattern_cover("dkDnDiag"), 0.5);
        assert_eq!(
            mix(Color::WHITE, Color::BLACK, 0.25),
            Color::rgb(191, 191, 191)
        );
    }

    #[test]
    fn hostile_dash_patterns_are_bounded() {
        let many = vec![1.0e-9; 100_000];
        let got = dash_pattern(&many, 2.0);
        assert_eq!(got.len(), limits::DASH_ENTRIES);
        assert!(got.iter().all(|d| *d >= 0.2));
        let huge = dash_pattern(&[1.0e300, f64::NAN], 2.0);
        assert_eq!(huge, vec![2000.0, 2.0]);
    }

    #[test]
    fn shading_moves_towards_white_or_black() {
        let c = Color::rgb(100, 100, 100);
        assert_eq!(shade(c, FillMode::Lighten), Color::rgb(162, 162, 162));
        assert_eq!(shade(c, FillMode::Darken), Color::rgb(60, 60, 60));
        assert_eq!(shade(c, FillMode::Norm), c);
    }
}
