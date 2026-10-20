//! Peak memory of parsing hostile decks, measured with a counting global
//! allocator (this test binary only).
//!
//! The parse budget (`src/pptx/budget.rs`) estimates what a deck will cost
//! and refuses it past the limit before DeckCraft builds anything. This
//! measures what the budget lets through: for each kind of content, the
//! largest amount the budget accepts, parsed, with the real peak
//! allocation measured. The kinds cover what earlier reviews used to get
//! around it (junk element names, digits and non-ASCII, attributes, table
//! padding, a slide listed many times) and the model objects DeckCraft
//! expands (paragraphs, runs, shapes, cells), plus random nested content
//! drawn from the whole DrawingML vocabulary (`tests/vocabulary.txt`, every
//! element name in the Apache POI corpus and the synthetic decks), to
//! catch an expansion the budget does not price. Whatever the next way
//! around the budget is, it has to stay under `CEILING` here.
//!
//! The accepted decks are parsed under a 48 MB budget (`BUDGET`), so a
//! debug build gets through them in seconds; what they allocate scales
//! with the budget, and the ceiling is a multiple of it. The refused decks
//! run under the real 512 MB budget. Cases run one after another in one
//! test, so each peak is its own.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{rebuild, with_junk};
use lampstand_core::{FontLookup, FontMatch};
use lampstand_slides::{import_pptx, import_pptx_with_budget, ImportError};

