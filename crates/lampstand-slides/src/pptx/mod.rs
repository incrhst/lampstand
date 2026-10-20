//! PPTX → `ParsedDeck`, on DeckCraft's reader.
//!
//! `deckcraft_pptx` reads the package into DeckCraft's editor model, which
//! is unresolved: placeholders inherit from their layout and master at
//! draw time. This module resolves every shape with `deckcraft_model::resolve`
//! (the same helpers DeckCraft's own renderer uses) and emits our scene
//! nodes, so nothing from DeckCraft leaves this crate.
//!
//! Node IDs. A slide shape's node ID is its DeckCraft `ShapeId`, which is
//! the PPTX `cNvPr id` wherever that is unique on the slide, so T30's build
//! animations can target it. Nodes the mapping adds (the background, master
//! and layout graphics, a shape's text next to its outline, table cells,
//! arrowheads) are numbered after the largest shape ID on the slide.
//!
//! Group transforms are flattened the way DeckCraft draws them: a group's
//! child-coordinate scale sizes and places its children, but outline
//! widths, insets and font sizes stay in slide points. Each node's local
//! space is the shape's box at its size on the slide.
//!
//! Not resolved here: text measurement. `normAutofit` applies the scale the
//! file was saved with (as PowerPoint does on open), and table rows keep
//! their stored heights.
//!
//! Decks are untrusted (guest speakers send them): numbers from the XML go
//! through `limits` before they size, repeat or allocate anything, a deck
//! that would cost too much to parse is refused (`budget`), and a slide
//! stops at `MAX_NODES` nodes.

mod budget;
mod geometry;
mod limits;
mod paint;
mod shape;
mod table;
mod text;

use std::collections::{BTreeMap, BTreeSet};

use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::{MediaId, Presentation, Slide};
use lampstand_core::geom::{Rect, Transform};
use lampstand_core::id::BlobHash;
use lampstand_core::scene::{Node, NodeId, NodeKind, Scene, Transition, COMPOSITOR_NODE_BASE};
use lampstand_core::{FontLookup, FontMatch};

use crate::{DeckSlide, FidelityReport, ImportError, Issue, IssueKind, MediaPart, ParsedDeck};

#[cfg(test)]
mod tests;

/// Parses a PPTX file. See `crate::import_pptx`.
pub(crate) fn import(bytes: &[u8], fonts: &dyn FontLookup) -> Result<ParsedDeck, ImportError> {
    import_with(bytes, fonts, budget::LIMITS.memory)
}

/// `import` with the import budget limited to `memory` bytes.
pub(crate) fn import_with(
    bytes: &[u8],
    fonts: &dyn FontLookup,
    memory: u64,
) -> Result<ParsedDeck, ImportError> {
    if deckcraft_pptx::is_encrypted(bytes) {
        return Err(ImportError::Encrypted);
    }
    if !deckcraft_pptx::sniff(bytes) {
        return Err(ImportError::NotAPresentation);
    }
    let limits = budget::Limits { memory };
    budget::check_with(bytes, &limits)?;
    let (pres, skipped) = deckcraft_pptx::import_checked(bytes).map_err(|e| match e {
        deckcraft_pptx::PptxError::NotPptx(_) => ImportError::NotAPresentation,
        other => ImportError::Corrupt(other.to_string()),
    })?;
    let size = slide_size(&pres);
    let (slides, issues, used) = {
        let mut deck = Deck::new(&pres, fonts);
        for part in skipped {
            deck.issue(
                None,
                IssueKind::UnsupportedContent {
                    what: format!("slide part {part} could not be read"),
                },
            );
        }
        let mut slides = Vec::with_capacity(pres.slides.len());
        for (index, slide) in pres.slides.iter().enumerate() {
            slides.push(deck.slide(index, slide));
        }
        (slides, deck.issues, deck.used)
    };
    Ok(ParsedDeck {
        width: size.0,
        height: size.1,
        slides,
        media: take_media(pres, used),
        report: FidelityReport { issues },
    })
}

/// The media the slides use, moved out of the presentation rather than
/// copied (a deck's video can be most of its size).
fn take_media(
    mut pres: Presentation,
    used: BTreeMap<BlobHash, Used>,
) -> BTreeMap<BlobHash, MediaPart> {
    let mut out = BTreeMap::new();
    for (hash, used) in used {
        let Some(item) = pres.media.iter_mut().find(|m| m.id == used.id) else {
            continue;
        };
        let data = std::mem::take(&mut item.data);
        let bytes =
            std::sync::Arc::try_unwrap(data).unwrap_or_else(|shared| shared.as_ref().clone());
        out.insert(
            hash,
            MediaPart {
                content_type: used.content_type,
                bytes,
            },
        );
    }
    out
}

