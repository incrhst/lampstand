//! T29 acceptance: the ten synthetic decks in `tests/decks/` parse into the
//! scenes in `tests/snapshots/`, and the properties each deck exists to
//! test hold. `tests/render.rs` draws them and compares with the references.
//!
//! The decks come from `tests/decks/generate.py` (python-pptx). To accept
//! new scenes after an intended change:
//!
//! INSTA_UPDATE=always cargo test -p lampstand-slides --test decks
//!
//! and review the diff before committing it.

use std::path::Path;

use lampstand_core::geom::Color;
use lampstand_core::scene::{Fit, Node, NodeKind, Paint, PathCommand, TextBlock};
use lampstand_layout::Fonts;
use lampstand_slides::{import_pptx, ImportError, IssueKind, ParsedDeck};
use serde_json::{json, Value};

fn parse(name: &str) -> ParsedDeck {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/decks")
        .join(format!("{name}.pptx"));
    let bytes = std::fs::read(&path).unwrap();
    // Loading the font set takes most of a parse's time in a debug build.
    thread_local!(static FONTS: Fonts = Fonts::bundled_only());
    FONTS.with(|fonts| import_pptx(&bytes, fonts).unwrap())
}

/// Every node, depth first.
fn walk(nodes: &[Node]) -> Vec<&Node> {
    let mut out = Vec::new();
    for n in nodes {
        out.push(n);
        if let NodeKind::Group(children) = &n.kind {
            out.extend(walk(children));
        }
    }
    out
}

fn texts(nodes: &[Node]) -> Vec<&TextBlock> {
    walk(nodes)
        .into_iter()
        .filter_map(|n| match &n.kind {
            NodeKind::Text(t) => Some(t),
            _ => None,
        })
        .collect()
}

fn text_of(t: &TextBlock) -> String {
    t.paragraphs
        .iter()
        .map(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The text block whose text starts with `prefix`.
fn find_text<'a>(nodes: &'a [Node], prefix: &str) -> &'a TextBlock {
    texts(nodes)
        .into_iter()
        .find(|t| text_of(t).starts_with(prefix))
        .unwrap_or_else(|| panic!("no text starting {prefix:?}"))
}

/// Floats rounded to hundredths, so snapshots do not depend on the last
/// bit of a platform's `sin` and `cos`.
fn rounded(v: Value) -> Value {
    match v {
        Value::Number(n) if !n.is_i64() && !n.is_u64() => {
            let f = n.as_f64().unwrap_or(0.0);
            json!((f * 100.0).round() / 100.0)
        }
        Value::Array(a) => Value::Array(a.into_iter().map(rounded).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, rounded(v))).collect()),
        other => other,
    }
}

fn snapshot(name: &str) {
    let deck = parse(name);
    let slides: Vec<Value> = deck
        .slides
        .iter()
        .map(|s| {
            json!({
                "hidden": s.hidden,
                "notes": s.notes,
                "builds": s.builds.len(),
                "scene": serde_json::to_value(&s.scene).unwrap(),
            })
        })
        .collect();
    let media: Vec<Value> = deck
        .media
        .iter()
        .map(|(hash, part)| json!({"blob": hash.to_string(), "type": part.content_type, "bytes": part.bytes.len()}))
        .collect();
    let issues: Vec<String> = deck
        .report
        .issues
        .iter()
        .map(|i| format!("{i:?}"))
        .collect();
    let value = rounded(json!({
        "width": deck.width,
        "height": deck.height,
        "media": media,
        "issues": issues,
        "slides": slides,
    }));
    insta::assert_json_snapshot!(name, value);
}

#[test]
fn snapshots() {
    for name in [
        "01-masters-placeholders",
        "02-theme-colors-fonts",
        "03-text",
        "04-preset-shapes",
        "05-groups-connectors",
        "06-fills-lines-shadows",
        "07-pictures-media",
        "08-tables",
        "09-backgrounds",
        "10-church-service",
    ] {
        snapshot(name);
    }
}

