//! One DeckCraft shape (and its children) to scene nodes.

use deckcraft_geom::preset::{FillMode, Geometry};
use deckcraft_geom::{Affine, Xfrm};
use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::style::{Fill, Line, LineEnd, PictureFill, PictureMode};
use deckcraft_model::{Geom, MediaId, Shape, ShapeKind};
use lampstand_core::geom::{Color, Point, Rect, Transform};
use lampstand_core::scene::{
    Fit, ImageNode, Node, NodeKind, Paint, PathCommand, PathNode, Stroke, VideoNode,
};

use super::geometry::{self, commands};
use super::text::{self, Frame};
use super::{paint, table, SlideCx};
use crate::IssueKind;

/// Deeper nesting than any real deck; a hostile file stops here.
const MAX_DEPTH: usize = 32;

pub(crate) fn transform(a: Affine) -> Transform {
    let [a, b, c, d, e, f] = a.as_coeffs().map(|v| v as f32);
    Transform { a, b, c, d, e, f }
}

/// A shape's local frame: the size it has on the slide, in points, and the
/// transform from that box to the slide. A group's child-coordinate scale
/// sizes the box, but outline widths, insets and type stay in points (as
/// PowerPoint and DeckCraft draw them).
pub(crate) struct Local {
    pub w: f32,
    pub h: f32,
    pub transform: Transform,
}

impl Local {
    fn new(x: &Xfrm, parent: Transform) -> Option<Self> {
        let m = transform(x.affine()).then(parent);
        let sx = m.a.hypot(m.b);
        let sy = m.c.hypot(m.d);
        let ok = |k: f32| k.is_finite() && k > 1e-6;
        let (sx, sy) = (if ok(sx) { sx } else { 1.0 }, if ok(sy) { sy } else { 1.0 });
        let (w, h) = (x.w as f32 * sx, x.h as f32 * sy);
        if !(w.is_finite() && h.is_finite()) {
            return None;
        }
        Some(Self {
            w: w.max(0.0),
            h: h.max(0.0),
            transform: Transform::scale(1.0 / sx, 1.0 / sy).then(m),
        })
    }

    pub(crate) fn rect(&self) -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            width: self.w,
            height: self.h,
        }
    }
}

pub(crate) fn node(id: u32, transform: Transform, kind: NodeKind) -> Node {
    Node {
        id,
        transform,
        opacity: 1.0,
        visible: true,
        kind,
    }
}

impl SlideCx<'_, '_> {
    /// Maps `s`, placed by `parent` (its parent's child space → slide),
    /// onto the end of `self.nodes`.
    pub(crate) fn shape(&mut self, ctx: &Ctx, s: &Shape, parent: Transform, depth: usize) {
        if depth > MAX_DEPTH || s.hidden {
            return;
        }
        let x = resolve::xfrm(ctx, s);
        if !x.is_finite() {
            return;
        }
        match &s.kind {
            ShapeKind::Group { children, child } => {
                let ch = deckcraft_geom::Rect::new(
                    child.x,
                    child.y,
                    child.x + child.w,
                    child.y + child.h,
                );
                let inner = transform(deckcraft_geom::group_child_affine(&x, ch)).then(parent);
                let id = self.shape_id(Some(s.id.0));
                // `grpFill` children take the group's fill.
                let fill = match &s.fill {
                    Some(Fill::Group) | None => self.group_fill.clone(),
                    Some(f) => Some(f.clone()),
                };
                let outer = std::mem::replace(&mut self.group_fill, fill);
                let before = self.nodes.len();
                for c in children {
                    self.shape(ctx, c, inner, depth + 1);
                }
                self.group_fill = outer;
                let kids = self.nodes.split_off(before);
                // Children carry slide transforms; the group node holds them
                // so a build can move or fade the group as one.
                self.nodes
                    .push(node(id, Transform::IDENTITY, NodeKind::Group(kids)));
            }
            _ => {
                let Some(local) = Local::new(&x, parent) else {
                    return;
                };
                self.leaf(ctx, s, &local, parent);
            }
        }
    }