/// A media item a slide uses: which one, and its content type.
struct Used {
    id: MediaId,
    content_type: String,
}

/// The slide size in points. EMU sizes rarely land on whole points
/// (13.333 in is 959.98 pt), so sizes within a hundredth of a point of a
/// whole number are rounded to it.
fn slide_size(pres: &Presentation) -> (f32, f32) {
    let side = |v: f64, fallback: f32| {
        if !v.is_finite() || v <= 0.0 {
            return fallback;
        }
        let r = v.round();
        let v = if (v - r).abs() < 0.05 { r } else { v };
        let (lo, hi) = limits::SLIDE_SIDE;
        limits::clamp(v, lo, hi, fallback)
    };
    (
        side(pres.slide_size.width, 960.0),
        side(pres.slide_size.height, 540.0),
    )
}

/// Deck-wide state while mapping: media found so far, the report, and the
/// fonts already checked.
pub(crate) struct Deck<'a> {
    pres: &'a Presentation,
    fonts: &'a dyn FontLookup,
    used: BTreeMap<BlobHash, Used>,
    by_id: BTreeMap<MediaId, BlobHash>,
    issues: Vec<Issue>,
    fonts_checked: BTreeSet<String>,
    reported: BTreeSet<(Option<usize>, String)>,
    /// Nodes the slides mapped so far have.
    nodes: usize,
    /// The most nodes the whole deck may have (`MAX_DECK_NODES`).
    node_limit: usize,
}

impl<'a> Deck<'a> {
    fn new(pres: &'a Presentation, fonts: &'a dyn FontLookup) -> Self {
        Self {
            pres,
            fonts,
            used: BTreeMap::new(),
            by_id: BTreeMap::new(),
            issues: Vec::new(),
            fonts_checked: BTreeSet::new(),
            reported: BTreeSet::new(),
            nodes: 0,
            node_limit: MAX_DECK_NODES,
        }
    }

    fn slide(&mut self, index: usize, slide: &Slide) -> DeckSlide {
        let size = slide_size(self.pres);
        let ctx = if self.nodes >= self.node_limit {
            // The deck has used its node budget: the rest come back blank.
            self.issue(
                None,
                IssueKind::UnsupportedContent {
                    what: format!(
                        "a deck of more than {} drawn parts; later slides are left blank",
                        self.node_limit
                    ),
                },
            );
            None
        } else {
            Ctx::for_slide(self.pres, slide)
        };
        let nodes = match ctx {
            Some(ctx) => {
                let mut sc = SlideCx::new(self, index, slide);
                sc.background(&ctx, slide, size);
                sc.backdrop(&ctx, slide);
                for s in &slide.shapes {
                    sc.shape(&ctx, s, Transform::IDENTITY, 0);
                }
                if sc.truncated {
                    sc.issue(IssueKind::UnsupportedContent {
                        what: format!(
                            "a slide past the drawn-part budget ({MAX_NODES} a slide, {} a deck); \
                             the rest are left out",
                            sc.deck.node_limit
                        ),
                    });
                }
                sc.deck.nodes += sc.issued.len() + sc.text_parts;
                if sc.exhausted {
                    sc.issue(IssueKind::UnsupportedContent {
                        what: "a slide with more shapes than node IDs; left blank".into(),
                    });
                    Vec::new()
                } else {
                    sc.nodes
                }
            }
            None => Vec::new(),
        };
        DeckSlide {
            scene: Scene {
                width: size.0,
                height: size.1,
                nodes,
            },
            // T30 maps `p:timing` and `p:transition`.
            builds: Vec::new(),
            transition: Transition::CUT,
            notes: notes_text(slide),
            hidden: slide.hidden,
        }
    }

    /// Records an issue once per slide and description.
    pub(crate) fn issue(&mut self, slide: Option<usize>, kind: IssueKind) {
        if self.reported.insert((slide, format!("{kind:?}"))) {
            self.issues.push(Issue { slide, kind });
        }
    }