#[test]
fn masters_layouts_and_placeholders() {
    let deck = parse("01-masters-placeholders");
    assert_eq!((deck.width, deck.height), (960.0, 540.0));
    assert_eq!(deck.slides.len(), 5);
    let hidden: Vec<bool> = deck.slides.iter().map(|s| s.hidden).collect();
    assert_eq!(hidden, [false, false, false, true, false]);
    assert_eq!(
        deck.slides[0].notes,
        "Greet everyone.\nMention the coffee after the service."
    );
    for slide in &deck.slides {
        let nodes = &slide.scene.nodes;
        // White under the master's background colour, then the master's band.
        let fills: Vec<Option<&Paint>> = nodes
            .iter()
            .take(3)
            .map(|n| match &n.kind {
                NodeKind::Path(p) => p.fill.as_ref(),
                _ => None,
            })
            .collect();
        assert_eq!(fills[0], Some(&Paint::Solid(Color::WHITE)));
        assert_eq!(fills[1], Some(&Paint::Solid(Color::rgb(0xF4, 0xF1, 0xEA))));
        // Accent 1 of python-pptx's default theme.
        assert_eq!(fills[2], Some(&Paint::Solid(Color::rgb(0x4F, 0x81, 0xBD))));
    }
    // The title takes its colour and weight from the layout placeholder and
    // its position from the layout too.
    let title = find_text(&deck.slides[1].scene.nodes, "Inherited title style");
    let run = &title.paragraphs[0].runs[0];
    assert_eq!(run.weight, 700);
    assert_eq!(run.color, Color::rgb(0xC0, 0x50, 0x4D));
    let title_node = walk(&deck.slides[1].scene.nodes)
        .into_iter()
        .find(|n| matches!(&n.kind, NodeKind::Text(t) if std::ptr::eq(t, title)))
        .unwrap();
    assert_eq!(title_node.transform.f, 0.6 * 72.0);
    // Body levels indent by the master's body style, with bullets.
    let body = find_text(&deck.slides[1].scene.nodes, "First level point");
    assert_eq!(body.paragraphs[0].bullet.as_deref(), Some("•"));
    assert!(body.paragraphs[0].runs[0].size > body.paragraphs[2].runs[0].size);
    // A slide-number field shows the slide's number.
    assert!(texts(&deck.slides[4].scene.nodes)
        .iter()
        .any(|t| text_of(t) == "Slide 5"));
}

#[test]
fn theme_colours_and_fonts() {
    let deck = parse("02-theme-colors-fonts");
    let slide = &deck.slides[0].scene.nodes;
    let accent1 = walk(slide)
        .into_iter()
        .find_map(|n| match &n.kind {
            NodeKind::Group(kids) => kids.iter().find_map(|k| match &k.kind {
                NodeKind::Path(p) => p.fill.clone(),
                _ => None,
            }),
            _ => None,
        })
        .unwrap();
    // Accent 1 (#2E6F9E) at half its HSL lightness: lumMod 50%.
    assert_eq!(accent1, Paint::Solid(Color::rgb(23, 55, 79)));
    let title = find_text(&deck.slides[1].scene.nodes, "Theme fonts");
    assert_eq!(title.paragraphs[0].runs[0].font_family, "Cambria");
    let body = find_text(&deck.slides[1].scene.nodes, "Body text");
    assert_eq!(body.paragraphs[0].runs[0].font_family, "Calibri");
    assert_eq!(body.paragraphs[1].runs[0].font_family, "Cambria");
    assert_eq!(
        body.paragraphs[3].runs[0].color,
        Color::rgb(0x1F, 0x2A, 0x44)
    );
    // Montserrat is not bundled: reported once, with the fallback.
    let missing: Vec<&IssueKind> = deck
        .report
        .issues
        .iter()
        .map(|i| &i.kind)
        .filter(
            |k| matches!(k, IssueKind::MissingFont { requested, .. } if requested == "Montserrat"),
        )
        .collect();
    assert_eq!(missing.len(), 1);
    // Cambria and Calibri have metric-compatible substitutes.
    assert!(deck.report.issues.iter().any(|i| matches!(&i.kind,
        IssueKind::MissingFont { requested, substitute: Some(s) } if requested == "Calibri" && s == "Carlito")));
}

