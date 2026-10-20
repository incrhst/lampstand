use std::sync::Arc;

use deckcraft_model::{MediaItem, Shape, ShapeId};
use lampstand_core::FontMatch;

use super::*;

struct AllFonts;

impl FontLookup for AllFonts {
    fn lookup(&self, _: &str) -> FontMatch {
        FontMatch::Exact
    }
}

fn slide_with_ids(ids: &[u32]) -> Slide {
    Slide {
        shapes: ids
            .iter()
            .map(|id| Shape {
                id: ShapeId(*id),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn ids_after_the_last_content_id_wrap_to_the_gaps() {
    // The review's repro: a shape at 2^24 - 1 leaves no room above it.
    let pres = Presentation::default();
    let mut deck = Deck::new(&pres, &AllFonts);
    let slide = slide_with_ids(&[1, 2, COMPOSITOR_NODE_BASE - 1]);
    let mut sc = SlideCx::new(&mut deck, 0, &slide);
    let mut ids: Vec<NodeId> = (0..100).map(|_| sc.fresh_id()).collect();
    assert!(!sc.exhausted);
    // The slide's own IDs stay theirs.
    assert_eq!(
        sc.shape_id(Some(COMPOSITOR_NODE_BASE - 1)),
        COMPOSITOR_NODE_BASE - 1
    );
    assert_eq!(sc.shape_id(Some(2)), 2);
    ids.extend([COMPOSITOR_NODE_BASE - 1, 2]);
    assert!(ids.iter().all(|id| *id > 0 && *id < COMPOSITOR_NODE_BASE));
    let n = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), n, "duplicate IDs");
}

#[test]
fn a_shape_claims_its_id_once() {
    let pres = Presentation::default();
    let mut deck = Deck::new(&pres, &AllFonts);
    let slide = slide_with_ids(&[5]);
    let mut sc = SlideCx::new(&mut deck, 0, &slide);
    assert_eq!(sc.shape_id(Some(5)), 5);
    // A second shape with the same ID (a malformed file) gets a fresh one.
    assert_ne!(sc.shape_id(Some(5)), 5);
    // An ID in the compositor's range is never kept.
    assert!(sc.shape_id(Some(COMPOSITOR_NODE_BASE + 7)) < COMPOSITOR_NODE_BASE);
}

#[test]
fn media_is_moved_not_copied() {
    let bytes = vec![7u8; 1 << 16];
    let at = bytes.as_ptr();
    let mut pres = Presentation::default();
    pres.media.push(MediaItem {
        id: MediaId(42),
        name: "image1.png".into(),
        content_type: "image/png".into(),
        data: Arc::new(bytes),
        link: None,
    });
    let hash = BlobHash::of(&pres.media[0].data);
    let used = BTreeMap::from([(
        hash,
        Used {
            id: MediaId(42),
            content_type: "image/png".into(),
        },
    )]);
    let media = take_media(pres, used);
    assert_eq!(media[&hash].bytes.as_ptr(), at);
}

#[test]
fn slide_sizes_stay_in_the_schema_range() {
    let pres = Presentation {
        slide_size: deckcraft_geom::Size::new(1.0e15, f64::NAN),
        ..Default::default()
    };
    assert_eq!(slide_size(&pres), (4032.0, 540.0));
}

/// The re-review's second repro, in miniature: a master with many shapes
/// shown on many slides costs its elements once but its nodes on every
/// slide. The deck budget stops it.
#[test]
fn a_heavy_master_on_many_slides_stops_at_the_deck_budget() {
    let mut pres = Presentation::default();
    let master = Arc::make_mut(&mut pres.masters[0]);
    master.shapes.extend((0..1000).map(|i| Shape {
        id: ShapeId(10_000 + i),
        xfrm: Some(deckcraft_geom::Xfrm::new(1.0, 1.0, 5.0, 5.0)),
        fill: Some(deckcraft_model::Fill::solid(
            deckcraft_model::ColorRef::rgb(deckcraft_model::Rgba::BLACK),
        )),
        ..Default::default()
    }));
    let layout = master.layouts[0].id;
    let slide = Slide {
        layout,
        ..Default::default()
    };
    let mut deck = Deck::new(&pres, &AllFonts);
    deck.node_limit = 20_000;
    let slides: Vec<DeckSlide> = (0..60).map(|i| deck.slide(i, &slide)).collect();
    let total: usize = slides.iter().map(|s| count(&s.scene.nodes)).sum();
    // Each slide has about 1,000 nodes: the budget is spent by slide 20.
    assert!(total <= 20_000 + 5, "{total} nodes");
    assert!(slides[5].scene.nodes.len() > 1000);
    assert!(slides[59].scene.nodes.is_empty());
    assert!(deck.issues.iter().any(|i| i.slide.is_none()
        && matches!(&i.kind, IssueKind::UnsupportedContent { what } if what.contains("later slides"))));
}

fn count(nodes: &[Node]) -> usize {
    nodes
        .iter()
        .map(|n| {
            1 + match &n.kind {
                NodeKind::Group(kids) => count(kids),
                _ => 0,
            }
        })
        .sum()
}
