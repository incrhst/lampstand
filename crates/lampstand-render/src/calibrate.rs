//! Calibration pattern (TS-1) as a `Scene`.
//!
//! Numbered rings step in from every edge at 1% of the output's width and
//! height. Ring `n` has its number just inside it, so the number can only be
//! seen if ring `n` is on the screen. A volunteer at the back reads the
//! smallest number they can see in full on each edge; that is the margin,
//! in percent, to set for that edge.
//!
//! At 1% on a 1080p output the rings are only about 11 px apart, so every
//! ring has a small number and every fifth ring also has a larger one,
//! which extends inwards from the ring and so is visible only if its ring is.
//! The rest of the pattern is there to catch a wrongly set up output:
//!
//! - a different colour in each corner, so a flipped or rotated output is
//!   obvious;
//! - a 10% grid, to spot keystone and scaling at a glance;
//! - the current safe area outlined in cyan, with each margin labelled and
//!   the edge being adjusted drawn in white with its label enlarged;
//! - a caption naming the output and its aspect ratio ("32:9").
//!
//! Like everything the renderer draws, this is only paths and text, and it
//! uses the bundled fonts, so it looks the same in a golden test as on the
//! projector. `output::overlay::calibration_scene` calls [`scene`].

use lampstand_core::geom::{Color, Point, Rect, Transform};
use lampstand_core::safe_area::SafeArea;
use lampstand_core::scene::{
    HAlign, Node, NodeKind, Paint, Paragraph, PathCommand, PathNode, Scene, Stroke, TextBlock,
    TextRun, VAlign,
};
use serde::{Deserialize, Serialize};

use crate::CalibrationPattern;

/// One side of the safe area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Edge {
    Top,
    Right,
    Bottom,
    Left,
}

impl Edge {
    pub const ALL: [Edge; 4] = [Edge::Top, Edge::Right, Edge::Bottom, Edge::Left];

    pub fn name(self) -> &'static str {
        match self {
            Edge::Top => "TOP",
            Edge::Right => "RIGHT",
            Edge::Bottom => "BOTTOM",
            Edge::Left => "LEFT",
        }
    }

    /// This edge's margin in `area`, as a fraction.
    pub fn margin(self, area: &SafeArea) -> f32 {
        match self {
            Edge::Top => area.top,
            Edge::Right => area.right,
            Edge::Bottom => area.bottom,
            Edge::Left => area.left,
        }
    }
}

/// Corner colours: top-left, top-right, bottom-right, bottom-left.
pub const CORNERS: [Color; 4] = [
    Color::rgb(255, 40, 40),
    Color::rgb(40, 220, 40),
    Color::rgb(60, 110, 255),
    Color::rgb(255, 60, 255),
];
/// Ring 0, the very edge of the picture.
pub const EDGE: Color = Color::rgb(255, 140, 0);
pub const GRID: Color = Color::rgb(70, 70, 70);
/// The ring colours, alternating every five rings.
pub const RING_A: Color = Color::rgb(255, 255, 255);
pub const RING_B: Color = Color::rgb(255, 210, 0);
pub const SAFE: Color = Color::rgb(0, 229, 255);
/// The edge being adjusted.
pub const ACTIVE: Color = Color::rgb(255, 255, 255);

/// Rings run to 45% at most: further in and they would cross.
const MAX_RING: u32 = 45;
/// Node IDs for the pattern, clear of content (below 2^24) and of the
/// compositor and the other overlays (from 2^24 up to `u32::MAX - 1024`).
const NODE_BASE: u32 = 1 << 31;
const FONT: &str = "Noto Sans";
/// Width of a bold digit or letter relative to its font size, rounded up so
/// that estimated label boxes always contain the text.
const GLYPH_WIDTH: f32 = 0.72;

