//! The service order and how each item expands into cues.

use serde::{Deserialize, Serialize};

use lampstand_scripture::{split, Budget, Reference};

use crate::library::Library;
use crate::protocol::{CueView, ItemKind};

/// Lines of a song section shown at once; longer sections continue on the next cue.
pub const SONG_LINES_PER_CUE: usize = 4;
/// Scripture is split to fit this budget.
pub const SCRIPTURE_BUDGET: Budget = Budget { chars_per_line: 40, lines: 6 };

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Service {
    pub name: String,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// Unique within the service; the operator page uses it to keep selection across edits.
    pub id: String,
    #[serde(flatten)]
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    Song { song: String },
    Scripture { reference: String, translation: String },
    Deck { deck: String },
    Logo,
}

/// An item expanded for the show.
#[derive(Debug, Clone, PartialEq)]
pub struct Expanded {
    pub kind: ItemKind,
    pub title: String,
    pub cues: Vec<CueView>,
    pub problem: Option<String>,
}

impl Expanded {
    fn problem(kind: ItemKind, title: String, problem: String) -> Expanded {
        Expanded { kind, title, cues: Vec::new(), problem: Some(problem) }
    }
}

pub fn expand(item: &Item, lib: &Library) -> Expanded {
    match &item.source {
        Source::Song { song } => expand_song(song, lib),
        Source::Scripture { reference, translation } => expand_scripture(reference, translation, lib),
        Source::Deck { deck } => expand_deck(deck, lib),
        Source::Logo => Expanded {
            kind: ItemKind::Logo,
            title: "Logo".into(),
            cues: vec![CueView::Text { lines: Vec::new(), footer: None, label: "Logo".into(), notes: None }],
            problem: None,
        },
    }
}

fn expand_song(id: &str, lib: &Library) -> Expanded {
    let Some(song) = lib.song(id) else {
        return Expanded::problem(ItemKind::Song, "Missing song".into(), format!("song {id} is not in the library"));
    };
    let sections = match song.arranged() {
        Ok(s) => s,
        Err(e) => return Expanded::problem(ItemKind::Song, song.title.clone(), e.to_string()),
    };
    let mut footer = song.title.clone();
    if !song.authors.is_empty() {
        footer.push_str(" · ");
        footer.push_str(&song.authors.join(", "));
    }
    if let Some(c) = &song.copyright {
        footer.push_str(" · ");
        footer.push_str(c);
    }
    if let Some(n) = song.ccli_number {
        footer.push_str(&format!(" · CCLI {n}"));
    }
    let mut cues = Vec::new();
    for section in sections {
        let lines: Vec<String> = section.lyrics().into_iter().filter(|l| !l.trim().is_empty()).collect();
        let chunks: Vec<&[String]> = if lines.is_empty() { vec![&[][..]] } else { lines.chunks(SONG_LINES_PER_CUE).collect() };
        let n = chunks.len();
        for (i, chunk) in chunks.into_iter().enumerate() {
            let label = if n > 1 { format!("{} {}/{n}", section.label, i + 1) } else { section.label.clone() };
            cues.push(CueView::Text { lines: chunk.to_vec(), footer: Some(footer.clone()), label, notes: None });
        }
    }
    Expanded { kind: ItemKind::Song, title: song.title.clone(), cues, problem: None }
}

fn expand_scripture(reference: &str, translation: &str, lib: &Library) -> Expanded {
    let title = format!("{reference} ({translation})");
    let r: Reference = match reference.parse() {
        Ok(r) => r,
        Err(e) => return Expanded::problem(ItemKind::Scripture, title, e.to_string()),
    };
    let Some(bible) = lib.bible(translation) else {
        return Expanded::problem(ItemKind::Scripture, title, format!("no translation called {translation} in the library"));
    };
    let verses = match bible.passage(&r) {
        Ok(v) => v,
        Err(e) => return Expanded::problem(ItemKind::Scripture, title, e.to_string()),
    };
    let canonical = r.to_string();
    let footer = format!("{canonical} · {}", bible.meta.abbreviation);
    let cues = split(&verses, SCRIPTURE_BUDGET)
        .into_iter()
        .map(|slide| {
            let label = match (slide.verses.first(), slide.verses.last()) {
                (Some(a), Some(b)) if (a.chapter, a.verse) != (b.chapter, b.verse) => {
                    if a.chapter == b.chapter {
                        format!("{}:{}\u{2013}{}", a.chapter, a.verse, b.verse)
                    } else {
                        format!("{}:{}\u{2013}{}:{}", a.chapter, a.verse, b.chapter, b.verse)
                    }
                }
                (Some(a), _) => format!("{}:{}", a.chapter, a.verse),
                _ => canonical.clone(),
            };
            CueView::Text { lines: slide.lines(SCRIPTURE_BUDGET.chars_per_line), footer: Some(footer.clone()), label, notes: None }
        })
        .collect();
    Expanded { kind: ItemKind::Scripture, title: format!("{canonical} ({})", bible.meta.abbreviation), cues, problem: None }
}

fn expand_deck(id: &str, lib: &Library) -> Expanded {
    let Some(entry) = lib.deck(id) else {
        return Expanded::problem(ItemKind::Deck, "Missing deck".into(), format!("deck {id} is not in the library"));
    };
    let deck = match &entry.deck {
        Ok(d) => d,
        Err(e) => return Expanded::problem(ItemKind::Deck, entry.name.clone(), e.clone()),
    };
    let info = deck.info();
    let mut cues = Vec::new();
    for (i, s) in info.slides.iter().enumerate().filter(|(_, s)| !s.hidden) {
        for step in 0..=s.clicks {
            let label = if s.clicks > 0 { format!("Slide {} · {}/{}", i + 1, step + 1, s.clicks + 1) } else { format!("Slide {}", i + 1) };
            cues.push(CueView::Image {
                url: format!("/media/deck/{id}/{i}/{step}.png"),
                aspect: info.aspect,
                label,
                notes: s.notes.clone(),
            });
        }
    }
    Expanded { kind: ItemKind::Deck, title: entry.name.clone(), cues, problem: None }
}