    fn leaf(&mut self, ctx: &Ctx, s: &Shape, local: &Local, parent: Transform) {
        match &s.kind {
            ShapeKind::Table(t) => {
                let id = self.shape_id(Some(s.id.0));
                let kids = table::nodes(self, ctx, s, t, local);
                self.nodes
                    .push(node(id, local.transform, NodeKind::Group(kids)));
            }
            ShapeKind::Chart(_) => self.issue(IssueKind::UnsupportedContent {
                what: "chart".into(),
            }),
            ShapeKind::Ink { strokes } => {
                let id = self.shape_id(Some(s.id.0));
                // Ink points are in the parent's space, not the shape's box.
                let kids = self.ink(strokes);
                self.nodes.push(node(id, parent, NodeKind::Group(kids)));
            }
            ShapeKind::Opaque { preview, label, .. } => match preview {
                Some(m) => self.picture_node(s, *m, local, None),
                None => self.issue(IssueKind::UnsupportedContent {
                    what: if label.is_empty() {
                        "embedded object".into()
                    } else {
                        label.clone()
                    },
                }),
            },
            ShapeKind::Model3d { poster, .. } => {
                self.issue(IssueKind::UnsupportedContent {
                    what: "3D model".into(),
                });
                if let Some(p) = poster {
                    self.picture_node(s, *p, local, None);
                }
            }
            ShapeKind::Media(clip) => {
                // Extract the clip even when nothing on the slide shows it
                // (audio), so the player can find it.
                let blob = self.deck.blob(self.index, clip.media);
                match (clip.video, blob) {
                    (true, Some(blob)) => {
                        // The poster frame shows until the video plays (and
                        // wherever video cannot be decoded), so it goes under it.
                        if let Some(img) = clip.poster.and_then(|p| {
                            self.image(
                                &PictureFill {
                                    media: p,
                                    ..Default::default()
                                },
                                local,
                            )
                        }) {
                            let id = self.fresh_id();
                            self.nodes.push(node(id, local.transform, img));
                        }
                        let id = self.shape_id(Some(s.id.0));
                        self.nodes.push(node(
                            id,
                            local.transform,
                            NodeKind::Video(VideoNode {
                                blob,
                                rect: local.rect(),
                                fit: Fit::Fill,
                                looped: clip.loop_play,
                                muted: clip.volume <= 0.0,
                                autoplay: clip.autoplay,
                            }),
                        ));
                    }
                    _ => {
                        if let Some(p) = clip.poster {
                            self.picture_node(s, p, local, None);
                        }
                    }
                }
            }
            ShapeKind::Picture { fill } => {
                let (line, line_ph) = resolve::line(ctx, s);
                let outline = paint::stroke(ctx, &line, line_ph, local.w, local.h).map(|st| {
                    let (g, _) = geometry::geometry(s, local.w as f64, local.h as f64);
                    (outline_commands(&g), st)
                });
                let pf = fill.clone();
                self.picture(s, &pf, local, outline);
            }
            ShapeKind::Shape | ShapeKind::Connector { .. } => self.drawn_shape(ctx, s, local),
            // Mapped by `shape`.
            ShapeKind::Group { .. } => {}
        }
    }

