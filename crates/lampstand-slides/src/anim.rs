//! Where a slide stands after some build steps, without DeckCraft: the
//! end state of each effect applied to the scene, for `scene_at_step`.
//!
//! An entrance leaves its node visible and an exit hidden; Spin and
//! Grow/Shrink leave a rotation or scale about the centre of the node's
//! bounds (where the renderer pivots them); Transparency leaves the node
//! that much see-through. Pulse and Play leave nothing behind.

use lampstand_core::geom::{Point, Rect, Transform};
use lampstand_core::scene::{Animation, Effect, Node, NodeKind, PathCommand, Scene};

/// `scene` with every step in `steps` played to its end, in order.
pub(crate) fn after<'a>(scene: &Scene, steps: impl IntoIterator<Item = &'a [Animation]>) -> Scene {
    let mut out = scene.clone();
    for step in steps {
        // Within a step the effect that ends last decides; ties keep the
        // step's order (the Animation Pane's).
        let mut order: Vec<&Animation> = step.iter().collect();
        order.sort_by_key(|a| a.delay_ms.saturating_add(a.duration_ms));
        for a in order {
            apply(&mut out.nodes, a);
        }
    }
    out
}

fn apply(nodes: &mut [Node], a: &Animation) {
    for n in nodes {
        if n.id == a.node {
            finish(n, a.effect);
        }
        if let NodeKind::Group(kids) = &mut n.kind {
            apply(kids, a);
        }
    }
}

fn finish(n: &mut Node, effect: Effect) {
    match effect {
        Effect::Appear | Effect::FadeIn | Effect::FlyIn(_) | Effect::WipeIn(_) | Effect::ZoomIn => {
            n.visible = true
        }
        Effect::Disappear
        | Effect::FadeOut
        | Effect::FlyOut(_)
        | Effect::WipeOut(_)
        | Effect::ZoomOut => n.visible = false,
        Effect::Spin(degrees) => pivot(n, 1.0, degrees),
        Effect::GrowShrink(factor) => pivot(n, factor, 0.0),
        Effect::Transparency(value) => n.opacity *= (1.0 - value).clamp(0.0, 1.0),
        Effect::Pulse | Effect::Play => {}
    }
}

/// Scales and turns `n` (clockwise, in degrees) about the centre of its
/// bounds as placed in its parent.
fn pivot(n: &mut Node, scale: f32, degrees: f32) {
    if !scale.is_finite() || !degrees.is_finite() {
        return;
    }
    let Some(b) = bounds(n) else { return };
    let b = aabb(b, n.transform);
    let (cx, cy) = (b.x + b.width / 2.0, b.y + b.height / 2.0);
    let (sin, cos) = degrees.to_radians().sin_cos();
    let turn = Transform {
        a: cos,
        b: sin,
        c: -sin,
        d: cos,
        e: 0.0,
        f: 0.0,
    };
    n.transform = n.transform.then(
        Transform::translate(-cx, -cy)
            .then(Transform::scale(scale, scale))
            .then(turn)
            .then(Transform::translate(cx, cy)),
    );
}

/// The node's extent in its own space, hidden children included (the
/// renderer's rule, so a pivot does not move as a build reveals parts).
fn bounds(node: &Node) -> Option<Rect> {
    match &node.kind {
        NodeKind::Group(kids) => kids
            .iter()
            .filter_map(|k| bounds(k).map(|r| aabb(r, k.transform)))
            .reduce(union),
        NodeKind::Path(p) => {
            let points: Vec<Point> = p
                .commands
                .iter()
                .flat_map(|c| match *c {
                    PathCommand::MoveTo(a) | PathCommand::LineTo(a) => vec![a],
                    PathCommand::QuadTo(a, b) => vec![a, b],
                    PathCommand::CubicTo(a, b, c) => vec![a, b, c],
                    PathCommand::Close => vec![],
                })
                .collect();
            span(&points)
        }
        NodeKind::Text(t) => Some(t.rect),
        NodeKind::Image(i) => Some(i.rect),
        NodeKind::Video(v) => Some(v.rect),
    }
}

fn aabb(r: Rect, t: Transform) -> Rect {
    let corners = [
        (r.x, r.y),
        (r.x + r.width, r.y),
        (r.x, r.y + r.height),
        (r.x + r.width, r.y + r.height),
    ]
    .map(|(x, y)| t.apply(Point { x, y }));
    span(&corners).unwrap_or(r)
}

fn span(points: &[Point]) -> Option<Rect> {
    let first = points.first()?;
    let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x, first.y);
    for p in points {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    Some(Rect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

fn union(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.width).max(b.x + b.width);
    let y1 = (a.y + a.height).max(b.y + b.height);
    Rect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lampstand_core::geom::Color;
    use lampstand_core::scene::{Direction, Easing, Paint, PathNode};

    fn square(id: u32, visible: bool) -> Node {
        let p = |x, y| Point { x, y };
        Node {
            id,
            transform: Transform::translate(100.0, 0.0),
            opacity: 1.0,
            visible,
            kind: NodeKind::Path(PathNode {
                commands: vec![
                    PathCommand::MoveTo(p(0.0, 0.0)),
                    PathCommand::LineTo(p(10.0, 0.0)),
                    PathCommand::LineTo(p(10.0, 10.0)),
                    PathCommand::LineTo(p(0.0, 10.0)),
                    PathCommand::Close,
                ],
                fill: Some(Paint::Solid(Color::BLACK)),
                stroke: None,
                shadow: None,
            }),
        }
    }

    fn anim(node: u32, effect: Effect, delay_ms: u32) -> Animation {
        Animation {
            node,
            effect,
            delay_ms,
            duration_ms: 500,
            easing: Easing::Linear,
        }
    }

    fn scene(nodes: Vec<Node>) -> Scene {
        Scene {
            width: 960.0,
            height: 540.0,
            nodes,
        }
    }

    #[test]
    fn entrances_show_and_exits_hide() {
        let s = scene(vec![square(1, false), square(2, true)]);
        let steps = [
            vec![anim(1, Effect::FlyIn(Direction::Left), 0)],
            vec![anim(2, Effect::FadeOut, 0)],
        ];
        let one = after(&s, steps.iter().take(1).map(Vec::as_slice));
        assert!(one.nodes[0].visible && one.nodes[1].visible);
        let two = after(&s, steps.iter().map(Vec::as_slice));
        assert!(two.nodes[0].visible && !two.nodes[1].visible);
    }

    #[test]
    fn the_effect_that_ends_last_wins_within_a_step() {
        let s = scene(vec![square(1, false)]);
        // Listed exit first, but it ends before the entrance.
        let step = [anim(1, Effect::FadeIn, 1000), anim(1, Effect::FadeOut, 0)];
        let out = after(&s, [&step[..]]);
        assert!(out.nodes[0].visible);
    }

    #[test]
    fn emphasis_stays_about_the_centre() {
        let s = scene(vec![square(1, true)]);
        let out = after(
            &s,
            [&[
                anim(1, Effect::GrowShrink(2.0), 0),
                anim(1, Effect::Transparency(0.75), 0),
            ][..]],
        );
        let n = &out.nodes[0];
        // The 10 pt square at x 100 doubles about its centre (105, 5).
        let corner = n.transform.apply(Point { x: 0.0, y: 0.0 });
        assert_eq!((corner.x, corner.y), (95.0, -5.0));
        assert_eq!(n.opacity, 0.25);
    }
}
