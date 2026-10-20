//! Shape geometry to paths: preset shapes (from DeckCraft's generators) and
//! custom geometry.
//!
//! DeckCraft builds every preset in the DrawingML schema (`ST_ShapeType`).
//! A name it doesn't know (a file outside the schema) becomes the bounding
//! rectangle, and the slide gets a fidelity issue.

use deckcraft_geom::preset::{self, FillMode, Geometry, SubPath};
use deckcraft_geom::{BezPath, KurboShape, PathEl};
use deckcraft_model::{Geom, Shape};
use lampstand_core::geom::Point;
use lampstand_core::scene::PathCommand;

/// The 40 most common presets. The first 30 are every preset that appears
/// in the Apache POI slideshow corpus (95 decks, 2,958 preset shapes),
/// most frequent first; the last 10 are the most used of PowerPoint's
/// gallery that the corpus lacks. Each must turn into a real outline, not
/// the fallback rectangle (tested below).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const COMMON_PRESETS: [&str; 40] = [
    // Corpus counts: rect 2486, roundRect 92, line 59, hexagon 56,
    // ellipse 47, straightConnector1 46, leftArrow 21, ...
    "rect",
    "roundRect",
    "line",
    "hexagon",
    "ellipse",
    "straightConnector1",
    "leftArrow",
    "stripedRightArrow",
    "rightArrow",
    "chevron",
    "flowChartMerge",
    "wedgeRoundRectCallout",
    "rtTriangle",
    "pie",
    "flowChartExtract",
    "triangle",
    "homePlate",
    "corner",
    "flowChartDelay",
    "bentConnector3",
    "bentUpArrow",
    "rightBrace",
    "actionButtonBlank",
    "curvedConnector3",
    "star8",
    "leftBrace",
    "arc",
    "sun",
    "flowChartManualInput",
    "smileyFace",
    // Gallery staples missing from the corpus.
    "star5",
    "diamond",
    "pentagon",
    "parallelogram",
    "trapezoid",
    "octagon",
    "downArrow",
    "upArrow",
    "wedgeRectCallout",
    "cloud",
];

/// A shape's geometry at its size on the slide, and whether its preset was
/// unknown (and is drawn as its bounding rectangle).
pub(crate) fn geometry(shape: &Shape, w: f64, h: f64) -> (Geometry, bool) {
    let (mut g, unknown) = build(shape, w, h);
    // Our scenes fill non-zero; make even-odd sub-paths fill the same way.
    for sp in &mut g.paths {
        if sp.even_odd {
            sp.path = nonzero_from_even_odd(&sp.path);
            sp.even_odd = false;
        }
    }
    (g, unknown)
}

fn build(shape: &Shape, w: f64, h: f64) -> (Geometry, bool) {
    let full = deckcraft_geom::Rect::new(0.0, 0.0, w, h);
    match &shape.geom {
        Geom::Preset { name, adj } => match preset::build(name, w, h, adj) {
            Some(g) => (g, false),
            None => (rect_geometry(w, h), name != "rect"),
        },
        Geom::Custom { paths } => {
            let mut out = Vec::with_capacity(paths.len());
            for p in paths {
                let pw = if p.w > 0.0 { p.w } else { w.max(1e-9) };
                let ph = if p.h > 0.0 { p.h } else { h.max(1e-9) };
                let mut path = parse_path(&p.d);
                path.apply_affine(deckcraft_geom::Affine::scale_non_uniform(w / pw, h / ph));
                out.push(SubPath {
                    path,
                    fill: p.fill,
                    stroke: p.stroke,
                    even_odd: true,
                });
            }
            (
                Geometry {
                    paths: out,
                    text_rect: full,
                    handles: vec![],
                    sites: vec![],
                },
                false,
            )
        }
    }
}

/// Contours in one even-odd sub-path, rewound so the non-zero rule (the
/// only one scenes have) fills the same area: a contour inside an even
/// number of others runs clockwise, one inside an odd number runs
/// anticlockwise. DeckCraft winds the holes of `donut`, `frame`,
/// `noSmoking` and `funnel` the same way as their outlines, and custom
/// shapes (converted icons, outlined text) can wind them either way.
/// Contours that cross each other are left as they are.
fn nonzero_from_even_odd(path: &BezPath) -> BezPath {
    // Quadratic in the number of contours; icons with more are left alone.
    const MAX_CONTOURS: usize = 256;
    let contours: Vec<BezPath> = path
        .subpaths()
        .map(|els| BezPath::from_vec(els.to_vec()))
        .collect();
    if contours.len() < 2 || contours.len() > MAX_CONTOURS {
        return path.clone();
    }
    let mut out = BezPath::new();
    for (i, contour) in contours.iter().enumerate() {
        let area = contour.area();
        let start = match contour.elements().first() {
            Some(PathEl::MoveTo(p)) => *p,
            _ => {
                out.extend(contour.iter());
                continue;
            }
        };
        let depth = contours
            .iter()
            .enumerate()
            .filter(|(j, other)| *j != i && other.winding(start) != 0)
            .count();
        // kurbo's area is positive for clockwise contours in y-down space.
        let want_positive = depth % 2 == 0;
        if area != 0.0 && (area > 0.0) != want_positive {
            out.extend(contour.reverse_subpaths().iter());
        } else {
            out.extend(contour.iter());
        }
    }
    out
}