/// Common names for an aspect ratio, so the volunteer can check the output
/// mode is what they expect.
pub fn aspect_name(width: u32, height: u32) -> String {
    let ratio = width as f32 / height as f32;
    let named = [
        (1.0, "1:1"),
        (5.0 / 4.0, "5:4"),
        (4.0 / 3.0, "4:3"),
        (3.0 / 2.0, "3:2"),
        (16.0 / 10.0, "16:10"),
        (16.0 / 9.0, "16:9"),
        (21.0 / 9.0, "21:9"),
        (32.0 / 9.0, "32:9"),
    ];
    named
        .iter()
        .find(|(r, _)| (ratio - r).abs() < 0.02)
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| format!("{ratio:.2}:1"))
}

/// Line thickness for an output: 2 px at 1080p, never under 1 px.
pub fn line_width(width: f32, height: f32) -> f32 {
    (width.min(height) / 540.0).round().max(1.0)
}

/// Where ring `percent` is: its distance in from the left and right edges,
/// and from the top and bottom, in pixels.
pub fn ring_inset(percent: u32, width: f32, height: f32) -> (f32, f32) {
    let f = percent as f32 / 100.0;
    (width * f, height * f)
}

fn dim(c: Color) -> Color {
    let k = |v: u8| (v as f32 * 0.55).round() as u8;
    Color::rgb(k(c.r), k(c.g), k(c.b))
}

/// The colour of ring `n` and of its numbers. Every fifth ring is drawn
/// bright; the others are dimmer so the fives stand out.
pub fn ring_color(n: u32) -> Color {
    let base = if (n / 5).is_multiple_of(2) {
        RING_A
    } else {
        RING_B
    };
    if n == 0 {
        EDGE
    } else if n.is_multiple_of(5) {
        base
    } else {
        dim(base)
    }
}

/// The colour of ring `n`'s numbers: always the bright one.
fn number_color(n: u32) -> Color {
    if n == 0 {
        EDGE
    } else if (n / 5).is_multiple_of(2) {
        RING_A
    } else {
        RING_B
    }
}

struct Builder {
    nodes: Vec<Node>,
    next: u32,
}

impl Builder {
    fn push(&mut self, kind: NodeKind) {
        self.nodes.push(Node {
            id: NODE_BASE + self.next,
            transform: Transform::IDENTITY,
            opacity: 1.0,
            visible: true,
            kind,
        });
        self.next += 1;
    }

    fn fill_rect(&mut self, r: Rect, color: Color) {
        self.push(NodeKind::Path(PathNode {
            commands: rect_commands(r),
            fill: Some(Paint::Solid(color)),
            stroke: None,
            shadow: None,
        }));
    }

    fn stroke(&mut self, commands: Vec<PathCommand>, color: Color, width: f32) {
        self.push(NodeKind::Path(PathNode {
            commands,
            fill: None,
            stroke: Some(Stroke {
                paint: Paint::Solid(color),
                width,
                dash: None,
            }),
            shadow: None,
        }));
    }

    fn stroke_rect(&mut self, r: Rect, color: Color, width: f32) {
        self.stroke(rect_commands(r), color, width);
    }

    fn line(&mut self, a: Point, b: Point, color: Color, width: f32) {
        self.stroke(
            vec![PathCommand::MoveTo(a), PathCommand::LineTo(b)],
            color,
            width,
        );
    }

    /// A single line of bold text in a box of `rect`.
    fn text(&mut self, rect: Rect, text: &str, size: f32, color: Color) {
        let block = text_block(
            rect,
            VAlign::Middle,
            vec![paragraph(HAlign::Center, text, size, color)],
        );
        self.push(NodeKind::Text(block));
    }

    /// Text centred on (`cx`, `cy`).
    fn label(&mut self, cx: f32, cy: f32, text: &str, size: f32, color: Color) {
        self.text(label_rect(cx, cy, text, size), text, size, color);
    }
}

fn p(x: f32, y: f32) -> Point {
    Point { x, y }
}

fn rect_commands(r: Rect) -> Vec<PathCommand> {
    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.width, r.y + r.height);
    vec![
        PathCommand::MoveTo(p(x0, y0)),
        PathCommand::LineTo(p(x1, y0)),
        PathCommand::LineTo(p(x1, y1)),
        PathCommand::LineTo(p(x0, y1)),
        PathCommand::Close,
    ]
}

