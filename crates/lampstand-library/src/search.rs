//! Song search with typo tolerance (SL-3).
//!
//! An in-memory index rebuilt at startup and updated on change. Libraries
//! run to a few thousand songs, so this stays well under 10 ms a query
//! without an on-disk engine.
//!
//! Ranking: title matches above lyric matches; whole words above prefixes
//! above typo matches (edit distance 1 for words of 4 to 7 letters, 2 for
//! longer). Case, accents and punctuation are ignored ("Ō" matches "o",
//! "it's" matches "its").

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Bound;

use lampstand_core::song::{Song, SongId};
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub song: SongId,
    pub title: String,
    /// The matching lyric line, if the match was in the lyrics.
    pub line: Option<String>,
    pub score: f32,
}

/// Score of a whole-word match; prefixes and typos scale it down.
const EXACT: f32 = 1.0;
const PREFIX: f32 = 0.7;
const TYPO: f32 = 0.4;
/// A title match counts double a lyric match.
const TITLE_WEIGHT: f32 = 2.0;

/// Where a word occurs in a song.
#[derive(Debug, Clone, Copy, Default)]
struct Presence {
    title: bool,
    lyrics: bool,
}

struct Doc {
    id: SongId,
    title: String,
    /// Chord-stripped lyric lines with their normalised words.
    lines: Vec<(String, Vec<String>)>,
    /// Every distinct normalised word, for removal.
    words: HashSet<String>,
}

/// Songs live in a slab so postings and scoring use small dense indexes.
#[derive(Default)]
pub struct SongIndex {
    slots: HashMap<SongId, u32>,
    docs: Vec<Option<Doc>>,
    free: Vec<u32>,
    /// Word → the songs containing it. Ordered so prefixes are a range scan.
    postings: BTreeMap<String, Vec<(u32, Presence)>>,
}

