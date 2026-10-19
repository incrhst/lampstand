//! Scenes the render thread draws on its own: the safe-area outline (TS-8)
//! and the calibration pattern (TS-1).
//!
//! Both are ordinary `Scene`s in the output's pixels, drawn into their own
//! texture and composited over (safe area) or instead of (calibration) the
//! live scene, so they need no renderer support of their own.

use lampstand_core::geom::{Color, Point, Rect, Transform};
use lampstand_core::safe_area::SafeArea;
use lampstand_core::scene::{Node, NodeKind, Paint, PathCommand, PathNode, Scene, Stroke};

use crate::CalibrationPattern;

/// Node IDs for overlay scenes, clear of content and compositor IDs.
const OVERLAY_NODE_BASE: u32 = u32::MAX - 1024;

fn rect_node(id: u32, r: Rect, fill: Option<Color>, stroke: Option<Stroke>) -> Node {
    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.width, r.y + r.height);
    Node {
        id,
        transform: Transform::IDENTITY,
        opacity: 1.0,
        visible: true,
        kind: NodeKind::Path(PathNode {
            commands: vec![
                PathCommand::MoveTo(Point { x: x0, y: y0 }),
                PathCommand::LineTo(Point { x: x1, y: y0 }),
                PathCommand::LineTo(Point { x: x1, y: y1 }),
                PathCommand::LineTo(Point { x: x0, y: y1 }),
                PathCommand::Close,
            ],
            fill: fill.map(Paint::Solid),
            stroke,
            shadow: None,
        }),
    }
}

/// A dashed outline of the safe area, two pixels wide at 1080p, for
/// preview panes and rehearsal windows (TS-8).
pub fn safe_area_scene(area: &SafeArea, width: f32, height: f32) -> Scene {
    let r = area.rect(width, height);
    let line = (height / 540.0).max(1.0);
    let dash = Stroke {
        paint: Paint::Solid(Color {
            r: 255,
            g: 214,
            b: 0,
            a: 230,
        }),
        width: line,
        dash: Some(vec![6.0 * line, 4.0 * line]),
    };
    Scene {
        width,
        height,
        nodes: vec![rect_node(OVERLAY_NODE_BASE, r, None, Some(dash))],
    }
}

/// The calibration pattern for an output of `width` × `height` pixels.
///
/// This is the hook T26 fills in with numbered edges at 1% steps and
/// corner markers. Until then it draws enough to calibrate by eye: a black
/// screen with the current margins as a bright outline.
pub fn calibration_scene(pattern: &CalibrationPattern, width: f32, height: f32) -> Scene {
    let line = (height / 360.0).max(1.0);
    let outline = Stroke {
        paint: Paint::Solid(Color::WHITE),
        width: line,
        dash: None,
    };
    let full = Rect {
        x: 0.0,
        y: 0.0,
        width,
        height,
    };
    Scene {
        width,
        height,
        nodes: vec![
            rect_node(OVERLAY_NODE_BASE + 1, full, Some(Color::BLACK), None),
            rect_node(
                OVERLAY_NODE_BASE + 2,
                pattern.current.rect(width, height),
                None,
                Some(outline),
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_area_outline_sits_on_the_margins() {
        let area = SafeArea {
            top: 0.1,
            right: 0.05,
            bottom: 0.1,
            left: 0.05,
        };
        let scene = safe_area_scene(&area, 1920.0, 1080.0);
        let NodeKind::Path(p) = &scene.nodes[0].kind else {
            panic!("expected a path");
        };
        assert_eq!(
            p.commands[0],
            PathCommand::MoveTo(Point { x: 96.0, y: 108.0 })
        );
        assert!(p.fill.is_none());
        assert_eq!(p.stroke.as_ref().unwrap().width, 2.0);
    }

    #[test]
    fn calibration_covers_the_screen() {
        let pattern = CalibrationPattern {
            current: SafeArea::default(),
            max_percent: 10,
        };
        let scene = calibration_scene(&pattern, 1280.0, 720.0);
        assert_eq!((scene.width, scene.height), (1280.0, 720.0));
        let NodeKind::Path(p) = &scene.nodes[0].kind else {
            panic!("expected a path");
        };
        assert_eq!(p.fill, Some(Paint::Solid(Color::BLACK)));
    }
}
