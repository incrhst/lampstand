//! Decks a hostile guest speaker could send: the synthetic decks with one
//! value pushed far outside its DrawingML range. Each must parse into a
//! sane scene (or be refused with an error), never abort or exhaust memory.

mod common;

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

/// `name` rebuilt entry by entry: `f` gets each entry's name and bytes and
/// returns the entries to write in its place. Stored uncompressed.
fn rebuild(name: &str, mut f: impl FnMut(&str, Vec<u8>) -> Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(deck_bytes(name))).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        let file_name = file.name().unwrap().to_string();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        for (n, d) in f(&file_name, data) {
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            out.start_file(n, options).unwrap();
            out.write_all(&d).unwrap();
        }
    }
    out.finish().unwrap().into_inner()
}

/// Deck 08 with 600,000 tiny elements in its first slide.
fn heavy_slide() -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(deck_bytes("08-tables"))).unwrap();
    let mut slide = String::new();
    zip.by_name("ppt/slides/slide1.xml")
        .unwrap()
        .read_to_string(&mut slide)
        .unwrap();
    // 70,000 paragraphs: about 560 MB of model to DeckCraft, over the
    // 512 MB budget, from 420 KB of XML.
    slide.replacen("</p:spTree>", &format!("{}</p:spTree>", heavy()), 1)
}

/// Content over the parse budget on its own.
const HEAVY: &str = "<a:p/>";
const HEAVY_COUNT: usize = 70_000;