impl SongIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces a song.
    pub fn upsert(&mut self, song: &Song) {
        self.remove(song.id);

        let mut presence: HashMap<String, Presence> = HashMap::new();
        for w in words(&song.title) {
            presence.entry(w).or_default().title = true;
        }
        let mut lines = Vec::new();
        for line in song.sections.iter().flat_map(|s| &s.lines) {
            let text = strip_chords(line);
            let ws = words(&text);
            if ws.is_empty() {
                continue;
            }
            for w in &ws {
                presence.entry(w.clone()).or_default().lyrics = true;
            }
            lines.push((text, ws));
        }

        let doc = Doc {
            id: song.id,
            title: song.title.clone(),
            lines,
            words: presence.keys().cloned().collect(),
        };
        let slot = match self.free.pop() {
            Some(slot) => {
                self.docs[slot as usize] = Some(doc);
                slot
            }
            None => {
                self.docs.push(Some(doc));
                (self.docs.len() - 1) as u32
            }
        };
        self.slots.insert(song.id, slot);
        for (w, p) in presence {
            self.postings.entry(w).or_default().push((slot, p));
        }
    }

    pub fn remove(&mut self, id: SongId) {
        let Some(slot) = self.slots.remove(&id) else {
            return;
        };
        let Some(doc) = self.docs[slot as usize].take() else {
            return;
        };
        for w in doc.words {
            if let Some(list) = self.postings.get_mut(&w) {
                list.retain(|(s, _)| *s != slot);
                if list.is_empty() {
                    self.postings.remove(&w);
                }
            }
        }
        self.free.push(slot);
    }

    /// Best matches first. Every word in the query must match.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        // Chords are never indexed, so they can't be searched for either.
        let query_words = words(&strip_chords(query));
        if query_words.is_empty() || limit == 0 {
            return Vec::new();
        }

        let n = self.docs.len();
        // Running total per slot; 0.0 means no match (every match scores > 0).
        let mut total = vec![0.0_f32; n];
        let mut all_title = vec![true; n];
        // Per query word: the vocabulary words it matched.
        let mut matched: Vec<HashSet<&str>> = Vec::with_capacity(query_words.len());
        for (k, q) in query_words.iter().enumerate() {
            let candidates = self.candidates(q);
            let mut best = vec![0.0_f32; n];
            let mut in_title = vec![false; n];
            for &(word, tier) in &candidates {
                for &(slot, p) in &self.postings[word] {
                    let s = slot as usize;
                    best[s] = best[s].max(tier * if p.title { TITLE_WEIGHT } else { 1.0 });
                    in_title[s] |= p.title;
                }
            }
            matched.push(candidates.iter().map(|&(w, _)| w).collect());
            let mut any = false;
            for s in 0..n {
                if best[s] == 0.0 || (k > 0 && total[s] == 0.0) {
                    total[s] = 0.0;
                    continue;
                }
                total[s] += best[s];
                all_title[s] &= in_title[s];
                any = true;
            }
            if !any {
                return Vec::new();
            }
        }

        let mut ranked: Vec<(f32, &Doc, bool)> = (0..n)
            .filter(|&s| total[s] > 0.0)
            .filter_map(|s| Some((total[s], self.docs[s].as_ref()?, all_title[s])))
            .collect();
        let order = |a: &(f32, &Doc, bool), b: &(f32, &Doc, bool)| {
            b.0.total_cmp(&a.0)
                .then_with(|| a.1.title.cmp(&b.1.title))
                .then_with(|| a.1.id.cmp(&b.1.id))
        };
        if ranked.len() > limit {
            ranked.select_nth_unstable_by(limit - 1, order);
            ranked.truncate(limit);
        }
        ranked.sort_by(order);
        ranked
            .into_iter()
            .map(|(score, doc, title_only)| Hit {
                song: doc.id,
                title: doc.title.clone(),
                line: if title_only {
                    None
                } else {
                    best_line(doc, &matched)
                },
                score,
            })
            .collect()
    }

    /// Vocabulary words matching `q`, with their tier. Typos are tried only
    /// when nothing matches as a whole word or a prefix.
    fn candidates(&self, q: &str) -> Vec<(&str, f32)> {
        let mut out: Vec<(&str, f32)> = self
            .postings
            .range::<str, _>((Bound::Included(q), Bound::Unbounded))
            .take_while(|(w, _)| w.starts_with(q))
            .map(|(w, _)| (w.as_str(), if w == q { EXACT } else { PREFIX }))
            .collect();
        if !out.is_empty() {
            return out;
        }
        let q_chars: Vec<char> = q.chars().collect();
        let max = max_edits(q_chars.len());
        if max == 0 {
            return out;
        }
        let mut buf: Vec<char> = Vec::new();
        for w in self.postings.keys() {
            // Bytes are at least chars, so this only rejects hopeless words.
            if w.len() + max < q_chars.len() || w.len() > q.len() * 4 + max {
                continue;
            }
            buf.clear();
            buf.extend(w.chars());
            if within_edits(&q_chars, &buf, max) {
                out.push((w.as_str(), TYPO));
            }
        }
        out
    }
}

/// The lyric line matching the most query words, if any lyric line matches.
fn best_line(doc: &Doc, matched: &[HashSet<&str>]) -> Option<String> {
    let mut best: Option<(usize, &str)> = None;
    for (text, ws) in &doc.lines {
        let hits = matched
            .iter()
            .filter(|set| ws.iter().any(|w| set.contains(w.as_str())))
            .count();
        if hits > 0 && best.is_none_or(|(n, _)| hits > n) {
            best = Some((hits, text));
        }
    }
    best.map(|(_, t)| t.to_string())
}

/// Edit distance allowed for a query word of `len` letters.
fn max_edits(len: usize) -> usize {
    match len {
        0..=3 => 0,
        4..=7 => 1,
        _ => 2,
    }
}

