//! Scene → frame: the CPU half of drawing.
//!
//! A `Frame` is a list of layers, each a list of draw items in painter's
//! order. Layer 0 is the target. A node gets its own layer when it cannot be
//! drawn straight into its parent:
//!
//! - opacity below 1 on anything that draws overlapping pieces (a group, a
//!   text block, a path with fill and stroke), so the pieces fade as one;
//! - a shadow: the shape is drawn into a padded layer, blurred, and
//!   composited tinted with the shadow colour;
//! - text under a rotation, skew, mirror or non-uniform scale, which glyphon
//!   cannot draw: it is drawn upright into a layer at the output scale and
//!   the layer is composited with the transform.
//!
//! A layer's children always have higher indices, so the GPU half draws
//! layers from last to first. Every coordinate a layer's items carry is in
//! that layer's pixels, already converted to clip space.

use std::ops::Range;

use cosmic_text::{Buffer, SwashCache};
use lampstand_core::geom::{Point, Rect, Transform};
use lampstand_core::id::BlobHash;
use lampstand_core::scene::{Node, NodeKind, Paint, PathNode, Shadow, TextBlock};
use lampstand_layout::Fonts;
use lyon::tessellation::LineJoin;

use crate::gpu::{Gpu, PaintUniform, Pipelines, QuadUniform, QuadVertex, ShapeVertex};
use crate::images::{self, ImageCache};
use crate::paint::{premultiplied, Ramps};
use crate::text::{self, LaidOut};
use crate::{tess, BlobSource};

pub(crate) struct Layer {
    pub width: u32,
    pub height: u32,
    /// Gaussian blur applied after drawing (sigma in pixels).
    pub blur: Option<f32>,
    pub items: Vec<Item>,
}

pub(crate) enum Item {
    Mesh {
        indices: Range<u32>,
        paint: u32,
    },
    Quad {
        source: Source,
        first: u32,
        uniform: u32,
    },
    Text(usize),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Source {
    Image(BlobHash),
    Layer(usize),
}

/// Text areas glyphon draws in one call.
pub(crate) struct TextJob {
    pub layer: usize,
    pub areas: Vec<Area>,
}

pub(crate) struct Area {
    pub buffer: usize,
    pub left: f32,
    pub top: f32,
    pub scale: f32,
}

#[derive(Default)]
pub(crate) struct Frame {
    pub layers: Vec<Layer>,
    pub vertices: Vec<ShapeVertex>,
    pub indices: Vec<u32>,
    pub quad_vertices: Vec<QuadVertex>,
    pub paints: Vec<PaintUniform>,
    pub quads: Vec<QuadUniform>,
    pub ramps: Ramps,
    pub buffers: Vec<Buffer>,
    pub texts: Vec<TextJob>,
}

/// Everything the builder borrows from the renderer.
pub(crate) struct Builder<'a> {
    pub frame: Frame,
    pub gpu: &'a Gpu,
    pub pipelines: &'a Pipelines,
    pub blobs: &'a dyn BlobSource,
    pub images: &'a mut ImageCache,
    pub fonts: &'a mut Fonts,
    pub swash: &'a mut SwashCache,
}

/// How much a transform scales lengths, on average.
fn scale_of(t: &Transform) -> f32 {
    (t.a * t.d - t.b * t.c).abs().sqrt()
}

/// True for a transform glyphon can draw: uniform positive scale plus
/// translation.
fn is_upright(t: &Transform) -> bool {
    let eps = 1e-4 * t.a.abs().max(1.0);
    t.b.abs() <= eps && t.c.abs() <= eps && t.a > 0.0 && (t.a - t.d).abs() <= eps
}

/// Whether `node` at reduced opacity can fold the opacity into its paint
/// instead of needing a layer.
fn is_single_primitive(node: &Node) -> bool {
    match &node.kind {
        NodeKind::Image(_) | NodeKind::Video(_) => true,
        NodeKind::Path(p) => p.shadow.is_none() && !(p.fill.is_some() && p.stroke.is_some()),
        NodeKind::Group(children) => {
            let mut visible = children.iter().filter(|c| c.visible);
            match (visible.next(), visible.next()) {
                (None, _) => true,
                (Some(only), None) => is_single_primitive(only),
                _ => false,
            }
        }
        NodeKind::Text(_) => false,
    }
}

