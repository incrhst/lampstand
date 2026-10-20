//! What a deck may cost to parse, checked before DeckCraft reads it.
//!
//! DeckCraft reads each XML part into a DOM and builds its model from it,
//! and the model can be far bigger than the file: a table row is padded
//! out to its grid's width, a part is parsed again for every reference to
//! it, and a paragraph, run, shape or cell becomes a model object of
//! kilobytes. Its own caps (4 million elements a part, 1,000 × 1,000 cells
//! a table, 10,000 slides) allow gigabytes from a deck of a few KB.
//!
//! So before DeckCraft sees a deck, this estimates what parsing it will
//! allocate and refuses it past `LIMITS`. Every zip entry is read once as a
//! stream, whatever its name, extension or content type, through the
//! quick-xml version DeckCraft parses with and with its settings, up to
//! where DeckCraft's parser would stop (its first error, or 256 levels
//! deep; binary media stops within a few events). It is charged:
//!
//! - for every element, whatever its name: `ELEMENT` bytes, plus the
//!   measured cost of the model object for the elements DeckCraft builds
//!   one from (`model_cost`);
//! - for every attribute: `ATTRIBUTE` bytes plus its key and value;
//! - for every byte of names, text, CDATA, comments and processing
//!   instructions: two (the DOM's copy and the model's);
//! - for every table: `CELL` bytes for each cell DeckCraft materialises
//!   (its rows times its grid's width);
//! - once more for every further reference to a part from another part's
//!   attributes (a slide listed twice in `sldIdLst` is parsed twice),
//!   with everything that part references in turn;
//! - for every entry, XML or not: its inflated bytes twice (`HELD`), for
//!   DeckCraft's package and its copy of each media part.
//!
//! One total bounds all of it (`LIMITS`), so the estimate stands for the
//! whole import's peak, media included.
//!
//! The charges are measured, with room to spare, by `tests/memory.rs`,
//! which also parses hostile decks built from the whole DrawingML
//! vocabulary at the limit and checks the real peak stays under a fixed
//! ceiling.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read};

use deckcraft_quick_xml::events::Event;
use deckcraft_quick_xml::Reader;

use crate::ImportError;

/// What one deck may cost.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    /// The estimated peak of the whole import, in bytes: the parse
    /// estimate plus every entry's inflated bytes `HELD` times.
    pub memory: u64,
}

/// One number for the whole import (T87). The largest real deck measured
/// (the Apache POI corpus's HowWeRefactor) peaks at 123 MB, a 200-slide
/// sermon deck with 100 photos at 148 MB, and a deck with one 350 MB
/// embedded video is charged about 700 MB; see `tests/memory.rs`.
pub(crate) const LIMITS: Limits = Limits { memory: 1 << 30 };

/// How many copies of each entry's inflated bytes an import holds at once:
/// DeckCraft's `Package` keeps every entry inflated, and `media_for`
/// copies each media part out of it. So every entry is charged twice,
/// whatever the XML scan made of it: binary media stops the scan at once
/// and costs nothing there.
const HELD: u64 = 2;

/// Every element: an `El` with a name `String` and two `Vec`s.
const ELEMENT: u64 = 160;
/// Every attribute: a `(String, String)`.
const ATTRIBUTE: u64 = 100;
/// Every cell DeckCraft materialises, empty or not.
const CELL: u64 = 6_000;

/// The model object DeckCraft (and the mapping after it) builds from an
/// element, by local name, measured with `tests/memory.rs`. Everything
/// else is charged `ELEMENT` only.
fn model_cost(local: &[u8]) -> u64 {
    match local {
        b"p" => 8_000,
        b"r" | b"fld" | b"br" => 4_000,
        b"sp" | b"pic" | b"grpSp" | b"graphicFrame" | b"cxnSp" | b"contentPart" => 6_000,
        _ => 0,
    }
}

/// DeckCraft's own nesting limit: its parser fails a part past it.
const MAX_DEPTH: usize = 256;
/// DeckCraft's table caps: rows, cells a row, grid columns.
const MAX_TABLE_SIDE: u64 = 1000;
/// References followed when charging a part again for each reference.
const MAX_REFERENCE_DEPTH: usize = 8;