fn heavy() -> String {
    HEAVY.repeat(HEAVY_COUNT)
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
fn a_slide_over_the_budget_is_refused() {
    let bytes = patched(
        "08-tables",
        "ppt/slides/slide1.xml",
        "</p:spTree>",
        &format!("{}</p:spTree>", heavy()),
    );
    let (what, found, limit) = refused(&bytes);
    assert_eq!(what, "estimated bytes to parse (at ppt/slides/slide1.xml)");
    assert!(found > limit);
}

#[test]
fn a_deck_over_the_budget_is_refused() {
    // A third of the heavy content on each of four slides: each part is
    // allowed, the deck is not.
    let filler = format!("<p:spTree>{}", HEAVY.repeat(HEAVY_COUNT / 3));
    let parts: Vec<String> = [1, 2, 3, 5]
        .iter()
        .map(|n| format!("ppt/slides/slide{n}.xml"))
        .collect();
    let edits: Vec<(&str, &str, &str)> = parts
        .iter()
        .map(|part| (part.as_str(), "<p:spTree>", filler.as_str()))
        .collect();
    let bytes = patched_many("01-masters-placeholders", &edits);
    assert!(refused(&bytes)
        .0
        .starts_with("estimated bytes to parse (at ppt/slides/slide"));
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
        lampstand_slides::IssueKind::UnsupportedContent { what } if what.contains("drawn-part budget")
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

/// The third review: DeckCraft lowercases part names, so the heavy slide
/// stored as `slide1.XML` is still the slide. The budget doesn't look at
/// names.
#[test]
fn a_heavy_slide_under_an_upper_case_name_is_refused() {
    let slide = heavy_slide();
    let bytes = rebuild("08-tables", |n, d| match n {
        "ppt/slides/slide1.xml" => {
            vec![("ppt/slides/slide1.XML".into(), slide.clone().into_bytes())]
        }
        _ => vec![(n.to_string(), d)],
    });
    let (what, _, _) = refused(&bytes);
    assert_eq!(what, "estimated bytes to parse (at ppt/slides/slide1.XML)");
}

/// The third review: the heavy slide as `slide1.dat`, with its relationship
/// and content type pointing there.
#[test]
fn a_heavy_slide_under_another_extension_is_refused() {
    let slide = heavy_slide();
    let bytes = rebuild("08-tables", |n, d| {
        let text = || String::from_utf8(d.clone()).unwrap();
        match n {
            "ppt/slides/slide1.xml" => {
                vec![("ppt/slides/slide1.dat".into(), slide.clone().into_bytes())]
            }
            "ppt/slides/_rels/slide1.xml.rels" => {
                vec![("ppt/slides/_rels/slide1.dat.rels".into(), d)]
            }
            "ppt/_rels/presentation.xml.rels" => {
                vec![(
                    n.to_string(),
                    text()
                        .replace("slides/slide1.xml", "slides/slide1.dat")
                        .into_bytes(),
                )]
            }
            "[Content_Types].xml" => {
                vec![(
                    n.to_string(),
                    text()
                        .replace("/ppt/slides/slide1.xml", "/ppt/slides/slide1.dat")
                        .into_bytes(),
                )]
            }
            _ => vec![(n.to_string(), d)],
        }
    });
    let (what, _, _) = refused(&bytes);
    assert_eq!(what, "estimated bytes to parse (at ppt/slides/slide1.dat)");
}

/// Parts nobody special-cased: a slide in UTF-16 (DeckCraft decodes it) and
/// an unreferenced entry that starts like a PNG. Every entry draws from
/// the same budget.
#[test]
fn parts_of_any_kind_draw_from_the_budget() {
    let slide = heavy_slide();
    let utf16: Vec<u8> = [0xFF, 0xFE]
        .into_iter()
        .chain(slide.encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    let bytes = rebuild("08-tables", |n, d| match n {
        "ppt/slides/slide1.xml" => vec![(n.to_string(), utf16.clone())],
        _ => vec![(n.to_string(), d)],
    });
    assert_eq!(
        refused(&bytes).0,
        "estimated bytes to parse (at ppt/slides/slide1.xml)"
    );

    let mut blob = b"\x89PNG\r\n\x1a\n".to_vec();
    blob.extend(heavy().bytes());
    let bytes = rebuild("08-tables", |n, d| {
        let mut out = vec![(n.to_string(), d)];
        if n == "[Content_Types].xml" {
            out.push(("ppt/embeddings/blob.bin".into(), blob.clone()));
        }
        out
    });
    assert_eq!(
        refused(&bytes).0,
        "estimated bytes to parse (at ppt/embeddings/blob.bin)"
    );
}

/// The third review's second repro: a master with many shapes, shown on
/// many slides, costs elements once (well inside the budget) but nodes on
/// every slide. The deck stops at its node budget and says so.
#[test]
fn a_heavy_master_on_many_slides_stops_at_the_node_budget() {
    let shapes: String = (0..10_000)
        .map(|i| {
            format!(
                r#"<p:sp><p:nvSpPr><p:cNvPr id="{}" name=""/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="12700" cy="12700"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="000000"/></a:solidFill><a:ln><a:noFill/></a:ln></p:spPr></p:sp>"#,
                1000 + i
            )
        })
        .collect();
    let slides = 110;
    let blank = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/></p:spTree></p:cSld></p:sld>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout7.xml"/></Relationships>"#;
    let bytes = rebuild("06-fills-lines-shadows", |n, d| {
        let text = || String::from_utf8(d.clone()).unwrap();
        let mut out = Vec::new();
        match n {
            "ppt/slideMasters/slideMaster1.xml" => out.push((
                n.to_string(),
                text()
                    .replacen("</p:spTree>", &format!("{shapes}</p:spTree>"), 1)
                    .into_bytes(),
            )),
            "ppt/presentation.xml" => {
                let ids: String = (0..slides)
                    .map(|k| format!(r#"<p:sldId id="{}" r:id="rIdX{k}"/>"#, 300 + k))
                    .collect();
                out.push((
                    n.to_string(),
                    text()
                        .replacen("</p:sldIdLst>", &format!("{ids}</p:sldIdLst>"), 1)
                        .into_bytes(),
                ));
            }
            "ppt/_rels/presentation.xml.rels" => {
                let more: String = (0..slides)
                    .map(|k| format!(r#"<Relationship Id="rIdX{k}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/extra{k}.xml"/>"#))
                    .collect();
                out.push((
                    n.to_string(),
                    text()
                        .replacen("</Relationships>", &format!("{more}</Relationships>"), 1)
                        .into_bytes(),
                ));
            }
            "[Content_Types].xml" => {
                let more: String = (0..slides)
                    .map(|k| format!(r#"<Override PartName="/ppt/slides/extra{k}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#))
                    .collect();
                out.push((
                    n.to_string(),
                    text()
                        .replacen("</Types>", &format!("{more}</Types>"), 1)
                        .into_bytes(),
                ));
                for k in 0..slides {
                    out.push((
                        format!("ppt/slides/extra{k}.xml"),
                        blank.as_bytes().to_vec(),
                    ));
                    out.push((
                        format!("ppt/slides/_rels/extra{k}.xml.rels"),
                        rels.as_bytes().to_vec(),
                    ));
                }
            }
            _ => out.push((n.to_string(), d)),
        }
        out
    });
    let deck = parse(&bytes);
    assert_eq!(deck.slides.len(), slides + 2);
    let total: usize = deck.slides.iter().map(|s| walk(&s.scene.nodes).len()).sum();
    // 10,000 master shapes on 112 slides would be 1.1 million nodes.
    assert!(total <= 1_000_000 + 10, "{total} nodes");
    assert!(
        deck.slides[3].scene.nodes.len() > 10_000,
        "early slides show the master"
    );
    assert!(deck.slides.last().unwrap().scene.nodes.is_empty());
    assert!(deck.report.issues.iter().any(|i| i.slide.is_none()
        && matches!(&i.kind, lampstand_slides::IssueKind::UnsupportedContent { what } if what.contains("later slides"))));
}

/// No real deck is refused by the budget or loses slides to the node caps:
/// `LAMPSTAND_CORPUS=dir cargo test -p lampstand-slides --test hostile
/// -- --ignored corpus --nocapture` (the Apache POI slideshow corpus
/// passes: its corrupt files fail as `NotAPresentation` or `Corrupt`).
#[test]
#[ignore]
fn corpus_is_not_refused() {
    let dir = std::env::var("LAMPSTAND_CORPUS").expect("LAMPSTAND_CORPUS");
    let fonts = Fonts::bundled_only();
    let (mut ok, mut failed) = (0, 0);
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "pptx") {
            continue;
        }
        match import_pptx(&std::fs::read(&path).unwrap(), &fonts) {
            Ok(deck) => {
                ok += 1;
                let capped = deck.report.issues.iter().any(|i| matches!(
                    &i.kind,
                    lampstand_slides::IssueKind::UnsupportedContent { what } if what.contains("drawn-part") || what.contains("later slides")
                ));
                assert!(!capped, "{}: hit a node budget", path.display());
            }
            Err(ImportError::TooLarge { what, found, limit }) => {
                panic!("{}: refused, {what} {found} > {limit}", path.display())
            }
            Err(e) => {
                failed += 1;
                eprintln!("{}: {e:?}", path.display());
            }
        }
    }
    eprintln!("{ok} parsed, {failed} failed");
}

/// A realistic large service: 100 lyric slides on a photo and 100 psalm
/// slides with autofit (deck 10's first two slides, copied). The budgets
/// must leave it whole.
#[test]
fn a_200_slide_service_deck_parses_whole() {
    let bytes = common::with_copies("10-church-service", &[(1, 100), (2, 100)]);
    let deck = parse(&bytes);
    assert_eq!(deck.slides.len(), 205);
    assert!(deck.slides.iter().all(|s| !s.scene.nodes.is_empty()));
    assert!(
        !deck.report.issues.iter().any(|i| matches!(
            &i.kind,
            lampstand_slides::IssueKind::UnsupportedContent { .. }
        )),
        "{:?}",
        deck.report.issues
    );
}