#[test]
fn text_runs_bullets_and_autofit() {
    let deck = parse("03-text");
    let runs = find_text(&deck.slides[0].scene.nodes, "Bold ");
    let first = &runs.paragraphs[0].runs;
    assert_eq!(first[0].weight, 700);
    assert!(first[1].italic && first[2].underline && first[3].strike);
    assert_eq!(first[4].color, Color::rgb(0xC0, 0x20, 0x20));
    assert_eq!(first[5].size, 40.0);
    assert_eq!(text_of(runs).lines().last(), Some("inside one paragraph"));

    let spacing = find_text(&deck.slides[0].scene.nodes, "Single spacing");
    assert_eq!(spacing.paragraphs[0].space_after, 20.0);
    assert!((spacing.paragraphs[1].line_spacing - 1.8).abs() < 1e-4);
    assert!((spacing.paragraphs[2].line_spacing - 2.0).abs() < 1e-4);

    let numbered = find_text(&deck.slides[1].scene.nodes, "Numbered one");
    let bullets: Vec<String> = numbered
        .paragraphs
        .iter()
        .map(|p| {
            p.bullet.clone().unwrap_or_else(|| {
                // Nested bullets are folded into the text (see `hang_bullet`).
                p.runs[0].text.split('\u{2009}').next().unwrap().to_string()
            })
        })
        .collect();
    assert_eq!(bullets, ["1.", "2.", "a)", "b)", "3.", "i.", "ii."]);

    let fitted = find_text(&deck.slides[2].scene.nodes, "This box holds");
    // 28 pt at the saved 62.5%, lines at 80% of single.
    assert_eq!(fitted.paragraphs[0].runs[0].size, 17.5);
    assert!((fitted.paragraphs[0].line_spacing - 0.96).abs() < 1e-4);
    let anchors: Vec<_> = ["Top", "Middle", "Bottom"]
        .iter()
        .map(|t| find_text(&deck.slides[2].scene.nodes, t).valign)
        .collect();
    use lampstand_core::scene::VAlign;
    assert_eq!(anchors, [VAlign::Top, VAlign::Middle, VAlign::Bottom]);
    // `wrap="none"` leaves room to run on.
    let unwrapped = find_text(&deck.slides[2].scene.nodes, "A text box that does not wrap");
    assert!(unwrapped.rect.width > 1000.0);
}

#[test]
fn common_presets_need_no_fallback() {
    let deck = parse("04-preset-shapes");
    assert!(
        deck.report
            .issues
            .iter()
            .all(|i| !matches!(i.kind, IssueKind::UnsupportedContent { .. })),
        "{:?}",
        deck.report.issues
    );
    for slide in &deck.slides {
        // Background (2), title, 20 shapes.
        assert_eq!(slide.scene.nodes.len(), 23);
    }
}

#[test]
fn groups_flips_and_line_ends() {
    let deck = parse("05-groups-connectors");
    let nodes = &deck.slides[0].scene.nodes;
    let groups: Vec<&Node> = nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Group(_)))
        .collect();
    // Outer group (with a nested group), rotated group, plus shapes with text.
    assert!(walk(nodes)
        .iter()
        .any(|n| matches!(&n.kind, NodeKind::Group(k) if k.iter().any(|c| matches!(c.kind, NodeKind::Group(_))))));
    assert!(!groups.is_empty());
    // The outer group is shrunk to 60%: its rounded rectangle is 3 × 1.5 in
    // in child space, so 129.6 × 64.8 pt on the slide, with type at 18 pt.
    // Its text box is inset by 0.29 of the 10.8 pt corner radius each side,
    // then by the 7.2 pt insets.
    let outer = find_text(nodes, "Outer group");
    let inset = 10.8 * 0.29289;
    assert!(
        (outer.rect.width - (129.6 - 2.0 * inset - 14.4)).abs() < 0.1,
        "{:?}",
        outer.rect
    );
    assert_eq!(outer.paragraphs[0].runs[0].size, 18.0);
    // A flipped shape's text is mirrored back.
    let flipped = walk(nodes)
        .into_iter()
        .find(|n| matches!(&n.kind, NodeKind::Group(k) if k.iter().any(|c| matches!(&c.kind, NodeKind::Text(t) if text_of(t).starts_with("Flipped arrow")))))
        .unwrap();
    assert!(flipped.transform.a < 0.0);
    let NodeKind::Group(kids) = &flipped.kind else {
        unreachable!()
    };
    let text = kids
        .iter()
        .find(|k| matches!(k.kind, NodeKind::Text(_)))
        .unwrap();
    assert!(text.transform.a < 0.0, "text is mirrored back");

    // Five straight lines: ends on the tail, and on the head for every other.
    let heads = deck.slides[1]
        .scene
        .nodes
        .iter()
        .map(|n| match &n.kind {
            NodeKind::Group(k) => k
                .iter()
                .filter(|c| matches!(&c.kind, NodeKind::Path(p) if p.fill.is_some() && p.stroke.is_none()))
                .count(),
            _ => 0,
        })
        .collect::<Vec<_>>();
    assert_eq!(&heads[3..8], &[1, 2, 1, 2, 1]);
}

