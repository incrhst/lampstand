//! Tables: cell fills, borders and text from the cells and the table style.
//!
//! Table styles the file defines (`ppt/tableStyles.xml`) are applied part by
//! part, as PowerPoint applies them. Built-in styles the file only names by
//! GUID are drawn as DeckCraft draws them (ported from `deckcraft-render`'s
//! `table.rs`, MIT OR Apache-2.0): Medium Style 2, the default, and the
//! light, medium 4 and dark families.
//!
//! Rows keep their stored height (`a:tr/@h`). PowerPoint grows a row to fit
//! its text; files PowerPoint and python-pptx save already store the grown
//! height, but this mapping cannot measure text to grow it further.

use deckcraft_color::{ColorTransform, Rgba, SchemeSlot};
use deckcraft_model::resolve::Ctx;
use deckcraft_model::style::{ColorRef, Fill, Line};
use deckcraft_model::table::{Cell, Table, TableBorder, TablePart, TablePartStyle, TableStyleDef};
use deckcraft_model::text::{Anchor, AutoFit, BodyProps, TextBody};
use deckcraft_model::Shape;
use lampstand_core::geom::{Point, Rect, Transform};
use lampstand_core::scene::{Node, NodeKind, Paint, PathCommand, PathNode, Stroke};

use super::shape::{node, Local};
use super::text::{self, Frame};
use super::{geometry, paint, SlideCx};

/// A cell's look from the table style.
#[derive(Default)]
struct Look {
    fill: Option<(Fill, Option<Rgba>)>,
    text: Option<Rgba>,
    bold: bool,
    /// Every side, when the style draws a plain grid.
    border: Option<(Rgba, f64)>,
    /// Left, right, top, bottom from a file-defined style; a transparent
    /// colour means no line.
    sides: [Option<(Rgba, f64)>; 4],
}

pub(crate) fn nodes(
    sc: &mut SlideCx,
    ctx: &Ctx,
    s: &Shape,
    t: &Table,
    _local: &Local,
) -> Vec<Node> {
    let xs = edges(&t.cols);
    let ys = edges(&t.rows.iter().map(|r| r.height).collect::<Vec<_>>());
    let mut fills = Vec::new();
    let mut lines = Vec::new();
    let mut texts = Vec::new();
    for (r, row) in t.rows.iter().enumerate() {
        for (c, cell) in row.cells.iter().enumerate() {
            if cell.is_covered() {
                continue;
            }
            let c1 = (c + cell.grid_span.max(1) as usize).min(xs.len() - 1);
            let r1 = (r + cell.row_span.max(1) as usize).min(ys.len() - 1);
            let (Some(&x0), Some(&x1), Some(&y0), Some(&y1)) =
                (xs.get(c), xs.get(c1), ys.get(r), ys.get(r1))
            else {
                continue;
            };
            let rect = Rect {
                x: x0 as f32,
                y: y0 as f32,
                width: (x1 - x0) as f32,
                height: (y1 - y0) as f32,
            };
            let look = look(ctx, t, r, c);
            // Fill: the cell's own, else the style's.
            let fill = match &cell.fill {
                Some(f) => Some((f.clone(), None)),
                None => look.fill.clone(),
            };
            if let Some((f, ph)) = fill {
                if let Some(p) = paint::paint(ctx, &f, ph, rect.width, rect.height) {
                    let id = sc.fresh_id();
                    fills.push(node(
                        id,
                        Transform::translate(rect.x, rect.y),
                        NodeKind::Path(PathNode {
                            commands: geometry::rect_path(rect.width, rect.height),
                            fill: Some(p),
                            stroke: None,
                            shadow: None,
                        }),
                    ));
                }
            }
            // Borders: left, right, top, bottom.
            let corners = [
                ((rect.x, rect.y), (rect.x, rect.y + rect.height)),
                (
                    (rect.x + rect.width, rect.y),
                    (rect.x + rect.width, rect.y + rect.height),
                ),
                ((rect.x, rect.y), (rect.x + rect.width, rect.y)),
                (
                    (rect.x, rect.y + rect.height),
                    (rect.x + rect.width, rect.y + rect.height),
                ),
            ];
            for (side, ((ax, ay), (bx, by))) in corners.into_iter().enumerate() {
                let stroke = match cell.borders.get(side).and_then(Option::as_ref) {
                    Some(line) => paint::stroke(ctx, line, None, rect.width, rect.height),
                    None => look.sides[side]
                        .or(look.border)
                        .filter(|(c, w)| c.a > 0 && *w > 0.0)
                        .map(|(c, w)| Stroke {
                            paint: Paint::Solid(paint::color(c)),
                            width: w as f32,
                            dash: None,
                        }),
                };
                if let Some(stroke) = stroke {
                    let id = sc.fresh_id();
                    lines.push(node(
                        id,
                        Transform::IDENTITY,
                        NodeKind::Path(PathNode {
                            commands: vec![
                                PathCommand::MoveTo(Point { x: ax, y: ay }),
                                PathCommand::LineTo(Point { x: bx, y: by }),
                            ],
                            fill: None,
                            stroke: Some(stroke),
                            shadow: None,
                        }),
                    ));
                }
            }
            // Text.
            let (body, bp) = cell_body(cell, &look);
            if body.is_empty() {
                continue;
            }
            let frame = Frame { rect, shadow: None };
            if let Some(placed) = text::block(sc, ctx, s, &body, &bp, frame) {
                let id = sc.fresh_id();
                texts.push(node(id, Transform::IDENTITY, NodeKind::Text(placed.block)));
            }
        }
    }
    fills.extend(lines);
    fills.extend(texts);
    fills
}

