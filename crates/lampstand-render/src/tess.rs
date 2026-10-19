//! Paths to triangles with lyon: fills, strokes and dashes.
//!
//! Tessellation happens in the node's local space, with the tolerance
//! scaled so curves stay within a fraction of a pixel on screen; vertices
//! are transformed afterwards. Strokes therefore scale and skew with their
//! node, as in SVG and the DOM.

use lampstand_core::scene::PathCommand;
use lyon::math::{point, Point};
use lyon::path::iterator::PathIterator;
use lyon::path::{Path, PathEvent};
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, LineCap, LineJoin,
    StrokeOptions, StrokeTessellator, StrokeVertex, VertexBuffers,
};

/// Maximum distance between a curve and its flattening, in output pixels.
const TOLERANCE_PX: f32 = 0.1;

pub(crate) type Mesh = VertexBuffers<Point, u32>;

pub(crate) fn build_path(commands: &[PathCommand]) -> Path {
    let mut b = Path::svg_builder();
    for command in commands {
        match *command {
            PathCommand::MoveTo(p) => {
                b.move_to(point(p.x, p.y));
            }
            PathCommand::LineTo(p) => {
                b.line_to(point(p.x, p.y));
            }
            PathCommand::QuadTo(c, p) => {
                b.quadratic_bezier_to(point(c.x, c.y), point(p.x, p.y));
            }
            PathCommand::CubicTo(c1, c2, p) => {
                b.cubic_bezier_to(point(c1.x, c1.y), point(c2.x, c2.y), point(p.x, p.y));
            }
            PathCommand::Close => b.close(),
        }
    }
    b.build()
}

/// Local-space tolerance for a transform that scales by `scale`.
pub(crate) fn tolerance(scale: f32) -> f32 {
    (TOLERANCE_PX / scale.max(1e-6)).max(1e-4)
}

pub(crate) fn fill(path: &Path, tolerance: f32) -> Mesh {
    let mut mesh = Mesh::new();
    let options = FillOptions::tolerance(tolerance).with_fill_rule(FillRule::NonZero);
    // A degenerate path yields an error and whatever was built; drawing that
    // (or nothing) beats failing the frame.
    let _ = FillTessellator::new().tessellate_path(
        path,
        &options,
        &mut BuffersBuilder::new(&mut mesh, |v: FillVertex| v.position()),
    );
    mesh
}

pub(crate) fn stroke(path: &Path, width: f32, join: LineJoin, tolerance: f32) -> Mesh {
    let mut mesh = Mesh::new();
    let options = StrokeOptions::tolerance(tolerance)
        .with_line_width(width)
        .with_line_join(join)
        .with_line_cap(LineCap::Butt);
    let _ = StrokeTessellator::new().tessellate_path(
        path,
        &options,
        &mut BuffersBuilder::new(&mut mesh, |v: StrokeVertex| v.position()),
    );
    mesh
}

/// `path` cut into dashes. Each subpath restarts the pattern, as in SVG.
/// An odd-length pattern is repeated to make it even. A pattern with no
/// positive length returns `None` (draw a solid line).
pub(crate) fn dash(path: &Path, pattern: &[f32], tolerance: f32) -> Option<Path> {
    let mut pattern: Vec<f32> = pattern.iter().map(|d| d.max(0.0)).collect();
    if pattern.len() % 2 == 1 {
        pattern.extend_from_within(..);
    }
    if pattern.iter().sum::<f32>() <= 0.0 {
        return None;
    }
    let mut out = Path::builder();
    let mut polyline: Vec<Point> = Vec::new();
    for event in path.iter().flattened(tolerance) {
        match event {
            PathEvent::Begin { at } => {
                polyline.clear();
                polyline.push(at);
            }
            PathEvent::Line { to, .. } => polyline.push(to),
            PathEvent::End { close, first, .. } => {
                if close {
                    polyline.push(first);
                }
                dash_polyline(&polyline, &pattern, &mut out);
            }
            PathEvent::Quadratic { .. } | PathEvent::Cubic { .. } => {
                unreachable!("flattened")
            }
        }
    }
    Some(out.build())
}

fn dash_polyline(points: &[Point], pattern: &[f32], out: &mut lyon::path::Builder) {
    let mut index = 0;
    let mut left = pattern[0];
    let mut on = true;
    let mut drawing = false;
    for pair in points.windows(2) {
        let (mut a, b) = (pair[0], pair[1]);
        let mut length = (b - a).length();
        while length > 0.0 {
            let step = left.min(length);
            let next = a + (b - a).normalize() * step;
            if on {
                if !drawing {
                    out.begin(a);
                    drawing = true;
                }
                out.line_to(next);
            }
            a = next;
            length -= step;
            left -= step;
            if left <= 0.0 {
                if drawing {
                    out.end(false);
                    drawing = false;
                }
                index = (index + 1) % pattern.len();
                left = pattern[index];
                on = index % 2 == 0;
            }
        }
    }
    if drawing {
        out.end(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lampstand_core::geom::Point as P;

    fn line(len: f32) -> Path {
        build_path(&[
            PathCommand::MoveTo(P { x: 0.0, y: 0.0 }),
            PathCommand::LineTo(P { x: len, y: 0.0 }),
        ])
    }

    fn subpaths(path: &Path) -> Vec<(f32, f32)> {
        let mut out = Vec::new();
        let mut start = 0.0;
        for event in path.iter() {
            match event {
                PathEvent::Begin { at } => start = at.x,
                PathEvent::End { last, .. } => out.push((start, last.x)),
                _ => {}
            }
        }
        out
    }

    #[test]
    fn dashes_follow_the_pattern() {
        let dashed = dash(&line(100.0), &[30.0, 10.0], 0.1).unwrap();
        assert_eq!(
            subpaths(&dashed),
            vec![(0.0, 30.0), (40.0, 70.0), (80.0, 100.0)]
        );
    }

    #[test]
    fn odd_pattern_repeats() {
        let dashed = dash(&line(50.0), &[10.0], 0.1).unwrap();
        assert_eq!(
            subpaths(&dashed),
            vec![(0.0, 10.0), (20.0, 30.0), (40.0, 50.0)]
        );
    }

    #[test]
    fn zero_pattern_is_solid() {
        assert!(dash(&line(50.0), &[0.0, 0.0], 0.1).is_none());
    }

    #[test]
    fn fill_of_a_square_has_area() {
        let square = build_path(&[
            PathCommand::MoveTo(P { x: 0.0, y: 0.0 }),
            PathCommand::LineTo(P { x: 10.0, y: 0.0 }),
            PathCommand::LineTo(P { x: 10.0, y: 10.0 }),
            PathCommand::LineTo(P { x: 0.0, y: 10.0 }),
            PathCommand::Close,
        ]);
        let mesh = fill(&square, 0.1);
        let area: f32 = mesh
            .indices
            .chunks(3)
            .map(|t| {
                let [a, b, c] = [0, 1, 2].map(|i| mesh.vertices[t[i] as usize]);
                ((b - a).cross(c - a) / 2.0).abs()
            })
            .sum();
        assert!((area - 100.0).abs() < 1e-3, "{area}");
    }
}