#[test]
fn fills_outlines_and_shadows() {
    let deck = parse("06-fills-lines-shadows");
    let paths: Vec<_> = walk(&deck.slides[0].scene.nodes)
        .into_iter()
        .filter_map(|n| match &n.kind {
            NodeKind::Path(p) => Some(p),
            _ => None,
        })
        .collect();
    assert!(paths.iter().any(|p| matches!(&p.fill, Some(Paint::Solid(c)) if *c == Color { r: 0xC8, g: 0x55, b: 0x3D, a: 128 })));
    let linear: Vec<_> = paths
        .iter()
        .filter_map(|p| match &p.fill {
            Some(Paint::Linear { start, end, stops }) => Some((start, end, stops.len())),
            _ => None,
        })
        .collect();
    assert_eq!(linear.len(), 4);
    // python-pptx's angle runs counter-clockwise, so its 90° is written as
    // `ang` 270°: the first stop at the bottom.
    let (s, e, _) = linear[2];
    assert!((s.x - e.x).abs() < 0.01 && e.y < s.y, "{s:?} → {e:?}");
    assert_eq!(linear[3].2, 3);
    assert!(paths
        .iter()
        .any(|p| matches!(p.fill, Some(Paint::Radial { .. }))));

    let shadows: Vec<_> = walk(&deck.slides[1].scene.nodes)
        .into_iter()
        .filter_map(|n| match &n.kind {
            NodeKind::Path(p) => p.shadow,
            _ => None,
        })
        .collect();
    // Five outlined boxes take the theme's effect-style shadow (python-pptx's
    // shapes refer to it); the four rounded boxes set their own.
    assert_eq!(shadows.len(), 9);
    let own = &shadows[5..];
    // 4 pt at 45°, blur 4 pt drawn as a CSS radius of 2/3 of it.
    assert!((own[0].offset.x - 2.83).abs() < 0.01 && (own[0].offset.y - 2.83).abs() < 0.01);
    assert!((own[0].blur - 2.67).abs() < 0.01);
    // 6 pt at 135°: down and to the left, no blur.
    assert!(own[2].offset.x < 0.0 && own[2].offset.y > 0.0 && own[2].blur == 0.0);
    let dashed = walk(&deck.slides[1].scene.nodes)
        .into_iter()
        .filter_map(|n| match &n.kind {
            NodeKind::Path(p) => p.stroke.as_ref().and_then(|s| s.dash.clone()),
            _ => None,
        })
        .count();
    assert_eq!(dashed, 2);
}

#[test]
fn pictures_and_video_extract_their_media() {
    let deck = parse("07-pictures-media");
    let types: Vec<&str> = deck
        .media
        .values()
        .map(|m| m.content_type.as_str())
        .collect();
    assert!(
        types.contains(&"image/png")
            && types.contains(&"image/jpeg")
            && types.contains(&"video/mp4")
    );
    let images: Vec<_> = walk(&deck.slides[0].scene.nodes)
        .into_iter()
        .filter_map(|n| match &n.kind {
            NodeKind::Image(i) => Some(i),
            _ => None,
        })
        .collect();
    assert_eq!(images.len(), 5);
    assert!(images
        .iter()
        .all(|i| i.fit == Fit::Fill && deck.media.contains_key(&i.blob)));
    let crop = images[2].crop.unwrap();
    assert_eq!(
        (crop.x, crop.y, crop.width, crop.height),
        (0.5, 0.5, 0.5, 0.5)
    );
    let video = walk(&deck.slides[1].scene.nodes)
        .into_iter()
        .find_map(|n| match &n.kind {
            NodeKind::Video(v) => Some(v),
            _ => None,
        })
        .unwrap();
    assert_eq!(deck.media[&video.blob].content_type, "video/mp4");
}