struct Counting;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grew(by: usize) {
    let now = CURRENT.fetch_add(by, Ordering::Relaxed) + by;
    PEAK.fetch_max(now, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            grew(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            grew(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        CURRENT.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            if new_size >= layout.size() {
                grew(new_size - layout.size());
            } else {
                CURRENT.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        p
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The parse budget the accepted decks are held to.
const BUDGET: u64 = 48 << 20;

/// The most parsing any deck here may allocate at once, over what was
/// allocated before it started (the deck's own bytes): twice the budget,
/// plus what any parse allocates whatever the deck (the scene of a few
/// slides, DeckCraft's defaults). At the real budget of 512 MB that is
/// about 1 GiB; the largest real deck measured (Apache POI's
/// HowWeRefactor) peaks at 123 MB.
const CEILING: usize = 2 * BUDGET as usize + (32 << 20);

/// Every font is present, so lookups don't allocate a font set.
struct AllFonts;

impl FontLookup for AllFonts {
    fn lookup(&self, _: &str) -> FontMatch {
        FontMatch::Exact
    }
}

/// Peak bytes allocated while parsing `bytes` under `budget` (the real
/// one when `None`), and how it went.
fn measure(bytes: &[u8], budget: Option<u64>) -> (usize, Result<usize, ImportError>) {
    let base = CURRENT.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let result = match budget {
        Some(b) => import_pptx_with_budget(bytes, &AllFonts, b),
        None => import_pptx(bytes, &AllFonts),
    }
    .map(|d| d.slides.len());
    (PEAK.load(Ordering::Relaxed).saturating_sub(base), result)
}

fn too_large(result: &Result<usize, ImportError>) -> bool {
    matches!(result, Err(ImportError::TooLarge { .. }))
}

/// The largest `n` (from `start`, shrinking by a fifth) whose deck the
/// budget accepts, its deck, and the peak of parsing it.
fn at_the_limit(start: usize, make: &dyn Fn(usize) -> Vec<u8>) -> (usize, usize, usize) {
    let mut n = start;
    loop {
        let bytes = make(n);
        let (peak, result) = measure(&bytes, Some(BUDGET));
        if !too_large(&result) {
            assert!(result.is_ok(), "n = {n}: {result:?}");
            // Refused at least once, so `n` is near the budget.
            assert!(n < start, "{start} was not enough to reach the budget");
            return (n, bytes.len(), peak);
        }
        assert!(n > 1, "nothing accepted");
        n = n * 4 / 5;
    }
}

/// Builds a deck with `n` of something.
type MakeDeck = Box<dyn Fn(usize) -> Vec<u8>>;

/// A small xorshift, so the random decks are the same every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    /// A name DeckCraft's parser takes as an element or attribute name:
    /// digits, punctuation and non-ASCII included.
    fn junk_name(&mut self) -> String {
        const PARTS: [&str; 8] = ["1", "é", "x", "-", "_", "ß", "9a", "名"];
        (0..1 + self.below(4))
            .map(|_| PARTS[self.below(PARTS.len())])
            .collect()
    }
}

/// Random nested content: elements from the DrawingML vocabulary (or junk
/// names), with attributes, text, and the containers DeckCraft builds
/// models from (shapes with text bodies, tables).
fn vocabulary_content(rng: &mut Rng, vocabulary: &[&str], units: usize) -> String {
    fn element(rng: &mut Rng, vocabulary: &[&str], depth: usize, out: &mut String) {
        let name = if rng.below(5) == 0 {
            rng.junk_name()
        } else {
            vocabulary[rng.below(vocabulary.len())].to_string()
        };
        out.push('<');
        out.push_str(&name);
        for _ in 0..rng.below(4) {
            let key = match rng.below(4) {
                0 => "val".to_string(),
                1 => "w".to_string(),
                2 => rng.junk_name(),
                _ => "r:id".to_string(),
            };
            out.push_str(&format!(" {key}=\"{}\"", rng.below(100_000)));
        }
        let children = if depth >= 6 { 0 } else { rng.below(4) };
        if children == 0 && rng.below(2) == 0 {
            out.push_str("/>");
            return;
        }
        out.push('>');
        if rng.below(3) == 0 {
            out.push_str("text");
        }
        for _ in 0..children {
            element(rng, vocabulary, depth + 1, out);
        }
        out.push_str(&format!("</{name}>"));
    }
    let mut out = String::new();
    for i in 0..units {
        let mut inner = String::new();
        for _ in 0..3 {
            element(rng, vocabulary, 0, &mut inner);
        }
        out.push_str(&match i % 3 {
            0 => format!(
                r#"<p:sp><p:nvSpPr><p:cNvPr id="{}" name=""/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:p>{inner}</a:p>{inner}</p:txBody></p:sp>"#,
                1000 + i
            ),
            1 => format!(
                r#"<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="{}" name=""/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm/><a:graphic><a:graphicData><a:tbl><a:tblGrid><a:gridCol w="12700"/><a:gridCol w="12700"/></a:tblGrid><a:tr h="1"><a:tc>{inner}</a:tc></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>"#,
                1000 + i
            ),
            _ => format!("<p:grpSp>{inner}</p:grpSp>"),
        });
    }
    out
}

/// `count` copies of `each`, split over deck 01's five slides.
fn junk_deck(each: &str, count: usize) -> Vec<u8> {
    with_junk(
        "01-masters-placeholders",
        &[1, 2, 3, 4, 5],
        &each.repeat(count.div_ceil(5)),
    )
}

/// A text box holding `inner` in its body, on deck 06's first slide.
fn text_box(inner: &str) -> Vec<u8> {
    with_junk(
        "06-fills-lines-shadows",
        &[1],
        &format!(
            r#"<p:sp><p:nvSpPr><p:cNvPr id="999" name=""/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/>{inner}</p:txBody></p:sp>"#
        ),
    )
}

/// Deck 08 with its first table given `cols` grid columns and `rows` more
/// rows of `cells` cells.
fn table(cols: usize, rows: usize, cells: usize) -> Vec<u8> {
    let row = format!(r#"<a:tr h="12700">{}</a:tr>"#, "<a:tc/>".repeat(cells));
    rebuild("08-tables", |n, d| {
        if n == "ppt/slides/slide1.xml" {
            let xml = String::from_utf8(d).unwrap();
            let xml = xml.replacen(
                "<a:tblGrid>",
                &format!("<a:tblGrid>{}", r#"<a:gridCol w="12700"/>"#.repeat(cols)),
                1,
            );
            let xml = xml.replacen(
                r#"<a:tr h="609600">"#,
                &format!(r#"{}<a:tr h="609600">"#, row.repeat(rows)),
                1,
            );
            vec![(n.to_string(), xml.into_bytes())]
        } else {
            vec![(n.to_string(), d)]
        }
    })
}

/// Deck 08 with its first slide (given `cells` more table cells) listed
/// `refs` more times in the slide list.
fn listed_again(refs: usize, cells: usize) -> Vec<u8> {
    let row = format!(r#"<a:tr h="12700">{}</a:tr>"#, "<a:tc/>".repeat(100));
    rebuild("08-tables", |n, d| {
        let text = || String::from_utf8(d.clone()).unwrap();
        match n {
            "ppt/slides/slide1.xml" => vec![(
                n.to_string(),
                text()
                    .replacen(
                        r#"<a:tr h="609600">"#,
                        &format!(r#"{}<a:tr h="609600">"#, row.repeat(cells / 100)),
                        1,
                    )
                    .into_bytes(),
            )],
            "ppt/presentation.xml" => {
                let ids: String = (0..refs)
                    .map(|k| format!(r#"<p:sldId id="{}" r:id="rIdD{k}"/>"#, 400 + k))
                    .collect();
                vec![(
                    n.to_string(),
                    text()
                        .replacen("</p:sldIdLst>", &format!("{ids}</p:sldIdLst>"), 1)
                        .into_bytes(),
                )]
            }
            "ppt/_rels/presentation.xml.rels" => {
                let rels: String = (0..refs)
                    .map(|k| format!(r#"<Relationship Id="rIdD{k}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/>"#))
                    .collect();
                vec![(
                    n.to_string(),
                    text()
                        .replacen("</Relationships>", &format!("{rels}</Relationships>"), 1)
                        .into_bytes(),
                )]
            }
            _ => vec![(n.to_string(), d)],
        }
    })
}

#[test]
fn peak_memory_stays_bounded() {
    let vocabulary: Vec<&str> = include_str!("vocabulary.txt").lines().collect();
    let mut report = Vec::new();
    let mut worst = (0, String::new());
    let mut record = |what: &str, n: usize, len: usize, peak: usize| {
        let line = format!(
            "{what}: n = {n}, {} KB deck, peak {} MB",
            len >> 10,
            peak >> 20
        );
        if peak > worst.0 {
            worst = (peak, line.clone());
        }
        report.push(line);
    };

    // Accepted: each kind of content, as much as the budget lets through.
    let cases: Vec<(&str, usize, MakeDeck)> = vec![
        (
            "junk elements <1/>",
            400_000,
            Box::new(|n| junk_deck("<1/>", n)),
        ),
        (
            "junk names with digits and non-ASCII",
            200_000,
            Box::new(|n| {
                let mut rng = Rng(7);
                let each: String = (0..n.div_ceil(5))
                    .map(|_| {
                        format!(
                            "<{} {}=\"{}\"/>",
                            rng.junk_name(),
                            rng.junk_name(),
                            rng.junk_name()
                        )
                    })
                    .collect();
                junk_deck(&each, 5)
            }),
        ),
        (
            "attributes on one element a slide",
            600_000,
            Box::new(|n| {
                let attrs: Vec<String> = (0..n / 5).map(|i| format!("a{i}=\"\"")).collect();
                junk_deck(&format!("<a:x {}/>", attrs.join(" ")), 5)
            }),
        ),
        (
            "paragraphs",
            8_000,
            Box::new(|n| text_box(&"<a:p/>".repeat(n))),
        ),
        (
            "runs",
            15_000,
            Box::new(|n| {
                text_box(&format!(
                    "<a:p>{}</a:p>",
                    "<a:r><a:t>x</a:t></a:r>".repeat(n)
                ))
            }),
        ),
        (
            "shapes",
            30_000,
            Box::new(|n| with_junk("06-fills-lines-shadows", &[1], &"<p:sp/>".repeat(n))),
        ),
        ("table cells", 120, Box::new(|n| table(97, n, 100))),
        (
            "table padding (500 columns)",
            25,
            Box::new(|n| table(497, n, 0)),
        ),
        (
            "a slide listed many times",
            8,
            Box::new(|n| listed_again(n, 2_000)),
        ),
        (
            "random DrawingML vocabulary",
            1_000,
            Box::new(move |n| {
                let mut rng = Rng(42);
                with_junk(
                    "06-fills-lines-shadows",
                    &[1, 2],
                    &vocabulary_content(&mut rng, &vocabulary, n / 2),
                )
            }),
        ),
    ];
    for (what, start, make) in &cases {
        let t = std::time::Instant::now();
        let (n, len, peak) = at_the_limit(*start, make.as_ref());
        eprintln!("{what}: {:.1} s", t.elapsed().as_secs_f32());
        record(what, n, len, peak);
    }

    // Refused: the fourth review's decks, and the third's.
    let refused: Vec<(&str, Vec<u8>)> = vec![
        (
            "<1/> x 3.9M on each of 5 slides",
            junk_deck("<1/>", 19_500_000),
        ),
        (
            "<é/> x 3.9M on one slide",
            with_junk("06-fills-lines-shadows", &[1], &"<é/>".repeat(3_900_000)),
        ),
        (
            "8M attributes on one element",
            junk_deck(
                &format!(
                    "<a:x {}/>",
                    (0..1_600_000)
                        .map(|i| format!("a{i}=\"\""))
                        .collect::<Vec<_>>()
                        .join(" ")
                ),
                5,
            ),
        ),
        ("500 grid columns, 1000 empty rows", table(497, 1000, 0)),
        (
            "200 references to a 2,000-cell slide",
            listed_again(200, 2_000),
        ),
    ];
    for (what, bytes) in &refused {
        let (peak, result) = measure(bytes, None);
        assert!(too_large(&result), "{what}: {result:?}");
        record(&format!("{what} (refused)"), 0, bytes.len(), peak);
    }

    // A realistic 200-slide service under the real budget: parsed whole.
    let service = common::with_copies("10-church-service", &[(1, 100), (2, 100)]);
    let (peak, result) = measure(&service, None);
    assert_eq!(result.as_ref().ok(), Some(&205), "{result:?}");
    record(
        "200-slide service deck (real budget)",
        205,
        service.len(),
        peak,
    );

    eprintln!("{}", report.join("\n"));
    assert!(
        worst.0 < CEILING,
        "peak over {} MB: {}",
        CEILING >> 20,
        worst.1
    );
}