/// Running edges 0, s0, s0 + s1, ... of a list of sizes.
fn edges(sizes: &[f64]) -> Vec<f64> {
    let mut out = Vec::with_capacity(sizes.len() + 1);
    let mut at = 0.0;
    out.push(at);
    for s in sizes {
        at += if s.is_finite() { s.max(0.0) } else { 0.0 };
        out.push(at);
    }
    out
}

/// A cell's text with the style's colour and weight, and its body
/// properties: margins (default 0.1 in and 0.05 in), anchor, wrapping.
fn cell_body(cell: &Cell, look: &Look) -> (TextBody, BodyProps) {
    let mut body = cell.text.clone();
    for p in &mut body.paragraphs {
        for run in &mut p.runs {
            if run.props.fill.is_none() {
                if let Some(tc) = look.text {
                    run.props.fill = Some(Fill::solid(ColorRef::rgb(tc)));
                }
            }
            if look.bold && run.props.bold.is_none() {
                run.props.bold = Some(true);
            }
        }
    }
    let [ml, mr, mt, mb] = cell.margins.unwrap_or([7.2, 7.2, 3.6, 3.6]);
    let bp = BodyProps {
        inset_l: Some(ml),
        inset_r: Some(mr),
        inset_t: Some(mt),
        inset_b: Some(mb),
        anchor: Some(cell.anchor.unwrap_or(Anchor::Top)),
        wrap: Some(true),
        autofit: Some(AutoFit::None),
        vert: cell.vertical,
        ..Default::default()
    };
    (body, bp)
}

fn look(ctx: &Ctx, t: &Table, r: usize, c: usize) -> Look {
    if let Some(def) = ctx
        .pres
        .table_styles
        .iter()
        .find(|d| d.id.eq_ignore_ascii_case(&t.style))
    {
        return look_def(ctx, def, t, r, c);
    }
    builtin(ctx, t, r, c)
}