fn paragraph(align: HAlign, text: &str, size: f32, color: Color) -> Paragraph {
    Paragraph {
        align,
        line_spacing: 1.0,
        space_before: 0.0,
        space_after: 0.0,
        indent: 0.0,
        bullet: None,
        runs: vec![TextRun {
            text: text.into(),
            font_family: FONT.into(),
            size,
            weight: 700,
            italic: false,
            underline: false,
            strike: false,
            color,
        }],
    }
}

fn text_block(rect: Rect, valign: VAlign, paragraphs: Vec<Paragraph>) -> TextBlock {
    TextBlock {
        rect,
        valign,
        paragraphs,
        shadow: None,
        outline: None,
    }
}

/// A box that holds `text` at `size`, centred on (`cx`, `cy`).
fn label_rect(cx: f32, cy: f32, text: &str, size: f32) -> Rect {
    let (w, h) = (text.chars().count() as f32 * size * GLYPH_WIDTH, size);
    Rect {
        x: cx - w / 2.0,
        y: cy - h / 2.0,
        width: w,
        height: h,
    }
}

/// The calibration pattern for an output of `width` × `height` pixels.
///
/// `pattern.current` is the safe area as it is now, `pattern.active` the
/// edge being adjusted (drawn in white with its label enlarged), and `pattern.label` names
/// the output in the caption. `pattern.max_percent` is the innermost ring.
pub fn scene(pattern: &CalibrationPattern, width: f32, height: f32) -> Scene {
    let (w, h) = (width, height);
    let line = line_width(w, h);
    let max = u32::from(pattern.max_percent).clamp(1, MAX_RING);
    let mut b = Builder {
        nodes: Vec::new(),
        next: 0,
    };
    b.fill_rect(
        Rect {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
        },
        Color::BLACK,
    );

    // A coarse 10% grid.
    for i in 1..10 {
        let f = i as f32 / 10.0;
        b.line(p(w * f, 0.0), p(w * f, h), GRID, line);
        b.line(p(0.0, h * f), p(w, h * f), GRID, line);
    }

    // Rings. Ring 0 is the very edge of the picture, drawn thick and inset
    // so that all of its line is on the screen.
    for n in 0..=max {
        let (dx, dy) = ring_inset(n, w, h);
        let (stroke, inset) = match n {
            0 => (line * 2.0, line),
            _ if n.is_multiple_of(5) => (line * 1.5, 0.0),
            _ => (line, 0.0),
        };
        b.stroke_rect(
            Rect {
                x: dx + inset,
                y: dy + inset,
                width: w - 2.0 * (dx + inset),
                height: h - 2.0 * (dy + inset),
            },
            ring_color(n),
            stroke,
        );
    }

    // Corner markers: an L in a different colour at each corner.
    let m = w.min(h);
    let arm = m * 0.08;
    let thick = (m * 0.012).max(2.0);
    let i = thick / 2.0;
    let corners = [
        [p(arm, i), p(i, i), p(i, arm)],
        [p(w - arm, i), p(w - i, i), p(w - i, arm)],
        [p(w - arm, h - i), p(w - i, h - i), p(w - i, h - arm)],
        [p(arm, h - i), p(i, h - i), p(i, h - arm)],
    ];
    for (points, color) in corners.into_iter().zip(CORNERS) {
        b.stroke(
            vec![
                PathCommand::MoveTo(points[0]),
                PathCommand::LineTo(points[1]),
                PathCommand::LineTo(points[2]),
            ],
            color,
            thick,
        );
    }
    let name_size = h * 0.03;
    for (name, (fx, fy), color) in [
        ("TL", (0.2, 0.2), CORNERS[0]),
        ("TR", (0.8, 0.2), CORNERS[1]),
        ("BR", (0.8, 0.8), CORNERS[2]),
        ("BL", (0.2, 0.8), CORNERS[3]),
    ] {
        b.label(w * fx, h * fy, name, name_size, color);
    }

    ring_numbers(&mut b, w, h, max);
    safe_area(&mut b, pattern, w, h, line);

    // Centre cross and the caption naming the output and its mode.
    let cross = m * 0.05;
    b.line(
        p(w / 2.0 - cross, h / 2.0),
        p(w / 2.0 + cross, h / 2.0),
        RING_A,
        line,
    );
    b.line(
        p(w / 2.0, h / 2.0 - cross),
        p(w / 2.0, h / 2.0 + cross),
        RING_A,
        line,
    );
    let name = if pattern.label.is_empty() {
        "Output"
    } else {
        &pattern.label
    };
    let mode = format!(
        "{} × {}  {}",
        w.round(),
        h.round(),
        aspect_name(w.round() as u32, h.round() as u32)
    );
    let (big, small) = (h * 0.04, h * 0.028);
    b.push(NodeKind::Text(text_block(
        Rect {
            x: w * 0.25,
            y: h / 2.0 + cross + small * 0.5,
            width: w * 0.5,
            height: big * 3.0,
        },
        VAlign::Top,
        vec![
            paragraph(HAlign::Center, name, big, RING_A),
            paragraph(HAlign::Center, &mode, small, RING_A),
        ],
    )));

    Scene {
        width,
        height,
        nodes: b.nodes,
    }
}

