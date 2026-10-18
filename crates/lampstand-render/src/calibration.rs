//! Calibration pattern (TS-1): numbered rings inset from every edge, corner
//! markers, and the current safe area, drawn on the output itself.
//!
//! Ring `n` is drawn `n` percent in from each edge and its number sits just
//! inside it. A volunteer at the back reads the smallest number they can see
//! fully on each edge; that number is the margin, in percent, to set for that
//! edge, and the cyan outline shows the safe area as it is now.

use lampstand_core::safe_area::{Rect, SafeArea};

use crate::scene::Color;

/// Distance between rings, in percent of the output's width or height.
pub const STEP_PERCENT: u32 = 2;
/// Innermost ring, in percent.
pub const MAX_PERCENT: u32 = 14;

pub const BACKGROUND: Color = Color::rgb(24, 24, 24);
pub const GRID: Color = Color::rgb(70, 70, 70);
pub const EDGE: Color = Color::rgb(255, 40, 40);
pub const RING_A: Color = Color::rgb(255, 255, 255);
pub const RING_B: Color = Color::rgb(255, 210, 0);
pub const SAFE: Color = Color::rgb(0, 229, 255);
/// Top-left, top-right, bottom-right, bottom-left.
pub const CORNERS: [Color; 4] = [
    Color::rgb(255, 40, 40),
    Color::rgb(40, 220, 40),
    Color::rgb(60, 110, 255),
    Color::rgb(255, 60, 255),
];

#[derive(Debug, Clone, PartialEq)]
pub struct Quad {
    pub rect: Rect,
    pub color: Color,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub text: String,
    /// Centre of the label, in pixels.
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub color: Color,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pattern {
    pub quads: Vec<Quad>,
    pub labels: Vec<Label>,
}

/// Line thickness for an output: 2 px at 1080p, never under 1 px.
pub fn line_width(width: f32, height: f32) -> f32 {
    (width.min(height) / 540.0).round().max(1.0)
}

/// The four sides of `rect` as quads `t` pixels thick, drawn inside it.
pub fn outline(rect: Rect, t: f32, color: Color) -> [Quad; 4] {
    let Rect { x, y, width: w, height: h } = rect;
    [
        Quad { rect: Rect { x, y, width: w, height: t }, color },
        Quad { rect: Rect { x, y: y + h - t, width: w, height: t }, color },
        Quad { rect: Rect { x, y, width: t, height: h }, color },
        Quad { rect: Rect { x: x + w - t, y, width: t, height: h }, color },
    ]
}

/// Common names for an aspect ratio, so the volunteer can check the output
/// mode is what they expect.
pub fn aspect_name(width: u32, height: u32) -> String {
    let ratio = width as f32 / height as f32;
    let named = [(4.0 / 3.0, "4:3"), (16.0 / 9.0, "16:9"), (16.0 / 10.0, "16:10"), (21.0 / 9.0, "21:9"), (32.0 / 9.0, "32:9")];
    named
        .iter()
        .find(|(r, _)| (ratio - r).abs() < 0.02)
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| format!("{ratio:.2}:1"))
}