/// The look of cell (`r`, `c`) from a style the file defines: each part
/// that applies (whole table, bands, first and last rows and columns,
/// corners, in that order) sets the fill, text, and the borders on its
/// region's edges (its inside lines within it).
fn look_def(ctx: &Ctx, def: &TableStyleDef, t: &Table, r: usize, c: usize) -> Look {
    let (rows, cols) = (t.rows.len(), t.cols.len());
    let (r1, c1) = t.cell(r, c).map_or((r, c), |cell| {
        (
            r + cell.row_span.max(1) as usize - 1,
            c + cell.grid_span.max(1) as usize - 1,
        )
    });
    let last_r = rows.saturating_sub(1);
    let last_c = cols.saturating_sub(1);
    let (r1, c1) = (r1.min(last_r), c1.min(last_c));
    let header = t.first_row && r == 0;
    let total = t.last_row && r1 == last_r && rows > 1;
    let first_col = t.first_col && c == 0;
    let last_col = t.last_col && c1 == last_c && cols > 1;
    let body_r = r.saturating_sub(usize::from(t.first_row));
    let body_c = c.saturating_sub(usize::from(t.first_col));
    let mut look = Look::default();
    for part in TablePart::ALL {
        // (applies, region: first row, last row, first col, last col)
        let (on, (rr0, rr1, cc0, cc1)) = match part {
            TablePart::WholeTbl => (true, (0, last_r, 0, last_c)),
            TablePart::Band1H => (
                t.band_row && !header && !total && body_r % 2 == 0,
                (r, r1, 0, last_c),
            ),
            TablePart::Band2H => (
                t.band_row && !header && !total && body_r % 2 == 1,
                (r, r1, 0, last_c),
            ),
            TablePart::Band1V => (
                t.band_col && !first_col && !last_col && body_c % 2 == 0,
                (0, last_r, c, c1),
            ),
            TablePart::Band2V => (
                t.band_col && !first_col && !last_col && body_c % 2 == 1,
                (0, last_r, c, c1),
            ),
            TablePart::LastCol => (last_col, (0, last_r, c, last_c)),
            TablePart::FirstCol => (first_col, (0, last_r, 0, c1)),
            TablePart::LastRow => (total, (r, last_r, 0, last_c)),
            TablePart::FirstRow => (header, (0, r1, 0, last_c)),
            TablePart::SeCell => (total && last_col, (r, r1, c, c1)),
            TablePart::SwCell => (total && first_col, (r, r1, c, c1)),
            TablePart::NeCell => (header && last_col, (r, r1, c, c1)),
            TablePart::NwCell => (header && first_col, (r, r1, c, c1)),
        };
        if !on {
            continue;
        }
        let Some(ps) = def.part(part) else { continue };
        if let Some(f) = style_fill(ctx, ps) {
            look.fill = Some(f);
        }
        if let Some(tc) = &ps.text_color {
            look.text = Some(ctx.color(tc, None));
        }
        if let Some(b) = ps.bold {
            look.bold = b;
        }
        // Left, right, top, bottom: the part's edge, or its inside line.
        let pick = [
            if c == cc0 { 0 } else { 5 },
            if c1 == cc1 { 1 } else { 5 },
            if r == rr0 { 2 } else { 4 },
            if r1 == rr1 { 3 } else { 4 },
        ];
        for (side, i) in pick.iter().enumerate() {
            if let Some(Some(b)) = ps.borders.get(*i) {
                look.sides[side] = Some(border(ctx, b));
            }
        }
    }
    look
}

/// A style part's fill; a theme fill reference becomes the theme's fill
/// with its colour as the placeholder colour.
fn style_fill(ctx: &Ctx, ps: &TablePartStyle) -> Option<(Fill, Option<Rgba>)> {
    if let Some((idx, col)) = &ps.fill_ref {
        let ph = ctx.color(col, None);
        return Some(
            match ctx
                .master
                .theme
                .format
                .fills
                .get((*idx as usize).saturating_sub(1))
            {
                _ if *idx == 0 => (Fill::None, None),
                Some(f) => (f.clone(), Some(ph)),
                None => (Fill::solid(ColorRef::rgb(ph)), None),
            },
        );
    }
    ps.fill.clone().map(|f| (f, None))
}

