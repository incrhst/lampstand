//! Decks a hostile guest speaker could send: the synthetic decks with one
//! value pushed far outside its DrawingML range. Each must parse into a
//! sane scene (or be refused with an error), never abort or exhaust memory.

use std::io::{Cursor, Read, Write};
use std::path::Path;

use lampstand_core::scene::{Node, NodeKind, COMPOSITOR_NODE_BASE};
use lampstand_layout::Fonts;
use lampstand_slides::{import_pptx, ImportError, ParsedDeck};

fn deck_bytes(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/decks")
        .join(format!("{name}.pptx"));
    std::fs::read(path).unwrap()
}

/// `name` with `from` replaced by `to` (once) in package part `part`.
fn patched(name: &str, part: &str, from: &str, to: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(deck_bytes(name))).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut found = false;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        let file_name = file.name().unwrap().to_string();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        if file_name == part {
            let xml = String::from_utf8(data).unwrap();
            assert!(xml.contains(from), "{part} has no {from:?}");
            data = xml.replacen(from, to, 1).into_bytes();
            found = true;
        }
        out.start_file(file_name, zip::write::SimpleFileOptions::default())
            .unwrap();
        out.write_all(&data).unwrap();
    }
    assert!(found, "no part {part}");
    out.finish().unwrap().into_inner()
}

fn parse(bytes: &[u8]) -> ParsedDeck {
    import_pptx(bytes, &Fonts::bundled_only()).unwrap()
}

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

/// The review's blocker: a bullet indent of 9e18 EMU used to become a
/// 483 TB allocation and abort the process.
#[test]
fn a_huge_bullet_indent_is_clamped() {
    let bytes = patched(
        "03-text",
        "ppt/slides/slide2.xml",
        r#"marL="762000" indent="-381000""#,
        r#"marL="9000000000000000000" indent="-8999999000000000000""#,
    );
    let deck = parse(&bytes);
    for n in walk(&deck.slides[1].scene.nodes) {
        if let NodeKind::Text(t) = &n.kind {
            for p in &t.paragraphs {
                assert!(p.indent <= 4032.0, "indent {}", p.indent);
                let len: usize = p.runs.iter().map(|r| r.text.len()).sum();
                assert!(len < 1000, "a paragraph of {len} bytes");
            }
        }
    }
}

/// A shape at the top of the content ID range left no room for the nodes
/// the mapping adds, which then all got the same ID.
#[test]
fn a_shape_id_at_the_top_of_the_range_leaves_ids_unique() {
    let bytes = patched(
        "06-fills-lines-shadows",
        "ppt/slides/slide1.xml",
        r#"<p:cNvPr id="3""#,
        r#"<p:cNvPr id="16777215""#,
    );
    let deck = parse(&bytes);
    let mut ids: Vec<u32> = walk(&deck.slides[0].scene.nodes)
        .iter()
        .map(|n| n.id)
        .collect();
    assert!(ids.contains(&(COMPOSITOR_NODE_BASE - 1)));
    assert!(ids.iter().all(|id| *id < COMPOSITOR_NODE_BASE));
    let n = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), n, "duplicate node IDs");
}

#[test]
fn huge_font_sizes_line_widths_and_slides_are_clamped() {
    let bytes = patched(
        "06-fills-lines-shadows",
        "ppt/slides/slide2.xml",
        r#"<a:ln w="9525">"#,
        r#"<a:ln w="9000000000000000000">"#,
    );
    let deck = parse(&bytes);
    for n in walk(&deck.slides[1].scene.nodes) {
        if let NodeKind::Path(p) = &n.kind {
            if let Some(s) = &p.stroke {
                assert!(s.width <= 1584.0, "stroke {}", s.width);
            }
        }
    }

    let bytes = patched(
        "03-text",
        "ppt/slides/slide1.xml",
        r#"sz="4000""#,
        r#"sz="2147483647""#,
    );
    let deck = parse(&bytes);
    for n in walk(&deck.slides[0].scene.nodes) {
        if let NodeKind::Text(t) = &n.kind {
            assert!(t
                .paragraphs
                .iter()
                .flat_map(|p| &p.runs)
                .all(|r| r.size <= 4000.0));
        }
    }

    let bytes = patched(
        "08-tables",
        "ppt/presentation.xml",
        r#"<p:sldSz cx="12191695""#,
        r#"<p:sldSz cx="9000000000000000000""#,
    );
    let deck = parse(&bytes);
    // DeckCraft falls back to its default size here; either way the slide
    // stays within `ST_SlideSizeCoordinate`.
    assert!((72.0..=4032.0).contains(&deck.width), "{}", deck.width);
}

#[test]
fn a_huge_custom_dash_is_bounded() {
    let many = "<a:ds d=\"1\" sp=\"1\"/>".repeat(5000);
    let bytes = patched(
        "05-groups-connectors",
        "ppt/slides/slide2.xml",
        r#"<a:prstDash val="dash"/>"#,
        &format!("<a:custDash>{many}</a:custDash>"),
    );
    let deck = parse(&bytes);
    for n in walk(&deck.slides[1].scene.nodes) {
        if let NodeKind::Path(p) = &n.kind {
            if let Some(dash) = p.stroke.as_ref().and_then(|s| s.dash.as_ref()) {
                assert!(dash.len() <= 32 && dash.iter().all(|d| *d >= 0.1));
            }
        }
    }
}

#[test]
fn a_deck_that_unpacks_past_the_limit_is_refused() {
    // Declare one part as 4 GiB in the zip's central directory. The check
    // reads only the directory, so nothing is inflated.
    let mut bytes = deck_bytes("01-masters-placeholders");
    let central = bytes
        .windows(4)
        .position(|w| w == [0x50, 0x4b, 0x01, 0x02])
        .unwrap();
    bytes[central + 24..central + 28].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
    match import_pptx(&bytes, &Fonts::bundled_only()) {
        Err(ImportError::TooLarge { unpacked, limit }) => {
            assert!(unpacked > limit, "{unpacked} > {limit}");
        }
        other => panic!("{other:?}"),
    }
}