    /// An autoshape, text box, placeholder or connector: its outline,
    /// filled and stroked, with its text on top.
    fn drawn_shape(&mut self, ctx: &Ctx, s: &Shape, local: &Local) {
        let (w, h) = (local.w, local.h);
        let (geo, unknown) = geometry::geometry(s, w as f64, h as f64);
        if unknown {
            if let Geom::Preset { name, .. } = &s.geom {
                self.issue(IssueKind::UnsupportedContent {
                    what: format!("preset shape \"{name}\", drawn as its bounding rectangle"),
                });
            }
        }
        let (fill, fill_ph) = resolve::fill(ctx, s);
        let (line, line_ph) = resolve::line(ctx, s);
        let (effects, fx_ph) = resolve::effects(ctx, s);
        let fill = match fill {
            Some(Fill::Group) => self.group_fill.clone(),
            f => f,
        };
        let shadow = effects
            .as_ref()
            .and_then(|e| e.outer_shadow.as_ref())
            .and_then(|sh| paint::shadow(ctx, sh, fx_ph, local.transform));

        let mut kids: Vec<Node> = Vec::new();
        let filled = fill.as_ref().is_some_and(|f| !f.is_none());
        let stroke = paint::stroke(ctx, &line, line_ph, w, h);

        // A picture fill: the picture stretched over the box (not clipped
        // to the outline), under the outline.
        if let Some(Fill::Picture(pf)) = &fill {
            if let Some(img) = self.image(pf, local) {
                let id = self.fresh_id();
                kids.push(node(id, Transform::IDENTITY, img));
            }
        }
        let base = match &fill {
            Some(Fill::Background) => self.background.clone(),
            Some(f) => paint::paint(ctx, f, fill_ph, w, h),
            None => None,
        };
        // Lines, connectors and open outlines get arrowheads, and their line
        // stops short under a head that would otherwise show its butt end.
        let open = s.is_line()
            || geo.is_open()
            || (matches!(s.geom, Geom::Custom { .. })
                && geo.paths.iter().find(|p| p.stroke).is_some_and(|p| {
                    !p.path
                        .elements()
                        .iter()
                        .any(|e| matches!(e, deckcraft_geom::PathEl::ClosePath))
                }));
        let heads = match (&stroke, open) {
            (Some(st), true) => arrowheads(&line, &geo, st.width),
            _ => Vec::new(),
        };
        let trims = match (&stroke, open) {
            (Some(st), true) => head_trims(&line, st.width),
            _ => (0.0, 0.0),
        };
        let mut trimmed = false;
        let mut shadow_left = shadow;
        for sp in &geo.paths {
            let mut cmds = commands(&sp.path);
            if cmds.is_empty() {
                continue;
            }
            let fill = match (&base, sp.fill) {
                (_, FillMode::None) => None,
                (Some(p), mode) => Some(paint::shade_paint(p.clone(), mode)),
                (None, _) => None,
            };
            let stroke = if sp.stroke { stroke.clone() } else { None };
            if fill.is_none() && stroke.is_none() {
                continue;
            }
            // Heads go on the first stroked sub-path.
            if stroke.is_some() && !trimmed {
                trimmed = true;
                if fill.is_none() {
                    geometry::trim_ends(&mut cmds, trims.0, trims.1);
                }
            }
            // The shape casts its shadow once, from its first drawn part.
            let shadow = if fill.is_some() || !filled {
                shadow_left.take()
            } else {
                None
            };
            let id = self.fresh_id();
            kids.push(node(
                id,
                Transform::IDENTITY,
                NodeKind::Path(PathNode {
                    commands: cmds,
                    fill,
                    stroke,
                    shadow,
                }),
            ));
        }
        if !heads.is_empty() {
            let color = paint::line_color(ctx, &line, line_ph).unwrap_or(Color::BLACK);
            for head in heads {
                let id = self.fresh_id();
                kids.push(node(
                    id,
                    Transform::IDENTITY,
                    NodeKind::Path(PathNode {
                        commands: head,
                        fill: Some(Paint::Solid(color)),
                        stroke: None,
                        shadow: None,
                    }),
                ));
            }
        }
        // Text. Flips mirror the outline, never the text: a flipped shape's
        // text is mirrored back about its rectangle's centre.
        if let Some(body) = s.text.as_ref().filter(|b| !b.is_empty()) {
            let bp = resolve::body(ctx, s);
            let tr = geo.text_rect;
            let rect = Rect {
                x: tr.x0 as f32,
                y: tr.y0 as f32,
                width: tr.width() as f32,
                height: tr.height() as f32,
            };
            let text_shadow = if !filled && stroke.is_none() {
                shadow_left.take()
            } else {
                None
            };
            let frame = Frame {
                rect,
                shadow: text_shadow,
            };
            if let Some(placed) = text::block(self, ctx, s, body, &bp, frame) {
                let c = Point {
                    x: rect.x + rect.width / 2.0,
                    y: rect.y + rect.height / 2.0,
                };
                let t = local.transform;
                let mut tt = Transform::IDENTITY;
                if t.a * t.d - t.b * t.c < 0.0 {
                    tt = about(c, Transform::scale(-1.0, 1.0));
                }
                if placed.rotation != 0.0 {
                    let (sin, cos) = placed.rotation.to_radians().sin_cos();
                    let rot = Transform {
                        a: cos,
                        b: sin,
                        c: -sin,
                        d: cos,
                        e: 0.0,
                        f: 0.0,
                    };
                    tt = about(c, rot).then(tt);
                }
                let id = self.fresh_id();
                kids.push(node(id, tt, NodeKind::Text(placed.block)));
            }
        }
        self.push_shape(s, local.transform, kids);
    }