/// DeckCraft doesn't parse an XML part bigger than this
/// (`deckcraft_pptx` `MAX_XML_PART`), so neither does the scan: quick-xml
/// buffers a whole tag, and one tag can be most of a part.
const MAX_XML_PART: u64 = 96 << 20;

/// UTF-16 parts are decoded in memory first, as DeckCraft does; real
/// decks never have large ones.
const MAX_UTF16_PART: u64 = 16 << 20;

fn too_large(what: String, found: u64, limit: u64) -> ImportError {
    ImportError::TooLarge { what, found, limit }
}

/// The deck's estimated cost, or the first limit it passes.
pub(crate) fn check_with(bytes: &[u8], limits: &Limits) -> Result<Cost, ImportError> {
    // DeckCraft opened this package (`sniff`). If our zip reader can't, the
    // two disagree about what it holds, and it is not checked: refuse it.
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| ImportError::Corrupt(format!("unreadable package: {e}")))?;
    let declared = zip.decompressed_size().unwrap_or(0);
    let unpacked_limit = limits.memory / HELD;
    if declared > u128::from(unpacked_limit) {
        let found = u64::try_from(declared).unwrap_or(u64::MAX);
        return Err(too_large("unpacked bytes".into(), found, unpacked_limit));
    }

    // Relationships first: which attribute values in a part refer to which
    // parts. DeckCraft finds a part's relationships by their path
    // (lowercased) and nothing else, so only those entries matter.
    // The map they make is charged too, before the parts are scanned.
    let mut rels: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut rels_memory = 0_u64;
    for i in 0..zip.len() {
        let Ok(entry) = zip.by_index(i) else { continue };
        let name = norm(&entry.name().unwrap_or_default());
        let Some(source) = rels_source(&name) else {
            continue;
        };
        let room = limits.memory.saturating_sub(rels_memory);
        let (map, memory) = relationships(BufReader::new(entry), &source, room);
        rels_memory += memory;
        if rels_memory > limits.memory {
            return Err(too_large(
                format!("estimated bytes to parse (relationships of {source})"),
                rels_memory,
                limits.memory,
            ));
        }
        rels.insert(source, map);
    }

    let mut deck = Cost {
        memory: rels_memory,
        ..Cost::default()
    };
    let mut parts: BTreeMap<String, Part> = BTreeMap::new();
    for i in 0..zip.len() {
        // An entry that can't be read (an unsupported compression method)
        // can't be read by DeckCraft either, which uses the same codecs.
        let Ok(entry) = zip.by_index(i) else { continue };
        let raw_name = entry.name().unwrap_or_default().to_string();
        let name = norm(&raw_name);
        let ids: BTreeSet<String> = rels
            .get(&name)
            .map(|r| r.keys().cloned().collect())
            .unwrap_or_default();
        // Read at most one byte past what is left of the budget once the
        // entry's bytes are held `HELD` times.
        let room = limits.memory.saturating_sub(deck.memory);
        let part = scan(entry.take(room / HELD + 1), &ids, room).map_err(|found| {
            too_large(format!("UTF-16 bytes in {raw_name}"), found, MAX_UTF16_PART)
        })?;
        deck.unpacked += part.unpacked;
        deck.memory = deck
            .memory
            .saturating_add(part.memory)
            .saturating_add(HELD.saturating_mul(part.unpacked));
        if deck.memory > limits.memory {
            return Err(too_large(
                format!("estimated bytes to parse (at {raw_name})"),
                deck.memory,
                limits.memory,
            ));
        }
        parts.insert(name, part);
    }

    // Parts referenced more than once are parsed more than once.
    let mut uses: BTreeMap<String, u64> = BTreeMap::new();
    for (source, part) in &parts {
        let Some(targets) = rels.get(source) else {
            continue;
        };
        for (id, n) in &part.references {
            if let Some(target) = targets.get(id) {
                *uses.entry(target.clone()).or_default() += n;
            }
        }
    }
    for (target, n) in uses.into_iter().filter(|(_, n)| *n > 1) {
        let once = reach(&target, &parts, &rels, &mut BTreeSet::new(), 0);
        deck.memory = deck.memory.saturating_add((n - 1).saturating_mul(once));
        if deck.memory > limits.memory {
            return Err(too_large(
                format!("estimated bytes to parse ({target} is read {n} times)"),
                deck.memory,
                limits.memory,
            ));
        }
    }
    Ok(deck)
}