/// Where numbers go along an edge. Small numbers are at 30% and 70% of the
/// edge, which leaves the middle for the safe-area labels. Large numbers
/// are placed within the ring they belong to (the fractions are of the
/// ring's own length), so they march in diagonally as the rings do and can
/// never end up on a ring further in. Those of the left and right edges are
/// at different fractions from those of the top and bottom, so they do not
/// meet at a corner.
const SMALL_ALONG: [f32; 2] = [0.3, 0.7];
const BIG_ALONG_HORIZONTAL: [f32; 2] = [0.1, 0.9];
const BIG_ALONG_VERTICAL: [f32; 2] = [0.12, 0.88];

/// A ring number, and the box it is drawn in.
pub struct Number {
    pub ring: u32,
    pub rect: Rect,
    pub size: f32,
}

/// Every ring number for an output: a small one for each ring in the band
/// just inside it, and a large one for each fifth ring, hanging inwards
/// from it. Number `n` is always entirely inside ring `n`, so seeing it
/// means seeing the ring.
pub fn numbers(width: f32, height: f32, max_percent: u32) -> Vec<Number> {
    let (w, h) = (width, height);
    let line = line_width(w, h);
    let max = max_percent.clamp(1, MAX_RING);
    let (band_x, band_y) = (w / 100.0, h / 100.0);
    let big_size = (h * 0.03).max(12.0);
    let mut out = Vec::new();
    for n in 0..=max {
        let (dx, dy) = ring_inset(n, w, h);
        let text = n.to_string();
        let chars = text.chars().count() as f32;
        let mut push = |cx: f32, cy: f32, size: f32| {
            out.push(Number {
                ring: n,
                rect: label_rect(cx, cy, &text, size),
                size,
            });
        };

        // Small: in the band from this ring to the next. Top and bottom
        // bands are as tall as they are, so the text is as high as the
        // band; left and right bands are as wide, so it is as wide.
        let tb = (band_y * 0.85).max(6.0);
        let lr = (band_x / (GLYPH_WIDTH * chars) * 0.9)
            .min(band_y * 2.5)
            .max(6.0);
        for along in SMALL_ALONG {
            push(w * along, dy + band_y / 2.0, tb);
            push(w * along, h - dy - band_y / 2.0, tb);
            push(dx + band_x / 2.0, h * along, lr);
            push(w - dx - band_x / 2.0, h * along, lr);
        }

        // Large: every fifth ring, anchored just inside the ring.
        if n.is_multiple_of(5) {
            let (bw, bh) = (chars * big_size * GLYPH_WIDTH, big_size);
            let gap = line * 2.0;
            for along in BIG_ALONG_HORIZONTAL {
                push(dx + (w - 2.0 * dx) * along, dy + gap + bh / 2.0, big_size);
                push(
                    dx + (w - 2.0 * dx) * along,
                    h - dy - gap - bh / 2.0,
                    big_size,
                );
            }
            for along in BIG_ALONG_VERTICAL {
                push(dx + gap + bw / 2.0, dy + (h - 2.0 * dy) * along, big_size);
                push(
                    w - dx - gap - bw / 2.0,
                    dy + (h - 2.0 * dy) * along,
                    big_size,
                );
            }
        }
    }
    out
}

