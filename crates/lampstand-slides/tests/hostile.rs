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
    patched_many(name, &[(part, from, to)])
}

/// `name` with each (part, from, to) replacement made once, in order. Parts
/// are stored uncompressed, so building a large test deck stays quick.
fn patched_many(name: &str, edits: &[(&str, &str, &str)]) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(deck_bytes(name))).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut done = 0;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        let file_name = file.name().unwrap().to_string();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        for (part, from, to) in edits.iter().filter(|(p, _, _)| *p == file_name) {
            let xml = String::from_utf8(data).unwrap();
            assert!(xml.contains(from), "{part} has no {from:?}");
            data = xml.replacen(from, to, 1).into_bytes();
            done += 1;
        }
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        out.start_file(file_name, options).unwrap();
        out.write_all(&data).unwrap();
    }
    assert_eq!(done, edits.len(), "not every edit applied");
    out.finish().unwrap().into_inner()
}

/// The limit a refused deck hit.
fn refused(bytes: &[u8]) -> (String, u64, u64) {
    match import_pptx(bytes, &Fonts::bundled_only()) {
        Err(ImportError::TooLarge { what, found, limit }) => {
            assert!(found > limit, "{what}: {found} > {limit}");
            (what, found, limit)
        }
        other => panic!("{other:?}"),
    }
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
    assert_eq!(refused(&bytes).0, "unpacked bytes");
}

/// The re-review's repro, scaled to stay quick: one slide of 600,000 tiny
/// elements (a 1000 × 400 table is 2.8 million) would take DeckCraft's DOM
/// to gigabytes. It is refused from a streaming count, before parsing.
#[test]
fn a_slide_of_too_many_elements_is_refused() {
    let cells = format!("<a:tr h=\"1\">{}</a:tr>", "<a:tc/>".repeat(600_000));
    let bytes = patched(
        "08-tables",
        "ppt/slides/slide1.xml",
        r#"<a:tr h="609600">"#,
        &format!(r#"{cells}<a:tr h="609600">"#),
    );
    let (what, found, _) = refused(&bytes);
    assert_eq!(what, "XML elements in ppt/slides/slide1.xml");
    assert!(found >= 600_000);
}

#[test]
fn a_part_too_big_to_parse_is_refused() {
    // 17 MB of text in one run.
    let text = "x".repeat(17 << 20);
    let bytes = patched(
        "08-tables",
        "ppt/slides/slide1.xml",
        "<a:t>Time</a:t>",
        &format!("<a:t>{text}</a:t>"),
    );
    assert_eq!(refused(&bytes).0, "bytes in ppt/slides/slide1.xml");
}

#[test]
fn a_deck_of_too_many_elements_is_refused() {
    // 400,000 elements on each of four slides: each part is allowed, the
    // deck is not.
    let filler = format!("<p:spTree>{}", "<a:x/>".repeat(400_000));
    let parts: Vec<String> = [1, 2, 3, 5]
        .iter()
        .map(|n| format!("ppt/slides/slide{n}.xml"))
        .collect();
    let edits: Vec<(&str, &str, &str)> = parts
        .iter()
        .map(|part| (part.as_str(), "<p:spTree>", filler.as_str()))
        .collect();
    let bytes = patched_many("01-masters-placeholders", &edits);
    assert_eq!(refused(&bytes).0, "XML elements");
}

/// A table under the parse limits that would still give the renderer
/// 120,000 nodes: the slide stops at `MAX_NODES` and says so.
#[test]
fn a_slide_stops_at_its_node_budget() {
    let cols = 100;
    let grid = format!(
        "<a:tblGrid>{}",
        r#"<a:gridCol w="12700"/>"#.repeat(cols - 3)
    );
    let cell =
        "<a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>x</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc>";
    let row = format!(r#"<a:tr h="12700">{}</a:tr>"#, cell.repeat(cols));
    let rows = format!(r#"{}<a:tr h="609600">"#, row.repeat(200));
    let bytes = patched_many(
        "08-tables",
        &[
            ("ppt/slides/slide1.xml", "<a:tblGrid>", grid.as_str()),
            (
                "ppt/slides/slide1.xml",
                r#"<a:tr h="609600">"#,
                rows.as_str(),
            ),
        ],
    );
    let deck = parse(&bytes);
    let nodes = walk(&deck.slides[0].scene.nodes).len();
    assert!(nodes <= 100_000 + 10, "{nodes} nodes");
    assert!(
        nodes > 50_000,
        "the table was mapped up to the budget: {nodes}"
    );
    assert!(deck.report.issues.iter().any(|i| matches!(
        &i.kind,
        lampstand_slides::IssueKind::UnsupportedContent { what } if what.contains("drawn parts")
    )));
}

/// The re-review's full-size repro: run by hand with `--ignored` under
/// `/usr/bin/time -l` to see the peak memory.
#[test]
#[ignore]
fn reviewer_repro_full_size() {
    let cell =
        "<a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>x</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc>";
    let row = format!(r#"<a:tr h="12700">{}</a:tr>"#, cell.repeat(400));
    let rows = format!(r#"{}<a:tr h="609600">"#, row.repeat(1000));
    let bytes = patched(
        "08-tables",
        "ppt/slides/slide1.xml",
        r#"<a:tr h="609600">"#,
        &rows,
    );
    eprintln!("{:?}", refused(&bytes));
}