/// A part's cost with everything it references, each part counted once.
fn reach(
    name: &str,
    parts: &BTreeMap<String, Part>,
    rels: &BTreeMap<String, BTreeMap<String, String>>,
    seen: &mut BTreeSet<String>,
    depth: usize,
) -> u64 {
    if depth > MAX_REFERENCE_DEPTH || !seen.insert(name.to_string()) {
        return 0;
    }
    let own = parts.get(name).map_or(0, |p| p.memory);
    let children: Vec<String> = rels
        .get(name)
        .map(|r| r.values().cloned().collect())
        .unwrap_or_default();
    children.iter().fold(own, |sum, child| {
        sum.saturating_add(reach(child, parts, rels, seen, depth + 1))
    })
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cost {
    pub unpacked: u64,
    pub memory: u64,
}

/// One entry, scanned.
#[derive(Debug, Default)]
struct Part {
    unpacked: u64,
    memory: u64,
    /// How often each of the part's relationship IDs appears as an
    /// attribute value.
    references: BTreeMap<String, u64>,
}

/// Wraps a reader to count the bytes taken from it.
struct Counted<R> {
    inner: R,
    count: u64,
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.count += n as u64;
        Ok(n)
    }
}

/// Reads one entry and charges it. Counting stops once the entry costs
/// more than `room` (the rest is still read, to count its bytes). `Err`
/// is an oversized UTF-16 part.
fn scan(r: impl Read, ids: &BTreeSet<String>, room: u64) -> Result<Part, u64> {
    let mut r = BufReader::with_capacity(64 * 1024, Counted { inner: r, count: 0 });
    let utf16 = matches!(
        r.fill_buf().map(|b| b.get(..2)),
        Ok(Some([0xFF, 0xFE] | [0xFE, 0xFF]))
    );
    let mut part = if utf16 {
        let mut raw = Vec::new();
        let _ = r.by_ref().take(MAX_UTF16_PART + 1).read_to_end(&mut raw);
        if raw.len() as u64 > MAX_UTF16_PART {
            return Err(raw.len() as u64);
        }
        let text = decode_utf16(&raw);
        drop(raw);
        count(text.as_bytes(), ids, room)
    } else {
        count((&mut r).take(MAX_XML_PART), ids, room)
    };
    // Whatever the parser left unread still inflates.
    let _ = std::io::copy(&mut r, &mut std::io::sink());
    part.unpacked = r.get_ref().count;
    Ok(part)
}

/// A table being read: its grid width and its rows' cell counts.
#[derive(Default)]
struct Table {
    in_grid: bool,
    grid: u64,
    /// Cells in each row DeckCraft reads (its first 1,000).
    rows: Vec<u64>,
}

impl Table {
    /// The most cells DeckCraft holds at once: it builds every row's own
    /// cells (up to 1,000), then pads or cuts the row to the grid's width
    /// (the widest row's when there is no grid).
    fn cells(&self) -> u64 {
        let widest = self.rows.iter().copied().max().unwrap_or(0);
        let width = if self.grid > 0 { self.grid } else { widest }.min(MAX_TABLE_SIDE);
        self.rows.iter().map(|own| (*own).max(width)).sum()
    }
}

fn local(name: &[u8]) -> &[u8] {
    name.rsplit(|b| *b == b':').next().unwrap_or(name)
}