fn rect_geometry(w: f64, h: f64) -> Geometry {
    let r = deckcraft_geom::Rect::new(0.0, 0.0, w, h);
    let mut path = BezPath::new();
    path.move_to((0.0, 0.0));
    path.line_to((w, 0.0));
    path.line_to((w, h));
    path.line_to((0.0, h));
    path.close_path();
    Geometry {
        paths: vec![SubPath {
            path,
            fill: FillMode::Norm,
            stroke: true,
            even_odd: false,
        }],
        text_rect: r,
        handles: vec![],
        sites: vec![],
    }
}

pub(crate) fn rect_path(w: f32, h: f32) -> Vec<PathCommand> {
    let p = |x, y| Point { x, y };
    vec![
        PathCommand::MoveTo(p(0.0, 0.0)),
        PathCommand::LineTo(p(w, 0.0)),
        PathCommand::LineTo(p(w, h)),
        PathCommand::LineTo(p(0.0, h)),
        PathCommand::Close,
    ]
}

pub(crate) fn commands(path: &BezPath) -> Vec<PathCommand> {
    let p = |q: deckcraft_geom::Point| Point {
        x: q.x as f32,
        y: q.y as f32,
    };
    let ok = |p: &deckcraft_geom::Point| p.x.is_finite() && p.y.is_finite();
    let finite = path.elements().iter().all(|el| match el {
        PathEl::MoveTo(a) | PathEl::LineTo(a) => ok(a),
        PathEl::QuadTo(a, b) => ok(a) && ok(b),
        PathEl::CurveTo(a, b, c) => ok(a) && ok(b) && ok(c),
        PathEl::ClosePath => true,
    });
    // A point that is not a number drops the whole sub-path: dropping just
    // that segment would join its neighbours with a stray line.
    if !finite {
        return Vec::new();
    }
    path.elements()
        .iter()
        .map(|el| match *el {
            PathEl::MoveTo(a) => PathCommand::MoveTo(p(a)),
            PathEl::LineTo(a) => PathCommand::LineTo(p(a)),
            PathEl::QuadTo(a, b) => PathCommand::QuadTo(p(a), p(b)),
            PathEl::CurveTo(a, b, c) => PathCommand::CubicTo(p(a), p(b), p(c)),
            PathEl::ClosePath => PathCommand::Close,
        })
        .collect()
}