fn ring_numbers(b: &mut Builder, w: f32, h: f32, max: u32) {
    for number in numbers(w, h, max) {
        let text = number.ring.to_string();
        b.text(number.rect, &text, number.size, number_color(number.ring));
    }
}

/// The safe area as currently set: a cyan outline, each margin labelled
/// just inside its edge, and the edge being adjusted drawn in white with its label enlarged.
fn safe_area(b: &mut Builder, pattern: &CalibrationPattern, w: f32, h: f32, line: f32) {
    let area = pattern.current;
    let r = area.rect(w, h);
    let stroke = line * 2.0;
    b.stroke_rect(
        Rect {
            x: r.x + line,
            y: r.y + line,
            width: r.width - 2.0 * line,
            height: r.height - 2.0 * line,
        },
        SAFE,
        stroke,
    );
    let (right, bottom) = (r.x + r.width, r.y + r.height);
    let base = h * 0.0195;
    for edge in Edge::ALL {
        let active = pattern.active == Some(edge);
        let (color, size) = if active {
            (ACTIVE, base * 1.6)
        } else {
            (SAFE, base)
        };
        if active {
            // Drawn over the cyan in white. Not thicker: it would hide the number of
            // the ring it sits on.
            let t = stroke;
            let (a, z) = match edge {
                Edge::Top => (p(r.x, r.y + t / 2.0), p(right, r.y + t / 2.0)),
                Edge::Bottom => (p(r.x, bottom - t / 2.0), p(right, bottom - t / 2.0)),
                Edge::Left => (p(r.x + t / 2.0, r.y), p(r.x + t / 2.0, bottom)),
                Edge::Right => (p(right - t / 2.0, r.y), p(right - t / 2.0, bottom)),
            };
            b.line(a, z, ACTIVE, t);
        }
        let text = format!("{} {:.1}%", edge.name(), edge.margin(&area) * 100.0);
        // Clear of the ring numbers: the middle of the edge, past
        // the thick line when it is the active one.
        let off = size * 1.2 + stroke;
        let (cx, cy) = match edge {
            Edge::Top => (r.x + r.width * 0.5, r.y + off),
            Edge::Bottom => (r.x + r.width * 0.5, bottom - off),
            Edge::Left => (
                r.x + off + text.chars().count() as f32 * size * GLYPH_WIDTH / 2.0,
                r.y + r.height * 0.5,
            ),
            Edge::Right => (
                right - off - text.chars().count() as f32 * size * GLYPH_WIDTH / 2.0,
                r.y + r.height * 0.5,
            ),
        };
        // A dark box behind the label so it reads over the ring lines.
        let rect = label_rect(cx, cy, &text, size);
        let pad = size * 0.25;
        b.fill_rect(
            Rect {
                x: rect.x - pad,
                y: rect.y - pad * 0.5,
                width: rect.width + 2.0 * pad,
                height: rect.height + pad,
            },
            Color::BLACK,
        );
        b.text(rect, &text, size, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pattern(area: SafeArea, active: Option<Edge>, max: u8) -> CalibrationPattern {
        CalibrationPattern {
            current: area,
            max_percent: max,
            active,
            label: "Main".into(),
        }
    }

    #[test]
    fn names_common_aspects() {
        assert_eq!(aspect_name(1024, 768), "4:3");
        assert_eq!(aspect_name(1920, 1080), "16:9");
        assert_eq!(aspect_name(1920, 1200), "16:10");
        assert_eq!(aspect_name(5120, 1440), "32:9");
        assert_eq!(aspect_name(1000, 100), "10.00:1");
    }

    fn texts(scene: &Scene) -> Vec<String> {
        scene
            .nodes
            .iter()
            .filter_map(|n| match &n.kind {
                NodeKind::Text(t) => Some(
                    t.paragraphs
                        .iter()
                        .flat_map(|p| p.runs.iter().map(|r| r.text.clone()))
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn caption_names_the_output_and_aspect() {
        let s = scene(&pattern(SafeArea::default(), None, 10), 3200.0, 900.0);
        let t = texts(&s);
        assert!(t.iter().any(|t| t.starts_with("Main ")));
        assert!(t.iter().any(|t| t.contains("32:9")), "{t:?}");
        let s = scene(&pattern(SafeArea::default(), None, 10), 1920.0, 1080.0);
        assert!(texts(&s).iter().any(|t| t.contains("1920 × 1080  16:9")));
    }

    #[test]
    fn every_ring_has_a_number_on_every_edge() {
        for (w, h) in [(1600.0, 900.0), (1200.0, 900.0), (3200.0, 900.0)] {
            let all = numbers(w, h, 10);
            for n in 0..=10 {
                let small = all
                    .iter()
                    .filter(|x| x.ring == n && x.size < h * 0.03)
                    .count();
                assert!(small >= 8, "{w}x{h} ring {n}: {small} small numbers");
            }
            // Large numbers on the fives only: 4 edges × 2 each.
            for n in 0..=10 {
                let big = all
                    .iter()
                    .filter(|x| x.ring == n && x.size >= h * 0.03 - 0.01)
                    .count();
                assert_eq!(big, if n.is_multiple_of(5) { 8 } else { 0 }, "ring {n}");
            }
        }
    }

    #[test]
    fn number_n_is_entirely_inside_ring_n() {
        for (w, h) in [(1024.0, 768.0), (1920.0, 1080.0), (3200.0, 900.0)] {
            for number in numbers(w, h, 15) {
                let (dx, dy) = ring_inset(number.ring, w, h);
                let r = number.rect;
                let inside = r.x >= dx - 0.01
                    && r.y >= dy - 0.01
                    && r.x + r.width <= w - dx + 0.01
                    && r.y + r.height <= h - dy + 0.01;
                assert!(inside, "{w}x{h} ring {}: {r:?}", number.ring);
            }
        }
    }

    fn overlaps(a: Rect, b: Rect) -> bool {
        a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
    }

    #[test]
    fn numbers_do_not_overlap_each_other() {
        for (w, h) in [
            (1024.0, 768.0),
            (1920.0, 1080.0),
            (3200.0, 900.0),
            (5760.0, 1080.0),
        ] {
            for max in [10, 15, 20] {
                let all = numbers(w, h, max);
                for (i, a) in all.iter().enumerate() {
                    for b in &all[i + 1..] {
                        assert!(
                            !overlaps(a.rect, b.rect),
                            "{w}x{h} max {max}: ring {} {:?} meets ring {} {:?}",
                            a.ring,
                            a.rect,
                            b.ring,
                            b.rect
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn small_numbers_are_legible_at_1080p() {
        let all = numbers(1920.0, 1080.0, 10);
        let smallest = all.iter().map(|n| n.size).fold(f32::MAX, f32::min);
        assert!(smallest >= 8.0, "smallest number is {smallest} px");
        let biggest = all.iter().map(|n| n.size).fold(0.0, f32::max);
        assert!(biggest >= 30.0, "large numbers are {biggest} px");
    }

    #[test]
    fn node_ids_are_unique_and_clear_of_content() {
        let s = scene(
            &pattern(SafeArea::default(), Some(Edge::Top), 15),
            1920.0,
            1080.0,
        );
        let mut ids: Vec<u32> = s.nodes.iter().map(|n| n.id).collect();
        assert!(ids.iter().all(|&id| id >= 1 << 24));
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), s.nodes.len());
    }

    #[test]
    fn max_percent_is_clamped_to_rings_that_do_not_cross() {
        let s = scene(&pattern(SafeArea::default(), None, 255), 1920.0, 1080.0);
        assert!(!s.nodes.is_empty());
        assert!(numbers(1920.0, 1080.0, 255)
            .iter()
            .all(|n| n.ring <= MAX_RING));
        assert!(numbers(1920.0, 1080.0, 0).iter().all(|n| n.ring <= 1));
    }
}