/// The part's cost as DeckCraft's parser and model would allocate it, up
/// to the parser's first error or its depth limit.
fn count(input: impl BufRead, ids: &BTreeSet<String>, room: u64) -> Part {
    let mut reader = Reader::from_reader(input);
    {
        // DeckCraft's settings (`deckcraft_pptx::xml::parse`).
        let c = reader.config_mut();
        c.trim_text(false);
        c.check_end_names = false;
        c.allow_unmatched_ends = true;
        c.expand_empty_elements = false;
    }
    let mut part = Part::default();
    let mut depth = 0_usize;
    let mut tables: Vec<Table> = Vec::new();
    let mut buf = Vec::new();
    loop {
        let event = match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,
            Ok(event) => event,
        };
        match &event {
            Event::Start(b) | Event::Empty(b) => {
                let name = b.name();
                let local = local(name.as_ref());
                part.memory += ELEMENT + 2 * name.as_ref().len() as u64 + model_cost(local);
                for a in b.attributes().with_checks(false) {
                    let Ok(a) = a else { break };
                    part.memory += ATTRIBUTE + 2 * (a.key.as_ref().len() + a.value.len()) as u64;
                    if !ids.is_empty() {
                        if let Ok(v) = std::str::from_utf8(&a.value) {
                            if ids.contains(v) {
                                *part.references.entry(v.to_string()).or_default() += 1;
                            }
                        }
                    }
                }
                let empty = matches!(event, Event::Empty(_));
                if let Some(t) = tables.last_mut() {
                    match local {
                        b"tblGrid" if !empty => t.in_grid = true,
                        b"gridCol" if t.in_grid => t.grid += 1,
                        b"tr" if (t.rows.len() as u64) < MAX_TABLE_SIDE => t.rows.push(0),
                        b"tc" => {
                            if let Some(cells) = t.rows.last_mut() {
                                *cells = (*cells + 1).min(MAX_TABLE_SIDE);
                            }
                        }
                        _ => {}
                    }
                }
                if local == b"tbl" && !empty {
                    tables.push(Table::default());
                }
                if !empty {
                    depth += 1;
                    if depth >= MAX_DEPTH {
                        break;
                    }
                }
            }
            Event::End(e) => {
                depth = depth.saturating_sub(1);
                match local(e.name().as_ref()) {
                    b"tbl" => {
                        if let Some(t) = tables.pop() {
                            part.memory += CELL * t.cells();
                        }
                    }
                    b"tblGrid" => {
                        if let Some(t) = tables.last_mut() {
                            t.in_grid = false;
                        }
                    }
                    _ => {}
                }
            }
            Event::Text(t) => part.memory += 2 * t.len() as u64,
            Event::CData(t) => part.memory += 2 * t.len() as u64,
            Event::Comment(t) => part.memory += 2 * t.len() as u64,
            Event::DocType(t) => part.memory += 2 * t.len() as u64,
            Event::PI(t) => part.memory += 2 * t.len() as u64,
            Event::GeneralRef(t) => part.memory += 2 * t.len() as u64,
            Event::Decl(t) => part.memory += 2 * t.len() as u64,
            Event::Eof => break,
        }
        if part.memory > room {
            break;
        }
        buf.clear();
    }
    // DeckCraft keeps elements left open at the end of a part.
    for t in tables {
        part.memory += CELL * t.cells();
    }
    part
}

/// Relationship ID → target part, for the relationships of `source`, as
/// DeckCraft resolves them (external targets left out), and what the map
/// costs. Reading stops once it costs more than `room`.
fn relationships(xml: impl BufRead, source: &str, room: u64) -> (BTreeMap<String, String>, u64) {
    let mut reader = Reader::from_reader(xml);
    let mut out = BTreeMap::new();
    let mut memory = 0_u64;
    let base = source
        .rsplit_once('/')
        .map_or("", |(dir, _)| dir)
        .to_string();
    let mut buf = Vec::new();
    loop {
        let b = match reader.read_event_into(&mut buf) {
            Ok(Event::Start(b) | Event::Empty(b)) => b,
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {
                buf.clear();
                continue;
            }
        };
        if local(b.name().as_ref()) == b"Relationship" {
            let (mut id, mut target, mut external) = (None, None, false);
            for a in b.attributes().with_checks(false).flatten() {
                let value = String::from_utf8_lossy(&a.value).into_owned();
                match a.key.as_ref() {
                    b"Id" => id = Some(value),
                    b"Target" => target = Some(value),
                    b"TargetMode" => external = value.eq_ignore_ascii_case("external"),
                    _ => {}
                }
            }
            if let (Some(id), Some(target), false) = (id, target, external) {
                let target = norm(&resolve(&base, &target));
                memory += 2 * ATTRIBUTE + (id.len() + target.len()) as u64;
                out.insert(id, target);
                if memory > room {
                    break;
                }
            }
        }
        buf.clear();
    }
    (out, memory)
}