    /// Pushes a shape's nodes: alone when there is one (taking the shape's
    /// ID), otherwise in a group with the shape's ID.
    fn push_shape(&mut self, s: &Shape, transform: Transform, mut kids: Vec<Node>) {
        if kids.is_empty() {
            return;
        }
        let id = self.shape_id(Some(s.id.0));
        if kids.len() == 1 {
            let mut only = kids.remove(0);
            only.id = id;
            only.transform = only.transform.then(transform);
            self.nodes.push(only);
        } else {
            self.nodes.push(node(id, transform, NodeKind::Group(kids)));
        }
    }

    /// A picture shape: the image, with its outline over it.
    fn picture(
        &mut self,
        s: &Shape,
        pf: &PictureFill,
        local: &Local,
        outline: Option<(Vec<PathCommand>, Stroke)>,
    ) {
        let mut kids = Vec::new();
        if let Some(img) = self.image(pf, local) {
            let id = self.fresh_id();
            let mut n = node(id, Transform::IDENTITY, img);
            n.opacity = pf.alpha.map_or(1.0, |a| a.clamp(0.0, 1.0) as f32);
            kids.push(n);
        }
        if let Some((commands, stroke)) = outline {
            let id = self.fresh_id();
            kids.push(node(
                id,
                Transform::IDENTITY,
                NodeKind::Path(PathNode {
                    commands,
                    fill: None,
                    stroke: Some(stroke),
                    shadow: None,
                }),
            ));
        }
        self.push_shape(s, local.transform, kids);
    }

    fn picture_node(
        &mut self,
        s: &Shape,
        media: MediaId,
        local: &Local,
        outline: Option<(Vec<PathCommand>, Stroke)>,
    ) {
        let pf = PictureFill {
            media,
            ..Default::default()
        };
        self.picture(s, &pf, local, outline);
    }

    /// An image node for a picture fill over the shape's box: cropped by
    /// `srcRect`, placed by `fillRect`. A tiled picture covers the box.
    pub(crate) fn image(&mut self, pf: &PictureFill, local: &Local) -> Option<NodeKind> {
        let blob = self.deck.blob(self.index, pf.media)?;
        let mut rect = local.rect();
        let fit = match &pf.mode {
            PictureMode::Stretch { fill_rect } => {
                let [l, t, r, b] = fill_rect.map(|v| if v.is_finite() { v as f32 } else { 0.0 });
                rect = Rect {
                    x: l * local.w,
                    y: t * local.h,
                    width: local.w * (1.0 - l - r),
                    height: local.h * (1.0 - t - b),
                };
                Fit::Fill
            }
            PictureMode::Tile { .. } => Fit::Cover,
        };
        let [l, t, r, b] = pf.crop.map(|v| if v.is_finite() { v as f32 } else { 0.0 });
        let crop = (pf.crop.iter().any(|v| *v != 0.0)).then_some(Rect {
            x: l,
            y: t,
            width: 1.0 - l - r,
            height: 1.0 - t - b,
        });
        Some(NodeKind::Image(ImageNode {
            blob,
            rect,
            fit,
            crop,
        }))
    }