impl Builder<'_> {
    pub fn push_layer(&mut self, width: u32, height: u32, blur: Option<f32>) -> usize {
        let max = self.gpu.max_texture_size();
        self.frame.layers.push(Layer {
            width: width.clamp(1, max),
            height: height.clamp(1, max),
            blur,
            items: Vec::new(),
        });
        self.frame.layers.len() - 1
    }

    fn size(&self, layer: usize) -> (f32, f32) {
        let l = &self.frame.layers[layer];
        (l.width as f32, l.height as f32)
    }

    fn clip(&self, layer: usize, p: Point) -> [f32; 2] {
        let (w, h) = self.size(layer);
        [p.x / w * 2.0 - 1.0, 1.0 - p.y / h * 2.0]
    }

    /// Draws `node`, whose parent space maps to `layer`'s pixels by `parent`.
    pub fn node(&mut self, layer: usize, node: &Node, parent: Transform, inherited: f32) {
        if !node.visible {
            return;
        }
        let opacity = node.opacity.clamp(0.0, 1.0) * inherited;
        if opacity <= 0.0 {
            return;
        }
        let t = node.transform.then(parent);
        if opacity < 1.0 && !is_single_primitive(node) {
            let (w, h) = self.size(layer);
            let sub = self.push_layer(w as u32, h as u32, None);
            self.content(sub, node, t, 1.0);
            self.composite(layer, sub, opacity);
            return;
        }
        self.content(layer, node, t, opacity);
    }

    fn content(&mut self, layer: usize, node: &Node, t: Transform, opacity: f32) {
        match &node.kind {
            NodeKind::Group(children) => {
                for child in children {
                    self.node(layer, child, t, opacity);
                }
            }
            NodeKind::Path(p) => {
                if let Some(shadow) = &p.shadow {
                    self.shadow(layer, shadow, t, opacity, |b, sub, st| {
                        b.path(sub, p, st, 1.0)
                    });
                }
                self.path(layer, p, t, opacity);
            }
            NodeKind::Text(block) => self.text(layer, block, t),
            NodeKind::Image(image) => {
                let Some((w, h)) =
                    self.images
                        .load(self.gpu, self.pipelines, self.blobs, &image.blob)
                else {
                    return;
                };
                let Some((dest, uv)) = images::place(image.rect, image.fit, image.crop, w, h)
                else {
                    return;
                };
                let corners = [
                    (dest.x, dest.y),
                    (dest.x + dest.width, dest.y),
                    (dest.x, dest.y + dest.height),
                    (dest.x + dest.width, dest.y + dest.height),
                ]
                .map(|(x, y)| t.apply(Point { x, y }));
                let uvs = [
                    [uv.x, uv.y],
                    [uv.x + uv.width, uv.y],
                    [uv.x, uv.y + uv.height],
                    [uv.x + uv.width, uv.y + uv.height],
                ];
                self.quad(
                    layer,
                    Source::Image(image.blob),
                    corners,
                    uvs,
                    QuadUniform {
                        opacity,
                        ..Default::default()
                    },
                );
            }
            // T27.
            NodeKind::Video(_) => {}
        }
    }

    fn path(&mut self, layer: usize, p: &PathNode, t: Transform, opacity: f32) {
        let path = tess::build_path(&p.commands);
        let tolerance = tess::tolerance(scale_of(&t));
        if let Some(fill) = &p.fill {
            let mesh = tess::fill(&path, tolerance);
            self.mesh(layer, &mesh, t, fill, opacity);
        }
        if let Some(stroke) = &p.stroke {
            if stroke.width <= 0.0 {
                return;
            }
            let dashed = stroke
                .dash
                .as_deref()
                .and_then(|d| tess::dash(&path, d, tolerance));
            let mesh = tess::stroke(
                dashed.as_ref().unwrap_or(&path),
                stroke.width,
                LineJoin::Miter,
                tolerance,
            );
            self.mesh(layer, &mesh, t, &stroke.paint, opacity);
        }
    }

    /// Adds a mesh in local coordinates, mapped to `layer`'s pixels by `t`.
    fn mesh(&mut self, layer: usize, mesh: &tess::Mesh, t: Transform, paint: &Paint, opacity: f32) {
        if mesh.indices.is_empty() {
            return;
        }
        let base = self.frame.vertices.len() as u32;
        for v in &mesh.vertices {
            let local = Point { x: v.x, y: v.y };
            let position = self.clip(layer, t.apply(local));
            self.frame.vertices.push(ShapeVertex {
                position,
                local: [v.x, v.y],
            });
        }
        let start = self.frame.indices.len() as u32;
        self.frame
            .indices
            .extend(mesh.indices.iter().map(|i| i + base));
        let end = self.frame.indices.len() as u32;
        let uniform = self.frame.ramps.uniform(paint, opacity);
        self.frame.paints.push(uniform);
        let paint = self.frame.paints.len() as u32 - 1;
        self.frame.layers[layer].items.push(Item::Mesh {
            indices: start..end,
            paint,
        });
    }

    /// Corners (top-left, top-right, bottom-left, bottom-right) in `layer`'s
    /// pixels.
    fn quad(
        &mut self,
        layer: usize,
        source: Source,
        corners: [Point; 4],
        uvs: [[f32; 2]; 4],
        uniform: QuadUniform,
    ) {
        let positions = corners.map(|c| self.clip(layer, c));
        let first = self.frame.quad_vertices.len() as u32;
        self.frame
            .quad_vertices
            .extend(images::quad_vertices(positions, uvs));
        self.frame.quads.push(uniform);
        let uniform = self.frame.quads.len() as u32 - 1;
        self.frame.layers[layer].items.push(Item::Quad {
            source,
            first,
            uniform,
        });
    }

    /// Composites a same-size sub-layer over `layer`.
    fn composite(&mut self, layer: usize, sub: usize, opacity: f32) {
        let (w, h) = self.size(layer);
        let corners = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)].map(|(x, y)| Point { x, y });
        self.quad(
            layer,
            Source::Layer(sub),
            corners,
            [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            QuadUniform {
                opacity,
                ..Default::default()
            },
        );
    }

    /// Draws a shadow: `draw` paints the shape into a padded layer (with the
    /// transform it is given), which is blurred and composited tinted.
    /// Offset and blur are in the node's local units, so they scale and
    /// rotate with it, like CSS shadows.
    fn shadow(
        &mut self,
        layer: usize,
        shadow: &Shadow,
        t: Transform,
        opacity: f32,
        draw: impl FnOnce(&mut Self, usize, Transform),
    ) {
        if shadow.color.a == 0 {
            return;
        }
        // CSS convention: the blur radius is twice the standard deviation.
        let sigma = shadow.blur.max(0.0) / 2.0 * scale_of(&t);
        let pad = (sigma * 3.0).ceil() + 1.0;
        let (w, h) = self.size(layer);
        let blur = (sigma > 0.25).then_some(sigma);
        let sub = self.push_layer((w + 2.0 * pad) as u32, (h + 2.0 * pad) as u32, blur);
        let (sw, sh) = self.size(sub);
        let shifted = Transform::translate(shadow.offset.x, shadow.offset.y)
            .then(t)
            .then(Transform::translate(pad, pad));
        draw(self, sub, shifted);
        let corners = [(0.0, 0.0), (sw, 0.0), (0.0, sh), (sw, sh)].map(|(x, y)| Point {
            x: x - pad,
            y: y - pad,
        });
        let tint = premultiplied(shadow.color).map(|c| c * opacity);
        self.quad(
            layer,
            Source::Layer(sub),
            corners,
            [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            QuadUniform {
                mode: 1,
                opacity,
                tint,
                ..Default::default()
            },
        );
    }

    fn text(&mut self, layer: usize, block: &TextBlock, t: Transform) {
        let laid = text::lay_out(self.fonts, block);
        if is_upright(&t) {
            self.text_upright(layer, block, &laid, t, true);
            return;
        }
        // Draw upright into a layer at the output scale, then composite the
        // layer through the transform.
        let mut b = laid.bounds;
        let mut grow = block.outline.map_or(0.0, |o| o.width.max(0.0));
        if let Some(s) = &block.shadow {
            grow += s.offset.x.abs().max(s.offset.y.abs()) + 1.5 * s.blur.max(0.0);
        }
        b = Rect {
            x: b.x - grow,
            y: b.y - grow,
            width: b.width + 2.0 * grow,
            height: b.height + 2.0 * grow,
        };
        let max = self.gpu.max_texture_size() as f32;
        let column = |x: f32, y: f32| (x * x + y * y).sqrt();
        let scale = column(t.a, t.b)
            .max(column(t.c, t.d))
            .min(max / b.width.max(1e-3))
            .min(max / b.height.max(1e-3));
        if scale <= 0.0 || b.width <= 0.0 || b.height <= 0.0 {
            return;
        }
        let (w, h) = ((b.width * scale).ceil(), (b.height * scale).ceil());
        let sub = self.push_layer(w as u32, h as u32, None);
        let upright = Transform::translate(-b.x, -b.y).then(Transform::scale(scale, scale));
        self.text_upright(sub, block, &laid, upright, true);
        let corners = [
            (b.x, b.y),
            (b.x + b.width, b.y),
            (b.x, b.y + b.height),
            (b.x + b.width, b.y + b.height),
        ]
        .map(|(x, y)| t.apply(Point { x, y }));
        let (u, v) = (b.width * scale / w, b.height * scale / h);
        self.quad(
            layer,
            Source::Layer(sub),
            corners,
            [[0.0, 0.0], [u, 0.0], [0.0, v], [u, v]],
            QuadUniform {
                opacity: 1.0,
                ..Default::default()
            },
        );
    }

    /// Draws text under an upright transform `t` (see `is_upright`): shadow,
    /// outline, glyphs, then underlines and strikes.
    fn text_upright(
        &mut self,
        layer: usize,
        block: &TextBlock,
        laid: &LaidOut,
        t: Transform,
        with_shadow: bool,
    ) {
        if with_shadow {
            if let Some(shadow) = &block.shadow {
                self.shadow(layer, shadow, t, 1.0, |b, sub, st| {
                    b.text_upright(sub, block, laid, st, false)
                });
            }
        }
        let scale = t.a;
        let areas: Vec<Area> = laid
            .pieces
            .iter()
            .map(|piece| {
                let buffer = self.frame.buffers.len();
                self.frame.buffers.push(piece.buffer.clone());
                Area {
                    buffer,
                    left: t.e + piece.x * scale,
                    top: t.f + piece.y * scale,
                    scale,
                }
            })
            .collect();
        if let Some(outline) = block.outline.filter(|o| o.width > 0.0 && o.color.a > 0) {
            let path = self.glyph_outlines(&areas);
            // Centred on the glyph edge and drawn under the fill, so `width`
            // is how far it shows outside the glyph (CSS `paint-order:
            // stroke` with a stroke of twice the width).
            let mesh = tess::stroke(
                &path,
                2.0 * outline.width * scale,
                LineJoin::Round,
                tess::tolerance(1.0),
            );
            self.mesh(
                layer,
                &mesh,
                Transform::IDENTITY,
                &Paint::Solid(outline.color),
                1.0,
            );
        }
        let decorations = self.decorations(&areas);
        self.frame.texts.push(TextJob { layer, areas });
        let job = self.frame.texts.len() - 1;
        self.frame.layers[layer].items.push(Item::Text(job));
        for (rect, color) in decorations {
            let mesh = tess::fill(&tess::build_path(&rect_path(rect)), tess::tolerance(1.0));
            self.mesh(layer, &mesh, Transform::IDENTITY, &Paint::Solid(color), 1.0);
        }
    }

    /// Every glyph's outline, in layer pixels, placed exactly where glyphon
    /// puts the glyph's bitmap.
    fn glyph_outlines(&mut self, areas: &[Area]) -> lyon::path::Path {
        use cosmic_text::Command;
        use lyon::math::point;
        let mut b = lyon::path::Path::svg_builder();
        for area in areas {
            let buffer = &self.frame.buffers[area.buffer];
            for run in buffer.layout_runs() {
                for glyph in run.glyphs {
                    let physical = glyph.physical((area.left, area.top), area.scale);
                    let x0 = physical.x as f32 + physical.cache_key.x_bin.as_float();
                    // `physical` truncates y, so there is no vertical bin.
                    let y0 = (run.line_y * area.scale).round() + physical.y as f32;
                    let Some(commands) = self
                        .swash
                        .get_outline_commands(self.fonts.system(), physical.cache_key)
                    else {
                        continue;
                    };
                    // Font outlines are y-up.
                    let p = |x: f32, y: f32| point(x0 + x, y0 - y);
                    for command in commands {
                        match *command {
                            Command::MoveTo(v) => {
                                b.move_to(p(v.x, v.y));
                            }
                            Command::LineTo(v) => {
                                b.line_to(p(v.x, v.y));
                            }
                            Command::QuadTo(c, v) => {
                                b.quadratic_bezier_to(p(c.x, c.y), p(v.x, v.y));
                            }
                            Command::CurveTo(c1, c2, v) => {
                                b.cubic_bezier_to(p(c1.x, c1.y), p(c2.x, c2.y), p(v.x, v.y));
                            }
                            Command::Close => b.close(),
                        }
                    }
                }
            }
        }
        b.build()
    }

    /// Underline and strike rectangles in layer pixels, from the font's own
    /// decoration metrics.
    fn decorations(&self, areas: &[Area]) -> Vec<(Rect, lampstand_core::geom::Color)> {
        let mut out = Vec::new();
        for area in areas {
            let buffer = &self.frame.buffers[area.buffer];
            for run in buffer.layout_runs() {
                for span in run.decorations {
                    let glyphs = &run.glyphs[span.glyph_range.clone()];
                    let x0 = glyphs.iter().map(|g| g.x).fold(f32::INFINITY, f32::min);
                    let x1 = glyphs
                        .iter()
                        .map(|g| g.x + g.w)
                        .fold(f32::NEG_INFINITY, f32::max);
                    if x1 <= x0 {
                        continue;
                    }
                    let color = span
                        .color_opt
                        .map_or(lampstand_core::geom::Color::WHITE, |c| {
                            lampstand_core::geom::Color {
                                r: c.r(),
                                g: c.g(),
                                b: c.b(),
                                a: c.a(),
                            }
                        });
                    let d = &span.data;
                    let mut line = |metrics: &cosmic_text::DecorationMetrics| {
                        let thickness = (metrics.thickness * span.font_size * area.scale).max(1.0);
                        let y =
                            area.top + (run.line_y - metrics.offset * span.font_size) * area.scale;
                        out.push((
                            Rect {
                                x: area.left + x0 * area.scale,
                                y,
                                width: (x1 - x0) * area.scale,
                                height: thickness,
                            },
                            color,
                        ));
                    };
                    if d.text_decoration.underline != cosmic_text::UnderlineStyle::None {
                        line(&d.underline_metrics);
                    }
                    if d.text_decoration.strikethrough {
                        line(&d.strikethrough_metrics);
                    }
                }
            }
        }
        out
    }
}

fn rect_path(r: Rect) -> [lampstand_core::scene::PathCommand; 5] {
    use lampstand_core::scene::PathCommand::{Close, LineTo, MoveTo};
    let p = |x, y| Point { x, y };
    [
        MoveTo(p(r.x, r.y)),
        LineTo(p(r.x + r.width, r.y)),
        LineTo(p(r.x + r.width, r.y + r.height)),
        LineTo(p(r.x, r.y + r.height)),
        Close,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upright_accepts_only_uniform_scale_and_translate() {
        assert!(is_upright(
            &Transform::scale(2.0, 2.0).then(Transform::translate(5.0, 1.0))
        ));
        assert!(!is_upright(&Transform::scale(2.0, 1.0)));
        assert!(!is_upright(&Transform::scale(-1.0, -1.0)));
        let rotate = Transform {
            a: 0.0,
            b: 1.0,
            c: -1.0,
            d: 0.0,
            e: 0.0,
            f: 0.0,
        };
        assert!(!is_upright(&rotate));
    }
}