/// Removes ChordPro `[chord]` markers.
fn strip_chords(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        match rest[open..].find(']') {
            Some(close) => {
                out.push_str(&rest[..open]);
                rest = &rest[open + close + 1..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// Lowercase words with accents removed (NFKD, combining marks dropped).
/// Apostrophes join ("it's" → "its"); other punctuation separates.
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.nfkd() {
        if is_combining_mark(c) || matches!(c, '\'' | '\u{2019}' | '\u{2018}' | '`') {
            continue;
        }
        if c.is_alphanumeric() {
            cur.extend(c.to_lowercase());
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Optimal-string-alignment distance (insert, delete, substitute, swap of
/// neighbours) within `max`, abandoning early once it cannot be.
fn within_edits(a: &[char], b: &[char], max: usize) -> bool {
    if a.len().abs_diff(b.len()) > max {
        return false;
    }
    // Words this long are never typo-matched; it keeps the rows on the stack.
    const CAP: usize = 64;
    if a.len() >= CAP || b.len() >= CAP {
        return false;
    }
    let mut prev2 = [0_usize; CAP];
    let mut prev = [0_usize; CAP];
    let mut cur = [0_usize; CAP];
    for (j, p) in prev.iter_mut().enumerate().take(b.len() + 1) {
        *p = j;
    }
    for i in 1..=a.len() {
        cur[0] = i;
        let mut row_min = cur[0];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut d = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d = d.min(prev2[j - 2] + 1);
            }
            cur[j] = d;
            row_min = row_min.min(d);
        }
        if row_min > max {
            return false;
        }
        std::mem::swap(&mut prev2, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()] <= max
}

#[cfg(test)]
mod tests {
    use super::*;
    use lampstand_core::song::Section;

    fn song(title: &str, lines: &[&str]) -> Song {
        Song {
            sections: vec![Section {
                label: "V1".into(),
                lines: lines.iter().map(|l| l.to_string()).collect(),
            }],
            ..Song::new(title)
        }
    }

    fn index() -> SongIndex {
        let mut i = SongIndex::new();
        i.upsert(&song(
            "Amazing Grace",
            &[
                "[G]Amazing grace how sweet the sound",
                "That saved a wretch like me",
            ],
        ));
        i.upsert(&song(
            "How Great Thou Art",
            &["O Lord my God", "When I in awesome wonder"],
        ));
        i.upsert(&song(
            "Great Is Thy Faithfulness",
            &["Great is Thy faithfulness, O God my Father"],
        ));
        i.upsert(&song("It's Your Blood", &["Washed in the blood"]));
        i
    }

    fn titles(hits: Vec<Hit>) -> Vec<String> {
        hits.into_iter().map(|h| h.title).collect()
    }

    #[test]
    fn finds_by_title_prefix_and_typo() {
        let i = index();
        assert_eq!(titles(i.search("amaz", 5)), ["Amazing Grace"]);
        assert_eq!(titles(i.search("amazng grase", 5)), ["Amazing Grace"]);
        assert_eq!(titles(i.search("its your", 5)), ["It's Your Blood"]);
    }

    #[test]
    fn title_matches_rank_above_lyrics() {
        let i = index();
        let mut great = titles(i.search("great", 5));
        great.sort();
        assert_eq!(great, ["Great Is Thy Faithfulness", "How Great Thou Art"]);
        let hits = i.search("wretch", 5);
        assert_eq!(hits[0].title, "Amazing Grace");
        assert_eq!(hits[0].line.as_deref(), Some("That saved a wretch like me"));
    }

    #[test]
    fn chords_are_not_indexed_and_removal_works() {
        let mut i = index();
        assert!(i.search("[G]", 5).is_empty());
        let id = i.search("amazing", 1)[0].song;
        i.remove(id);
        assert!(i.search("amazing", 5).is_empty());
    }

    #[test]
    fn accents_case_and_punctuation_are_ignored() {
        let mut i = SongIndex::new();
        i.upsert(&song("Ōtautahi", &["Café, naïve — “quoted” text!"]));
        assert_eq!(titles(i.search("OTAUTAHI", 5)), ["Ōtautahi"]);
        assert_eq!(titles(i.search("cafe naive", 5)), ["Ōtautahi"]);
        assert_eq!(titles(i.search("quoted", 5)), ["Ōtautahi"]);
    }

    #[test]
    fn every_query_word_must_match() {
        let i = index();
        assert!(i.search("amazing sound blood", 5).is_empty());
        assert_eq!(titles(i.search("amazing sound", 5)), ["Amazing Grace"]);
        assert!(i.search("", 5).is_empty());
        assert!(i.search("amazing", 0).is_empty());
    }

    #[test]
    fn whole_words_rank_above_prefixes_above_typos() {
        let mut i = SongIndex::new();
        i.upsert(&song("Gracefully", &[]));
        i.upsert(&song("Grace", &[]));
        i.upsert(&song("Graze", &[]));
        assert_eq!(titles(i.search("grace", 5)), ["Grace", "Gracefully"]);
        assert_eq!(titles(i.search("grase", 5)), ["Grace", "Graze"]);
    }

    #[test]
    fn transposed_letters_and_long_word_typos() {
        let i = index();
        assert_eq!(titles(i.search("amazign", 5)), ["Amazing Grace"]);
        // Two edits are allowed for words longer than seven letters.
        assert_eq!(
            titles(i.search("faithfulnses", 5)),
            ["Great Is Thy Faithfulness"]
        );
        // Short words get none.
        assert!(i.search("gra", 5).len() == 1 && i.search("grx", 5).is_empty());
    }

    #[test]
    fn upsert_replaces_and_limit_applies() {
        let mut i = index();
        let mut s = song("Amazing Grace", &["New words entirely"]);
        s.id = i.search("amazing", 1)[0].song;
        i.upsert(&s);
        assert!(i.search("wretch", 5).is_empty());
        assert_eq!(titles(i.search("entirely", 5)), ["Amazing Grace"]);
        assert_eq!(i.search("o", 2).len(), 2);
    }

    /// SL-3: a query over 5,000 songs takes under 10 ms (release builds).
    #[test]
    fn five_thousand_songs_query_under_10ms() {
        use std::time::Instant;

        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = move |n: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % n as u64) as usize
        };
        const SYL: [&str; 24] = [
            "ba", "re", "mi", "so", "lu", "ke", "no", "da", "pi", "ta", "vo", "ne", "gri", "sha",
            "lo", "fen", "mar", "tor", "bel", "an", "ith", "ur", "jo", "ce",
        ];
        let word = |next: &mut dyn FnMut(usize) -> usize| {
            (0..1 + next(3))
                .map(|_| SYL[next(SYL.len())])
                .collect::<String>()
        };

        let mut i = SongIndex::new();
        for n in 0..5000 {
            let title = format!("{} {} {n}", word(&mut next), word(&mut next));
            let lines: Vec<String> = (0..16)
                .map(|_| {
                    (0..7)
                        .map(|_| word(&mut next))
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect();
            let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
            i.upsert(&song(&title, &refs));
        }

        // Debug builds are an order of magnitude slower; only release is held to 10 ms.
        let limit = if cfg!(debug_assertions) { 5000.0 } else { 10.0 };
        let queries = [
            "bare",
            "bareluk",
            "mirenoda gri",
            "zzzzqq",
            "ithurjo mar lo",
            "b",
        ];
        for q in queries {
            // Best of five, so a busy CI machine doesn't fail the build.
            let mut hits = 0;
            let ms = (0..5)
                .map(|_| {
                    let start = Instant::now();
                    hits = i.search(q, 20).len();
                    start.elapsed().as_secs_f64() * 1000.0
                })
                .fold(f64::MAX, f64::min);
            eprintln!("{q:?}: {hits} hits in {ms:.2} ms");
            assert!(ms < limit, "{q:?} took {ms:.2} ms");
        }
    }
}