#[test]
fn tables_take_their_style_and_merges() {
    let deck = parse("08-tables");
    let nodes = &deck.slides[0].scene.nodes;
    let header = find_text(nodes, "Time");
    // Medium Style 2, accent 1: white bold header text.
    assert_eq!(header.paragraphs[0].runs[0].color, Color::WHITE);
    assert_eq!(header.paragraphs[0].runs[0].weight, 700);
    let merged = find_text(nodes, "Merged header");
    assert!(
        (merged.rect.width - (4.4 * 72.0 - 14.4)).abs() < 0.5,
        "{:?}",
        merged.rect
    );
    let rows = find_text(nodes, "Rows merged");
    assert_eq!(rows.valign, lampstand_core::scene::VAlign::Middle);
    // The cell filled by hand.
    assert!(walk(nodes)
        .iter()
        .any(|n| matches!(&n.kind, NodeKind::Path(p) if p.fill == Some(Paint::Solid(Color::rgb(0xF2, 0xA5, 0x41))))));
}

#[test]
fn backgrounds_from_slide_and_layout() {
    let deck = parse("09-backgrounds");
    let bg = |i: usize| &deck.slides[i].scene.nodes[1].kind;
    assert!(
        matches!(bg(0), NodeKind::Path(p) if p.fill == Some(Paint::Solid(Color::rgb(0x1F, 0x2A, 0x44))))
    );
    assert!(matches!(bg(1), NodeKind::Path(p) if matches!(p.fill, Some(Paint::Linear { .. }))));
    assert!(matches!(bg(2), NodeKind::Image(i) if i.rect.width == 960.0 && i.fit == Fit::Fill));
    assert!(
        matches!(bg(3), NodeKind::Path(p) if p.fill == Some(Paint::Solid(Color::rgb(0x3B, 0xA9, 0x9C))))
    );
}

#[test]
fn a_four_by_three_service_deck() {
    let deck = parse("10-church-service");
    assert_eq!((deck.width, deck.height), (720.0, 540.0));
    assert!(deck.slides.last().unwrap().hidden);
    let lyrics = find_text(&deck.slides[0].scene.nodes, "Amazing grace");
    assert!(lyrics.shadow.is_some(), "lyrics keep their text shadow");
    assert_eq!(lyrics.paragraphs.len(), 4);
    let psalm = find_text(&deck.slides[1].scene.nodes, "The Lord is my shepherd");
    assert!(psalm.paragraphs.iter().all(|p| p.bullet.is_none()));
    assert!(psalm.paragraphs[0].runs[0].size < 32.0 * 0.78);
    let points = find_text(&deck.slides[3].scene.nodes, "God is with us");
    let bullets: Vec<_> = points
        .paragraphs
        .iter()
        .map(|p| p.bullet.clone().unwrap())
        .collect();
    assert_eq!(bullets, ["1.", "2.", "3."]);
    assert_eq!(
        deck.slides[3].notes,
        "Reveal one point at a time.\nPause after the second."
    );
}

#[test]
fn node_ids_are_unique_and_below_the_compositor_range() {
    for name in [
        "01-masters-placeholders",
        "05-groups-connectors",
        "08-tables",
    ] {
        let deck = parse(name);
        for slide in &deck.slides {
            let mut ids: Vec<u32> = walk(&slide.scene.nodes).iter().map(|n| n.id).collect();
            assert!(ids
                .iter()
                .all(|id| *id < lampstand_core::scene::COMPOSITOR_NODE_BASE));
            let n = ids.len();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), n, "{name}: duplicate node IDs");
        }
    }
}

#[test]
fn rejects_what_is_not_a_presentation() {
    let fonts = Fonts::bundled_only();
    assert_eq!(
        import_pptx(b"hello", &fonts),
        Err(ImportError::NotAPresentation)
    );
    // An OLE compound file holding an encrypted package.
    let mut ole = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    ole.resize(4096, 0);
    let err = import_pptx(&ole, &fonts).unwrap_err();
    assert!(
        matches!(err, ImportError::NotAPresentation | ImportError::Encrypted),
        "{err:?}"
    );
}

#[test]
fn paths_have_finite_points() {
    let deck = parse("04-preset-shapes");
    for slide in &deck.slides {
        for n in walk(&slide.scene.nodes) {
            if let NodeKind::Path(p) = &n.kind {
                for c in &p.commands {
                    let pts = match c {
                        PathCommand::MoveTo(a) | PathCommand::LineTo(a) => vec![a],
                        PathCommand::QuadTo(a, b) => vec![a, b],
                        PathCommand::CubicTo(a, b, c) => vec![a, b, c],
                        PathCommand::Close => vec![],
                    };
                    assert!(pts.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
                }
            }
        }
    }
}