fn border(ctx: &Ctx, b: &TableBorder) -> (Rgba, f64) {
    let width = |l: &Line| l.width.unwrap_or(0.75);
    match b {
        TableBorder::Line { line } => match &line.fill {
            Some(Fill::Solid { color }) => (ctx.color(color, None), width(line)),
            Some(Fill::Gradient(g)) => (
                g.stops
                    .first()
                    .map_or(Rgba::BLACK, |s| ctx.color(&s.color, None)),
                width(line),
            ),
            _ => (Rgba::TRANSPARENT, 0.0),
        },
        TableBorder::Ref { idx, color } => {
            if *idx == 0 {
                return (Rgba::TRANSPARENT, 0.0);
            }
            let w = ctx
                .master
                .theme
                .format
                .lines
                .get((*idx as usize).saturating_sub(1))
                .and_then(|l| l.width)
                .unwrap_or(0.75);
            (ctx.color(color, None), w)
        }
    }
}

/// Built-in styles named only by GUID (DeckCraft maps the GUID to
/// `medium2-accent1` and so on).
fn builtin(ctx: &Ctx, t: &Table, r: usize, c: usize) -> Look {
    let slot = match t.style.rsplit('-').next().unwrap_or("accent1") {
        "accent2" => SchemeSlot::Accent2,
        "accent3" => SchemeSlot::Accent3,
        "accent4" => SchemeSlot::Accent4,
        "accent5" => SchemeSlot::Accent5,
        "accent6" => SchemeSlot::Accent6,
        "tx1" => SchemeSlot::Tx1,
        _ => SchemeSlot::Accent1,
    };
    let a = ctx.color(&ColorRef::scheme(slot), None);
    let kind = t.style.split('-').next().unwrap_or("medium2");
    let header = t.first_row && r == 0;
    let total = t.last_row && r + 1 == t.rows.len();
    let first_col = t.first_col && c == 0;
    let last_col = t.last_col && c + 1 == t.cols.len();
    let body_r = if t.first_row { r.wrapping_sub(1) } else { r };
    let band = t.band_row && !header && !total && body_r % 2 == 0;
    let band_c = t.band_col && c.is_multiple_of(2);
    let tint = |k: i32| deckcraft_color::apply(a, &[ColorTransform::Tint(k)]);
    let solid = |c: Rgba| Some((Fill::solid(ColorRef::rgb(c)), None));
    let white = Rgba::WHITE;
    let strong = header || total || first_col || last_col;
    match kind {
        "none" => Look::default(),
        "grid" => Look {
            border: Some((ctx.color(&ColorRef::scheme(SchemeSlot::Tx1), None), 0.75)),
            ..Look::default()
        },
        "light1" => Look {
            fill: if band || band_c {
                solid(tint(20000))
            } else {
                None
            },
            bold: strong,
            border: header.then_some((a, 1.0)),
            ..Look::default()
        },
        "light2" => Look {
            fill: if header { solid(a) } else { None },
            text: header.then_some(white),
            bold: header || total || first_col,
            border: Some((a, 1.0)),
            ..Look::default()
        },
        "dark1" => Look {
            fill: solid(if header {
                Rgba::BLACK
            } else if band || band_c {
                deckcraft_color::apply(a, &[ColorTransform::Shade(60000)])
            } else {
                deckcraft_color::apply(a, &[ColorTransform::Shade(75000)])
            }),
            text: Some(white),
            bold: header || total || first_col,
            border: Some((white, 1.0)),
            ..Look::default()
        },
        "medium4" => Look {
            fill: solid(if header || band || band_c {
                tint(40000)
            } else {
                tint(20000)
            }),
            bold: strong,
            border: Some((a, 0.75)),
            ..Look::default()
        },
        // Medium Style 2: strong header, banded light tints, white grid.
        _ => Look {
            fill: solid(if strong {
                a
            } else if band || band_c {
                tint(40000)
            } else {
                tint(20000)
            }),
            text: strong.then_some(white),
            bold: strong,
            border: Some((white, 1.0)),
            ..Look::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_run_on_from_zero() {
        assert_eq!(
            edges(&[10.0, 20.0, -5.0, 5.0]),
            vec![0.0, 10.0, 30.0, 30.0, 35.0]
        );
    }
}