    fn ink(&mut self, strokes: &[deckcraft_model::InkStroke]) -> Vec<Node> {
        let mut out = Vec::new();
        for sk in strokes {
            let pts: Vec<Point> = sk
                .points
                .iter()
                .filter(|p| p.0.is_finite() && p.1.is_finite())
                .map(|p| Point {
                    x: p.0 as f32,
                    y: p.1 as f32,
                })
                .collect();
            if pts.len() < 2 {
                continue;
            }
            let mut commands = vec![PathCommand::MoveTo(pts[0])];
            commands.extend(pts[1..].iter().map(|p| PathCommand::LineTo(*p)));
            let id = self.fresh_id();
            let mut n = node(
                id,
                Transform::IDENTITY,
                NodeKind::Path(PathNode {
                    commands,
                    fill: None,
                    stroke: Some(Stroke {
                        paint: Paint::Solid(paint::color(sk.color)),
                        width: sk.width.abs().max(0.1) as f32,
                        dash: None,
                    }),
                    shadow: None,
                }),
            );
            if sk.highlighter {
                n.opacity = 0.4;
            }
            out.push(n);
        }
        out
    }
}

/// `t` applied about `c`.
fn about(c: Point, t: Transform) -> Transform {
    Transform::translate(-c.x, -c.y)
        .then(t)
        .then(Transform::translate(c.x, c.y))
}

fn outline_commands(g: &Geometry) -> Vec<PathCommand> {
    g.paths
        .iter()
        .filter(|p| p.stroke)
        .flat_map(|p| commands(&p.path))
        .collect()
}