/// A package part name as DeckCraft keys it.
fn norm(name: &str) -> String {
    name.trim_start_matches('/')
        .replace('\\', "/")
        .to_ascii_lowercase()
}

/// The part whose relationships a `…/_rels/<file>.rels` entry holds (the
/// package itself for `_rels/.rels`).
fn rels_source(name: &str) -> Option<String> {
    let file = name.strip_suffix(".rels")?;
    let (dir, file) = file.rsplit_once('/')?;
    let parent = dir.strip_suffix("_rels")?.trim_end_matches('/');
    Some(if parent.is_empty() {
        file.to_string()
    } else {
        format!("{parent}/{file}")
    })
}

/// A target resolved against the source's folder, as DeckCraft resolves it.
fn resolve(base: &str, target: &str) -> String {
    let target = percent_decode(target);
    let target = target.split('#').next().unwrap_or("");
    let mut segs: Vec<&str> = if target.starts_with('/') {
        vec![]
    } else {
        base.split('/').filter(|s| !s.is_empty()).collect()
    };
    for s in target.split('/') {
        match s {
            "" | "." => {}
            ".." => {
                segs.pop();
            }
            s => segs.push(s),
        }
    }
    segs.join("/")
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let hex = |c: u8| (c as char).to_digit(16);
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// UTF-16 to text, as DeckCraft decodes a UTF-16 part.
fn decode_utf16(bytes: &[u8]) -> String {
    let le = bytes.starts_with(&[0xFF, 0xFE]);
    let units: Vec<u16> = bytes
        .get(2..)
        .unwrap_or(&[])
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            if le {
                u16::from_le_bytes(*pair)
            } else {
                u16::from_be_bytes(*pair)
            }
        })
        .collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cost(xml: &[u8]) -> u64 {
        scan(xml, &BTreeSet::new(), u64::MAX).unwrap().memory
    }

    #[test]
    fn every_element_costs_whatever_its_name() {
        // `<1/>`, `<é/>` and `<-/>` are elements to DeckCraft's parser.
        for junk in ["<1/>", "<é/>", "<-/>", "<x/>"] {
            assert!(
                cost(junk.repeat(1000).as_bytes()) >= 1000 * ELEMENT,
                "{junk}"
            );
        }
    }

    #[test]
    fn attributes_cost_whatever_their_name() {
        let attrs: String = (0..1000).map(|i| format!(" {i}é=\"\"")).collect();
        assert!(cost(format!("<x{attrs}/>").as_bytes()) >= 1000 * ATTRIBUTE);
    }

    #[test]
    fn text_before_the_root_does_not_hide_xml() {
        let mut part = b"not an image ".to_vec();
        part.extend(b"<1/>".repeat(1000));
        assert!(cost(&part) >= 1000 * ELEMENT);
    }

    #[test]
    fn model_objects_cost_what_they_build() {
        assert!(cost(&b"<a:p/>".repeat(100)) >= 100 * 8_000);
        assert!(cost(&b"<x:p/>".repeat(100)) >= 100 * 8_000, "any prefix");
    }

    #[test]
    fn padded_and_unclosed_tables_cost_their_cells() {
        // 500 grid columns and 100 empty rows: 50,000 cells from 600 elements.
        let grid = format!(
            "<a:tbl><a:tblGrid>{}</a:tblGrid>",
            "<a:gridCol/>".repeat(500)
        );
        let padded = format!("{grid}{}</a:tbl>", "<a:tr/>".repeat(100));
        assert!(cost(padded.as_bytes()) >= 50_000 * CELL);
        let open = format!("{grid}{}", "<a:tr/>".repeat(100));
        assert!(cost(open.as_bytes()) >= 50_000 * CELL);
        // Cells beyond the grid are built before the row is cut to it.
        let wide = format!(
            "<a:tbl><a:tblGrid><a:gridCol/></a:tblGrid>{}</a:tbl>",
            format!("<a:tr>{}</a:tr>", "<a:tc/>".repeat(100)).repeat(20)
        );
        assert!(cost(wide.as_bytes()) >= 2_000 * CELL);
        // No grid: the widest row sets the width.
        let rows = format!(
            "<a:tbl>{}</a:tbl>",
            format!("<a:tr>{}</a:tr>", "<a:tc/>".repeat(50)).repeat(20)
        );
        assert!(cost(rows.as_bytes()) >= 1_000 * CELL);
    }

    #[test]
    fn binary_media_stops_the_count_early() {
        // Random bytes, as compressed media looks: the parser fails fast.
        let mut x: u32 = 1;
        let noise: Vec<u8> = (0..(4 << 20))
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect();
        let part = scan(&noise[..], &BTreeSet::new(), u64::MAX).unwrap();
        assert_eq!(part.unpacked, 4 << 20);
        assert!(
            part.memory < 1 << 20,
            "{} bytes estimated for noise",
            part.memory
        );
    }

    #[test]
    fn utf16_parts_are_counted() {
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("<1/>".repeat(100).encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert!(cost(&utf16) >= 100 * ELEMENT);
    }

    #[test]
    fn relationship_paths_resolve_as_deckcraft_resolves_them() {
        assert_eq!(
            rels_source("ppt/slides/_rels/slide1.xml.rels").as_deref(),
            Some("ppt/slides/slide1.xml")
        );
        assert_eq!(rels_source("_rels/.rels").as_deref(), Some(""));
        assert_eq!(rels_source("ppt/slides/slide1.xml"), None);
        assert_eq!(
            resolve("ppt/slides", "../slideLayouts/slideLayout1.xml"),
            "ppt/slideLayouts/slideLayout1.xml"
        );
        assert_eq!(
            resolve("ppt", "/ppt/slides/slide%201.xml"),
            "ppt/slides/slide 1.xml"
        );
    }

    #[test]
    fn understated_and_oversized_entries_stay_within_the_budget() {
        use std::io::Write;
        let package = |declared: Option<u32>| {
            let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            out.start_file(
                "ppt/media/image1.png",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            out.write_all(&[7_u8; 64 << 10]).unwrap();
            let mut bytes = out.finish().unwrap().into_inner();
            if let Some(size) = declared {
                let central = bytes
                    .windows(4)
                    .position(|w| w == [0x50, 0x4b, 0x01, 0x02])
                    .unwrap();
                bytes[central + 24..central + 28].copy_from_slice(&size.to_le_bytes());
            }
            bytes
        };
        let small = Limits { memory: 2000 };
        // Declared honestly: refused.
        assert!(matches!(
            check_with(&package(None), &small),
            Err(ImportError::TooLarge { .. })
        ));
        // Declared as 10 bytes: the zip reader yields at most the declared
        // size (and so does DeckCraft's, with the same codec), so the entry
        // costs 10 bytes to anyone who reads it.
        assert!(check_with(&package(Some(10)), &small).is_ok());
    }

    /// What real decks cost: `LAMPSTAND_CORPUS=dir cargo test -p
    /// lampstand-slides --lib -- --ignored corpus_costs --nocapture`.
    #[test]
    #[ignore]
    fn corpus_costs() {
        let dir = std::env::var("LAMPSTAND_CORPUS").expect("LAMPSTAND_CORPUS");
        let unlimited = Limits {
            memory: u64::MAX / 2,
        };
        let mut worst = (0, String::new());
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let Ok(cost) = check_with(&std::fs::read(&path).unwrap(), &unlimited) else {
                continue;
            };
            if cost.memory > worst.0 {
                worst = (cost.memory, path.display().to_string());
            }
        }
        eprintln!("most: {} MB estimated, {}", worst.0 >> 20, worst.1);
    }
}