pub fn pattern(width: u32, height: u32, safe_area: &SafeArea, label: &str) -> Pattern {
    let (w, h) = (width as f32, height as f32);
    let t = line_width(w, h);
    let mut p = Pattern::default();
    let full = Rect { x: 0.0, y: 0.0, width: w, height: h };
    p.quads.push(Quad { rect: full, color: BACKGROUND });

    // A coarse 10% grid, to spot keystone and scaling at a glance.
    for i in 1..10 {
        let f = i as f32 / 10.0;
        p.quads.push(Quad { rect: Rect { x: w * f - t / 2.0, y: 0.0, width: t, height: h }, color: GRID });
        p.quads.push(Quad { rect: Rect { x: 0.0, y: h * f - t / 2.0, width: w, height: t }, color: GRID });
    }

    // Numbered rings. Ring 0 is the very edge of the picture.
    let label_size = (h * STEP_PERCENT as f32 / 100.0 * 0.75).max(8.0);
    for (i, pct) in (0..=MAX_PERCENT).step_by(STEP_PERCENT as usize).enumerate() {
        let f = pct as f32 / 100.0;
        let ring = Rect { x: w * f, y: h * f, width: w * (1.0 - 2.0 * f), height: h * (1.0 - 2.0 * f) };
        let color = match (pct, i % 2) {
            (0, _) => EDGE,
            (_, 0) => RING_A,
            _ => RING_B,
        };
        let thickness = if pct == 0 { 2.0 * t } else { t };
        p.quads.extend(outline(ring, thickness, color));

        // Number centred in the band just inside the ring, at a quarter,
        // half and three quarters along each edge.
        let band = (pct as f32 + STEP_PERCENT as f32 / 2.0) / 100.0;
        let text = pct.to_string();
        for along in [0.25, 0.5, 0.75] {
            for (x, y) in [
                (w * along, h * band),
                (w * along, h * (1.0 - band)),
                (w * band, h * along),
                (w * (1.0 - band), h * along),
            ] {
                p.labels.push(Label { text: text.clone(), x, y, size: label_size, color });
            }
        }
    }

    // Corner markers: an L in a different colour at each corner, so a
    // cropped or mirrored picture is obvious.
    let arm = w.min(h) * 0.08;
    let thick = (w.min(h) * 0.012).max(2.0);
    let corners = [(0.0, 0.0, 1.0, 1.0), (w, 0.0, -1.0, 1.0), (w, h, -1.0, -1.0), (0.0, h, 1.0, -1.0)];
    for ((cx, cy, dx, dy), color) in corners.into_iter().zip(CORNERS) {
        let horizontal = Rect { x: if dx > 0.0 { cx } else { cx - arm }, y: if dy > 0.0 { cy } else { cy - thick }, width: arm, height: thick };
        let vertical = Rect { x: if dx > 0.0 { cx } else { cx - thick }, y: if dy > 0.0 { cy } else { cy - arm }, width: thick, height: arm };
        p.quads.push(Quad { rect: horizontal, color });
        p.quads.push(Quad { rect: vertical, color });
    }
    for ((name, (x, y)), color) in ["TL", "TR", "BR", "BL"]
        .into_iter()
        .zip([(0.2, 0.2), (0.8, 0.2), (0.8, 0.8), (0.2, 0.8)])
        .zip(CORNERS)
    {
        p.labels.push(Label { text: name.into(), x: w * x, y: h * y, size: label_size * 1.5, color });
    }

    // The safe area as currently set.
    p.quads.extend(outline(safe_area.rect(w, h), 2.0 * t, SAFE));

    // Centre cross and a caption naming the output and its mode.
    let cross = w.min(h) * 0.05;
    p.quads.push(Quad { rect: Rect { x: w / 2.0 - cross, y: h / 2.0 - t / 2.0, width: 2.0 * cross, height: t }, color: RING_A });
    p.quads.push(Quad { rect: Rect { x: w / 2.0 - t / 2.0, y: h / 2.0 - cross, width: t, height: 2.0 * cross }, color: RING_A });
    let caption = format!("{label}\n{width} × {height}  {}", aspect_name(width, height));
    // Labels are centred on their point, so push the block down by half its
    // height to keep a multi-line caption clear of the cross.
    let size = h * 0.035;
    let lines = caption.lines().count() as f32;
    let y = h / 2.0 + cross + size + lines * size * 1.2 / 2.0;
    p.labels.push(Label { text: caption, x: w / 2.0, y, size, color: RING_A });
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_common_aspects() {
        assert_eq!(aspect_name(1024, 768), "4:3");
        assert_eq!(aspect_name(1920, 1080), "16:9");
        assert_eq!(aspect_name(1920, 1200), "16:10");
        assert_eq!(aspect_name(5120, 1440), "32:9");
        assert_eq!(aspect_name(1000, 100), "10.00:1");
    }

    #[test]
    fn rings_are_numbered_by_percent_on_every_edge() {
        let p = pattern(1920, 1080, &SafeArea::default(), "Main");
        for n in (0..=MAX_PERCENT).step_by(STEP_PERCENT as usize) {
            let count = p.labels.iter().filter(|l| l.text == n.to_string()).count();
            assert_eq!(count, 12, "ring {n}: 3 labels on each of 4 edges");
        }
        // The "4" labels on the left edge sit in the band from 4% to 6%.
        let left: Vec<_> = p.labels.iter().filter(|l| l.text == "4" && l.x < 200.0).collect();
        assert!(left.iter().all(|l| (l.x - 1920.0 * 0.05).abs() < 0.01));
    }

    #[test]
    fn everything_is_on_screen() {
        for (w, h) in [(1024, 768), (1920, 1080), (1920, 1200), (5760, 1080)] {
            let p = pattern(w, h, &SafeArea { top: 0.05, right: 0.05, bottom: 0.1, left: 0.05 }, "x");
            let screen = Rect { x: 0.0, y: 0.0, width: w as f32, height: h as f32 };
            for q in &p.quads {
                assert!(crate::layout::contains(screen, q.rect), "{w}x{h}: {q:?}");
            }
        }
    }
}