    /// Checks a font family against the font set, once per deck: the
    /// first slide that uses a family it lacks gets the issue.
    pub(crate) fn check_font(&mut self, slide: usize, family: &str) {
        if family.is_empty() || !self.fonts_checked.insert(family.to_string()) {
            return;
        }
        let substitute = match self.fonts.lookup(family) {
            FontMatch::Exact => return,
            FontMatch::Substitute(s) => Some(s),
            FontMatch::Missing => None,
        };
        self.issue(
            Some(slide),
            IssueKind::MissingFont {
                requested: family.to_string(),
                substitute,
            },
        );
    }

    /// The blob for an embedded media item, extracting it on first use.
    /// `None` for a missing item, or a linked one (reported).
    pub(crate) fn blob(&mut self, slide: usize, id: MediaId) -> Option<BlobHash> {
        if let Some(h) = self.by_id.get(&id) {
            return Some(*h);
        }
        let item = self.pres.media(id)?;
        if let Some(link) = &item.link {
            self.issue(
                Some(slide),
                IssueKind::MissingLinkedMedia {
                    target: link.clone(),
                },
            );
            return None;
        }
        if item.data.is_empty() {
            return None;
        }
        let hash = BlobHash::of(&item.data);
        self.used.entry(hash).or_insert_with(|| Used {
            id,
            content_type: content_type(&item.content_type, &item.name),
        });
        self.by_id.insert(id, hash);
        Some(hash)
    }
}

/// A media item's content type, from its file name when the package left
/// it generic.
fn content_type(given: &str, name: &str) -> String {
    if !given.is_empty() && given != "application/octet-stream" {
        return given.to_string();
    }
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "wmv" => "video/x-ms-wmv",
        "avi" => "video/x-msvideo",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        _ => given,
    }
    .to_string()
}