/// DeckCraft's custom path syntax (`M x y L x y C … Q … Z`), as its
/// renderer reads it.
fn parse_path(d: &str) -> BezPath {
    let mut bp = BezPath::new();
    let toks: Vec<&str> = d
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .collect();
    let num = |i: usize| {
        toks.get(i)
            .and_then(|t| t.parse::<f64>().ok())
            .filter(|v| v.is_finite())
    };
    let mut started = false;
    let mut i = 0;
    while i < toks.len() {
        match toks[i] {
            "M" => {
                if let (Some(x), Some(y)) = (num(i + 1), num(i + 2)) {
                    bp.move_to((x, y));
                    started = true;
                }
                i += 3;
            }
            "L" => {
                if let (Some(x), Some(y)) = (num(i + 1), num(i + 2)) {
                    if started {
                        bp.line_to((x, y));
                    } else {
                        bp.move_to((x, y));
                        started = true;
                    }
                }
                i += 3;
            }
            "C" => {
                if let (Some(a), Some(b), Some(c), Some(e), Some(f), Some(g), true) = (
                    num(i + 1),
                    num(i + 2),
                    num(i + 3),
                    num(i + 4),
                    num(i + 5),
                    num(i + 6),
                    started,
                ) {
                    bp.curve_to((a, b), (c, e), (f, g));
                }
                i += 7;
            }
            "Q" => {
                if let (Some(a), Some(b), Some(c), Some(e), true) =
                    (num(i + 1), num(i + 2), num(i + 3), num(i + 4), started)
                {
                    bp.quad_to((a, b), (c, e));
                }
                i += 5;
            }
            "Z" | "z" => {
                if started {
                    bp.close_path();
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    bp
}

/// The two ends of a stroked open path, each with its neighbouring point,
/// for arrowheads: ((start, next), (end, previous)). The head
/// (`a:headEnd`) is at the path's start, the tail (`a:tailEnd`) at its end.
pub(crate) fn ends(path: &BezPath) -> Option<((Point, Point), (Point, Point))> {
    let els = path.elements();
    let pt = |q: deckcraft_geom::Point| Point {
        x: q.x as f32,
        y: q.y as f32,
    };
    let start = match els.first()? {
        PathEl::MoveTo(a) => *a,
        _ => return None,
    };
    let after_start = match els.get(1)? {
        PathEl::LineTo(a) => *a,
        PathEl::QuadTo(a, _) => *a,
        PathEl::CurveTo(a, _, _) => *a,
        _ => return None,
    };
    let (end, before_end) = els.iter().rev().find_map(|el| match el {
        PathEl::LineTo(a) => Some((*a, None)),
        PathEl::QuadTo(c, a) => Some((*a, Some(*c))),
        PathEl::CurveTo(_, c, a) => Some((*a, Some(*c))),
        _ => None,
    })?;
    let before_end = before_end.unwrap_or_else(|| {
        // A line's previous point.
        let mut prev = start;
        for el in els {
            match el {
                PathEl::MoveTo(a) | PathEl::LineTo(a) if *a != end => prev = *a,
                PathEl::QuadTo(_, a) | PathEl::CurveTo(_, _, a) if *a != end => prev = *a,
                _ => {}
            }
        }
        prev
    });
    Some(((pt(start), pt(after_start)), (pt(end), pt(before_end))))
}

/// Shortens an open path by `start` at its first segment and `end` at its
/// last, along each segment's direction at that end. Only the end segments
/// move, and never by more than nine tenths of their length.
pub(crate) fn trim_ends(cmds: &mut [PathCommand], start: f32, end: f32) {
    fn pull(from: Point, towards: Point, by: f32) -> Option<Point> {
        let (dx, dy) = (towards.x - from.x, towards.y - from.y);
        let len = dx.hypot(dy);
        if by <= 0.0 || len < 1e-6 {
            return None;
        }
        let t = (by / len).min(0.9);
        Some(Point {
            x: from.x + dx * t,
            y: from.y + dy * t,
        })
    }
    if cmds.iter().any(|c| matches!(c, PathCommand::Close)) {
        return;
    }
    // Start: the move and the first point after it.
    if let (Some(PathCommand::MoveTo(a)), Some(next)) =
        (cmds.first().copied(), cmds.get(1).copied())
    {
        let towards = match next {
            PathCommand::LineTo(p) | PathCommand::QuadTo(p, _) | PathCommand::CubicTo(p, _, _) => {
                Some(p)
            }
            _ => None,
        };
        if let Some(p) = towards.and_then(|t| pull(a, t, start)) {
            cmds[0] = PathCommand::MoveTo(p);
        }
    }
    // End: the last point and the point before it.
    let n = cmds.len();
    if n >= 2 {
        let before = match cmds[n - 2] {
            PathCommand::MoveTo(p) | PathCommand::LineTo(p) => Some(p),
            PathCommand::QuadTo(_, p) | PathCommand::CubicTo(_, _, p) => Some(p),
            PathCommand::Close => None,
        };
        let moved = match cmds[n - 1] {
            PathCommand::LineTo(z) => before
                .and_then(|b| pull(z, b, end))
                .map(PathCommand::LineTo),
            PathCommand::QuadTo(c, z) => pull(z, c, end).map(|p| PathCommand::QuadTo(c, p)),
            PathCommand::CubicTo(c1, c2, z) => {
                pull(z, c2, end).map(|p| PathCommand::CubicTo(c1, c2, p))
            }
            _ => None,
        };
        if let Some(m) = moved {
            cmds[n - 1] = m;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_common_preset_has_its_own_outline() {
        let rect = commands(&rect_geometry(200.0, 100.0).paths[0].path);
        for name in COMMON_PRESETS {
            let shape = Shape {
                geom: Geom::preset(name),
                ..Default::default()
            };
            let (g, unknown) = geometry(&shape, 200.0, 100.0);
            assert!(!unknown, "{name} fell back to a rectangle");
            let cmds: Vec<PathCommand> = g.paths.iter().flat_map(|p| commands(&p.path)).collect();
            assert!(cmds.len() >= 2, "{name} has no outline");
            if name != "rect" {
                assert_ne!(cmds, rect, "{name} is drawn as a plain rectangle");
            }
        }
    }

    #[test]
    fn common_presets_are_forty_distinct_names() {
        let mut names = COMMON_PRESETS.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 40);
    }

    #[test]
    fn an_unknown_preset_is_its_bounding_rectangle() {
        let shape = Shape {
            geom: Geom::preset("notAShape"),
            ..Default::default()
        };
        let (g, unknown) = geometry(&shape, 50.0, 20.0);
        assert!(unknown);
        assert_eq!(commands(&g.paths[0].path), rect_path(50.0, 20.0));
    }

    #[test]
    fn custom_paths_scale_to_the_shape() {
        let shape = Shape {
            geom: Geom::Custom {
                paths: vec![deckcraft_model::CustomPath {
                    w: 10.0,
                    h: 10.0,
                    d: "M 0 0 L 10 0 L 10 10 Z".into(),
                    ..Default::default()
                }],
            },
            ..Default::default()
        };
        let (g, unknown) = geometry(&shape, 100.0, 50.0);
        assert!(!unknown);
        assert_eq!(
            commands(&g.paths[0].path),
            vec![
                PathCommand::MoveTo(Point { x: 0.0, y: 0.0 }),
                PathCommand::LineTo(Point { x: 100.0, y: 0.0 }),
                PathCommand::LineTo(Point { x: 100.0, y: 50.0 }),
                PathCommand::Close,
            ]
        );
    }

    #[test]
    fn trimming_shortens_only_the_ends() {
        let p = |x, y| Point { x, y };
        let mut cmds = vec![
            PathCommand::MoveTo(p(0.0, 0.0)),
            PathCommand::LineTo(p(100.0, 0.0)),
            PathCommand::LineTo(p(100.0, 50.0)),
        ];
        trim_ends(&mut cmds, 10.0, 5.0);
        assert_eq!(
            cmds,
            vec![
                PathCommand::MoveTo(p(10.0, 0.0)),
                PathCommand::LineTo(p(100.0, 0.0)),
                PathCommand::LineTo(p(100.0, 45.0)),
            ]
        );
    }

    /// Signed areas of a path's contours.
    fn contour_areas(path: &BezPath) -> Vec<f64> {
        path.subpaths()
            .map(|els| BezPath::from_vec(els.to_vec()).area())
            .collect()
    }

    #[test]
    fn holes_wind_against_their_outlines() {
        for name in ["donut", "frame", "noSmoking", "funnel"] {
            let shape = Shape {
                geom: Geom::preset(name),
                ..Default::default()
            };
            let (g, _) = geometry(&shape, 200.0, 100.0);
            let holed = g
                .paths
                .iter()
                .map(|p| contour_areas(&p.path))
                .find(|a| a.len() >= 2)
                .unwrap_or_else(|| panic!("{name} has no hole"));
            // The outline and the hole (the first two contours) now wind
            // opposite ways, so non-zero leaves the hole empty.
            assert!(holed[0] * holed[1] < 0.0, "{name}: {holed:?}");
            assert!(g.paths.iter().all(|p| !p.even_odd));
        }
    }

    #[test]
    fn a_custom_icon_keeps_its_holes() {
        // Outer square and inner square wound the same way; even-odd made
        // the inner one a hole.
        let shape = Shape {
            geom: Geom::Custom {
                paths: vec![deckcraft_model::CustomPath {
                    w: 10.0,
                    h: 10.0,
                    d: "M 0 0 L 10 0 L 10 10 L 0 10 Z M 3 3 L 7 3 L 7 7 L 3 7 Z".into(),
                    ..Default::default()
                }],
            },
            ..Default::default()
        };
        let (g, _) = geometry(&shape, 10.0, 10.0);
        let areas = contour_areas(&g.paths[0].path);
        assert_eq!(areas.len(), 2);
        assert!(areas[0] * areas[1] < 0.0, "{areas:?}");
    }

    #[test]
    fn a_sub_path_with_a_point_that_is_not_a_number_is_dropped() {
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        p.line_to((10.0, 0.0));
        p.curve_to((f64::NAN, 1.0), (2.0, 2.0), (3.0, 3.0));
        assert!(commands(&p).is_empty());
    }

    #[test]
    fn line_ends_point_back_along_the_path() {
        let mut p = BezPath::new();
        p.move_to((0.0, 0.0));
        p.line_to((10.0, 0.0));
        p.line_to((10.0, 20.0));
        let ((head, from_head), (tail, from_tail)) = ends(&p).unwrap();
        assert_eq!(
            (head, from_head),
            (Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 0.0 })
        );
        assert_eq!(
            (tail, from_tail),
            (Point { x: 10.0, y: 20.0 }, Point { x: 10.0, y: 0.0 })
        );
    }
}