/// Filled arrowheads for a line's `headEnd` (at its start) and `tailEnd`
/// (at its end), sized from the line width as PowerPoint sizes them:
/// small, medium and large are 2, 3 and 5 times the width, never smaller
/// than for a 2 pt line (DeckCraft's measurements).
fn arrowheads(line: &Line, geo: &Geometry, width: f32) -> Vec<Vec<PathCommand>> {
    let Some(path) = geo.paths.iter().find(|p| p.stroke).map(|p| &p.path) else {
        return Vec::new();
    };
    let Some(((start, after_start), (end, before_end))) = geometry::ends(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (end_style, tip, from) in [
        (&line.head, start, after_start),
        (&line.tail, end, before_end),
    ] {
        if let Some(e) = end_style {
            if let Some(head) = arrowhead(e, tip, from, width) {
                out.push(head);
            }
        }
    }
    out
}

fn arrowhead(e: &LineEnd, tip: Point, from: Point, width: f32) -> Option<Vec<PathCommand>> {
    let size = |s: &str| match s {
        "sm" => 2.0,
        "lg" => 5.0,
        _ => 3.0,
    };
    let k = width.max(2.0);
    let (half_w, len) = (size(&e.w) * k / 2.0, size(&e.len) * k);
    let (dx, dy) = (tip.x - from.x, tip.y - from.y);
    let d = dx.hypot(dy);
    if d < 1e-6 {
        return None;
    }
    // Unit vector along the line towards the tip, and its normal.
    let (ux, uy) = (dx / d, dy / d);
    let (nx, ny) = (-uy, ux);
    let at = |along: f32, across: f32| Point {
        x: tip.x - ux * along + nx * across,
        y: tip.y - uy * along + ny * across,
    };
    let p = |q: Point| PathCommand::LineTo(q);
    Some(match e.kind.as_str() {
        "none" | "" => return None,
        "triangle" => vec![
            PathCommand::MoveTo(tip),
            p(at(len, half_w)),
            p(at(len, -half_w)),
            PathCommand::Close,
        ],
        "stealth" => vec![
            PathCommand::MoveTo(tip),
            p(at(len, half_w)),
            p(at(len * 0.6, 0.0)),
            p(at(len, -half_w)),
            PathCommand::Close,
        ],
        "diamond" => vec![
            PathCommand::MoveTo(at(-len / 2.0, 0.0)),
            p(at(0.0, half_w)),
            p(at(len / 2.0, 0.0)),
            p(at(0.0, -half_w)),
            PathCommand::Close,
        ],
        "oval" => {
            // A circle of four cubic arcs centred on the end.
            let (rx, ry) = (len / 2.0, half_w);
            let k = 0.552_284_8;
            let c = tip;
            let pt = |a: f32, b: f32| Point {
                x: c.x + ux * a + nx * b,
                y: c.y + uy * a + ny * b,
            };
            vec![
                PathCommand::MoveTo(pt(rx, 0.0)),
                PathCommand::CubicTo(pt(rx, ry * k), pt(rx * k, ry), pt(0.0, ry)),
                PathCommand::CubicTo(pt(-rx * k, ry), pt(-rx, ry * k), pt(-rx, 0.0)),
                PathCommand::CubicTo(pt(-rx, -ry * k), pt(-rx * k, -ry), pt(0.0, -ry)),
                PathCommand::CubicTo(pt(rx * k, -ry), pt(rx, -ry * k), pt(rx, 0.0)),
                PathCommand::Close,
            ]
        }
        // `arrow`: an open V, drawn as a thin filled chevron.
        _ => vec![
            PathCommand::MoveTo(tip),
            p(at(len, half_w)),
            p(at(len, half_w - width)),
            p(at(width, 0.0)),
            p(at(len, -half_w + width)),
            p(at(len, -half_w)),
            PathCommand::Close,
        ],
    })
}

/// How far a line stops short of each end under its arrowheads (head,
/// tail): to a triangle's base, into a stealth head's notch, and to where
/// an open arrow's legs meet. Ovals and diamonds sit on the end itself.
fn head_trims(line: &Line, width: f32) -> (f32, f32) {
    let k = width.max(2.0);
    let trim = |e: &Option<LineEnd>| {
        let Some(e) = e else { return 0.0 };
        let size = |s: &str| match s {
            "sm" => 2.0,
            "lg" => 5.0,
            _ => 3.0,
        };
        let (w, len) = (size(&e.w) * k, size(&e.len) * k);
        match e.kind.as_str() {
            "triangle" => len,
            "stealth" => (len * (0.6 + 0.4 * width / w.max(1e-6))).min(len),
            "arrow" => {
                let half = (w / 2.0).atan2(len);
                if half.sin() > 1e-6 {
                    (width / 2.0 / half.sin()).min(len)
                } else {
                    0.0
                }
            }
            _ => 0.0,
        }
    };
    (trim(&line.head), trim(&line.tail))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_frame_keeps_points_inside_a_scaled_group() {
        // A shape 100 × 50 in a group drawn at half its child size.
        let x = Xfrm::new(10.0, 20.0, 100.0, 50.0);
        let local = Local::new(&x, Transform::scale(0.5, 0.5)).unwrap();
        assert_eq!((local.w, local.h), (50.0, 25.0));
        let corner = local.transform.apply(Point { x: 50.0, y: 25.0 });
        assert_eq!(corner, Point { x: 55.0, y: 35.0 });
    }

    #[test]
    fn a_triangle_head_points_at_the_line_end() {
        let e = LineEnd {
            kind: "triangle".into(),
            w: "med".into(),
            len: "med".into(),
        };
        let head = arrowhead(
            &e,
            Point { x: 100.0, y: 0.0 },
            Point { x: 0.0, y: 0.0 },
            2.0,
        )
        .unwrap();
        assert_eq!(head[0], PathCommand::MoveTo(Point { x: 100.0, y: 0.0 }));
        assert_eq!(head[1], PathCommand::LineTo(Point { x: 94.0, y: 3.0 }));
        assert_eq!(head[2], PathCommand::LineTo(Point { x: 94.0, y: -3.0 }));
    }
}