/// Speaker notes as plain text, one line per paragraph.
fn notes_text(slide: &Slide) -> String {
    slide
        .notes
        .paragraphs
        .iter()
        .map(|p| p.text().replace('\u{b}', "\n"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

/// The most nodes one slide's scene may have. Real slides have hundreds;
/// a hostile one (a 1000 × 400 table) could have millions, more than a
/// renderer can draw.
pub(crate) const MAX_NODES: usize = 100_000;

/// The most nodes the whole deck may have. Master and layout graphics are
/// mapped onto every slide that shows them (scenes can't share nodes), so
/// a heavy master costs elements once but nodes on every slide; this bounds
/// that. A real 200-slide deck has about 100,000; a node costs a few
/// hundred bytes.
pub(crate) const MAX_DECK_NODES: usize = 1_000_000;

/// Per-slide state: the nodes so far and the next free node ID.
pub(crate) struct SlideCx<'d, 'a> {
    pub(crate) deck: &'d mut Deck<'a>,
    pub(crate) index: usize,
    pub(crate) nodes: Vec<Node>,
    next_id: NodeId,
    /// `next_id` has run up to `COMPOSITOR_NODE_BASE` and started again
    /// from 1, looking for gaps.
    wrapped: bool,
    /// Every content ID is in use: the slide cannot be mapped.
    exhausted: bool,
    /// The slide reached `MAX_NODES`; later shapes were left out.
    truncated: bool,
    /// Text paragraphs and runs mapped, which count against the node
    /// budgets like nodes (a master's text block is drawn on every slide).
    pub(crate) text_parts: usize,
    /// IDs handed out so far.
    issued: BTreeSet<NodeId>,
    /// The slide's background, for shapes filled with it (`useBgFill`).
    pub(crate) background: Option<lampstand_core::scene::Paint>,
    /// IDs the slide's own shapes use and have not claimed yet.
    taken: BTreeSet<NodeId>,
    /// Mapping master or layout graphics: no shape keeps its own ID.
    backdrop: bool,
    /// The fill of the group being mapped, for children filled with it
    /// (`grpFill`).
    pub(crate) group_fill: Option<deckcraft_model::Fill>,
}

impl<'d, 'a> SlideCx<'d, 'a> {
    fn new(deck: &'d mut Deck<'a>, index: usize, slide: &Slide) -> Self {
        let mut taken = BTreeSet::new();
        deckcraft_model::walk(&slide.shapes, &mut |s, _| {
            if (1..COMPOSITOR_NODE_BASE).contains(&s.id.0) {
                taken.insert(s.id.0);
            }
        });
        let next_id = taken.iter().next_back().map_or(1, |m| m + 1);
        Self {
            deck,
            index,
            nodes: Vec::new(),
            next_id,
            wrapped: false,
            exhausted: false,
            truncated: false,
            text_parts: 0,
            issued: BTreeSet::new(),
            background: None,
            taken,
            backdrop: false,
            group_fill: None,
        }
    }

    /// A fresh node ID for a node the mapping adds: after the slide's
    /// largest shape ID, then any gap below it. IDs are never reused and
    /// never reach `COMPOSITOR_NODE_BASE`. If every content ID is in use
    /// the slide is marked exhausted (and dropped by `Deck::slide`).
    pub(crate) fn fresh_id(&mut self) -> NodeId {
        loop {
            if self.next_id >= COMPOSITOR_NODE_BASE {
                if self.wrapped {
                    self.exhausted = true;
                    return 0;
                }
                self.wrapped = true;
                self.next_id = 1;
            }
            let id = self.next_id;
            self.next_id += 1;
            if !self.taken.contains(&id) && self.issued.insert(id) {
                return id;
            }
        }
    }

    /// Whether the slide (or the deck) has as many nodes as it may have.
    /// Once it does,
    /// mapping stops adding shapes (and table cells) and the slide is
    /// reported.
    pub(crate) fn full(&mut self) -> bool {
        let here = self.issued.len() + self.text_parts;
        if here >= MAX_NODES || self.deck.nodes + here >= self.deck.node_limit {
            self.truncated = true;
        }
        self.truncated
    }

    /// The node ID for a slide shape: its own ID where it is free.
    pub(crate) fn shape_id(&mut self, own: Option<u32>) -> NodeId {
        match own {
            Some(id) if !self.backdrop && self.taken.remove(&id) => {
                self.issued.insert(id);
                id
            }
            _ => self.fresh_id(),
        }
    }

    pub(crate) fn issue(&mut self, kind: IssueKind) {
        self.deck.issue(Some(self.index), kind);
    }

    fn background(&mut self, ctx: &Ctx, slide: &Slide, size: (f32, f32)) {
        let (fill, ph) = resolve::background(ctx, Some(slide));
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: size.0,
            height: size.1,
        };
        // PowerPoint lays every background over white, which shows through
        // a see-through colour or around a picture that does not cover it.
        let white = self.fresh_id();
        self.nodes.push(Node {
            id: white,
            transform: Transform::IDENTITY,
            opacity: 1.0,
            visible: true,
            kind: NodeKind::Path(lampstand_core::scene::PathNode {
                commands: geometry::rect_path(rect.width, rect.height),
                fill: Some(lampstand_core::scene::Paint::Solid(
                    lampstand_core::geom::Color::WHITE,
                )),
                stroke: None,
                shadow: None,
            }),
        });
        let kind = match &fill {
            deckcraft_model::Fill::Picture(pf) => {
                let local = shape::Local {
                    w: rect.width,
                    h: rect.height,
                    transform: Transform::IDENTITY,
                };
                self.image(pf, &local)
            }
            f => paint::paint(ctx, f, ph, rect.width, rect.height).map(|p| {
                NodeKind::Path(lampstand_core::scene::PathNode {
                    commands: geometry::rect_path(rect.width, rect.height),
                    fill: Some(p),
                    stroke: None,
                    shadow: None,
                })
            }),
        };
        if let Some(kind) = kind {
            let id = self.fresh_id();
            self.nodes.push(shape::node(id, Transform::IDENTITY, kind));
        }
        self.background = paint::paint(ctx, &fill, ph, rect.width, rect.height);
    }

    /// Master and layout graphics (their non-placeholder shapes) behind the
    /// slide's own, where the slide shows them.
    fn backdrop(&mut self, ctx: &Ctx, slide: &Slide) {
        let (show_layout, show_master) = resolve::show_master_shapes(slide, ctx.layout);
        if show_master {
            let mctx = Ctx::for_master(self.deck.pres, ctx.master);
            for s in ctx.master.shapes.iter().filter(|s| s.ph.is_none()) {
                self.backdrop_shape(&mctx, s);
            }
        }
        if let (true, Some(layout)) = (show_layout, ctx.layout) {
            let lctx = Ctx::for_layout(self.deck.pres, ctx.master, layout);
            for s in layout.shapes.iter().filter(|s| s.ph.is_none()) {
                self.backdrop_shape(&lctx, s);
            }
        }
    }

    /// A master or layout shape: its ID may clash with the slide's shapes,
    /// so it (and everything in it) gets fresh IDs.
    fn backdrop_shape(&mut self, ctx: &Ctx, s: &deckcraft_model::Shape) {
        self.backdrop = true;
        self.shape(ctx, s, Transform::IDENTITY, 0);
        self.backdrop = false;
    }
}
