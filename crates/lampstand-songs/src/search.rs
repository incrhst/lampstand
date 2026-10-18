//! Full-text search over titles, lyrics and authors, with typo tolerance.
//!
//! PRD: SL-3. See the crate docs for why this is a small in-memory index
//! rather than tantivy.
//!
//! Matching rules, per word of the query:
//! - exact word matches score highest;
//! - the last word also matches as a prefix, so results appear while the
//!   operator is still typing ("amazing gr");
//! - words of 4 to 7 letters tolerate one typo, longer words two, counting
//!   a swapped pair of letters as one typo ("graec" finds "grace");
//! - case, accents and apostrophes are ignored ("Jesu" finds "Jésu",
//!   "twas" finds "'Twas").
//!
//! Songs matching more of the query words rank first, then by a BM25 score
//! in which title words count three times as much as lyric words. Typing a
//! CCLI number finds that song.

use std::collections::HashMap;
use std::hash::Hash;

use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

use lampstand_core::song::Song;

const TITLE: usize = 0;
const LYRICS: usize = 1;
const AUTHORS: usize = 2;
const FIELD_WEIGHTS: [f32; 3] = [3.0, 1.0, 0.5];
const K1: f32 = 1.2;
const B: f32 = 0.75;

#[derive(Debug, Clone, PartialEq)]
pub struct Hit<K> {
    pub key: K,
    pub score: f32,
}

#[derive(Debug, Clone)]
struct Doc<K> {
    key: K,
    lengths: [u32; 3],
    title: String,
    ccli: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
struct Posting {
    doc: u32,
    tf: [u16; 3],
}

/// An in-memory search index over songs, keyed by whatever id the caller's
/// song store uses.
#[derive(Debug, Clone)]
pub struct SongIndex<K> {
    /// `None` marks a removed song; its postings are skipped at query time.
    docs: Vec<Option<Doc<K>>>,
    by_key: HashMap<K, u32>,
    term_ids: HashMap<String, usize>,
    vocab: Vec<String>,
    postings: Vec<Vec<Posting>>,
    live: usize,
    total_lengths: [u64; 3],
}

impl<K> Default for SongIndex<K> {
    fn default() -> Self {
        SongIndex {
            docs: Vec::new(),
            by_key: HashMap::new(),
            term_ids: HashMap::new(),
            vocab: Vec::new(),
            postings: Vec::new(),
            live: 0,
            total_lengths: [0; 3],
        }
    }
}

impl<K: Clone + Eq + Hash> SongIndex<K> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.live
    }

    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// Adds a song, replacing any song already indexed under the same key.
    pub fn insert(&mut self, key: K, song: &Song) {
        self.remove(&key);
        let doc = self.docs.len() as u32;

        let mut titles = song.title.clone();
        for alt in &song.alternate_titles {
            titles.push('\n');
            titles.push_str(alt);
        }
        let fields = [titles, song.lyrics_text(), song.authors.join("\n")];

        let mut counts: HashMap<String, [u16; 3]> = HashMap::new();
        let mut lengths = [0u32; 3];
        for (f, text) in fields.iter().enumerate() {
            for token in tokenize(text) {
                lengths[f] += 1;
                let tf = &mut counts.entry(token).or_default()[f];
                *tf = tf.saturating_add(1);
            }
        }
        for (term, tf) in counts {
            let id = match self.term_ids.get(&term) {
                Some(&id) => id,
                None => {
                    self.vocab.push(term.clone());
                    self.postings.push(Vec::new());
                    self.term_ids.insert(term, self.vocab.len() - 1);
                    self.vocab.len() - 1
                }
            };
            self.postings[id].push(Posting { doc, tf });
        }
        for (total, len) in self.total_lengths.iter_mut().zip(lengths) {
            *total += u64::from(len);
        }

        let title = normalise(&song.title);
        self.docs.push(Some(Doc { key: key.clone(), lengths, title, ccli: song.ccli_number }));
        self.by_key.insert(key, doc);
        self.live += 1;
    }

    /// Removes a song. Returns whether it was there.
    pub fn remove(&mut self, key: &K) -> bool {
        let Some(doc) = self.by_key.remove(key) else { return false };
        if let Some(d) = self.docs[doc as usize].take() {
            for (total, len) in self.total_lengths.iter_mut().zip(d.lengths) {
                *total -= u64::from(len);
            }
            self.live -= 1;
        }
        true
    }

    /// The best matches for what the operator typed, best first.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit<K>> {
        let tokens = tokenize(query);
        if tokens.is_empty() || self.live == 0 {
            return Vec::new();
        }
        let still_typing = !query.ends_with(char::is_whitespace);
        let avg = self.total_lengths.map(|t| (t as f32 / self.live as f32).max(1.0));
        let n = self.live as f32;

        // Per document: summed score and how many query words it matched.
        let mut scores: HashMap<u32, (f32, usize)> = HashMap::new();
        for (i, token) in tokens.iter().enumerate() {
            let prefix = still_typing && i == tokens.len() - 1;
            let mut best: HashMap<u32, f32> = HashMap::new();
            for (term, quality) in self.candidates(token, prefix) {
                let postings = &self.postings[term];
                let df = postings.len() as f32;
                let idf = (1.0 + (n - df + 0.5) / (df + 0.5)).ln().max(0.01);
                for p in postings {
                    let Some(doc) = &self.docs[p.doc as usize] else { continue };
                    let mut tf = 0.0;
                    for f in [TITLE, LYRICS, AUTHORS] {
                        let norm = 1.0 - B + B * doc.lengths[f] as f32 / avg[f];
                        tf += FIELD_WEIGHTS[f] * f32::from(p.tf[f]) / norm;
                    }
                    let score = quality * idf * tf * (K1 + 1.0) / (tf + K1);
                    let entry = best.entry(p.doc).or_insert(0.0);
                    *entry = entry.max(score);
                }
            }
            for (doc, score) in best {
                let entry = scores.entry(doc).or_insert((0.0, 0));
                entry.0 += score;
                entry.1 += 1;
            }
        }

        // Titles that start with the query, and exact CCLI numbers, jump up.
        let query_norm = tokens.join(" ");
        let ccli: Option<u32> = query.trim().parse().ok();
        for (id, doc) in self.docs.iter().enumerate() {
            let Some(doc) = doc else { continue };
            let id = id as u32;
            if ccli.is_some() && doc.ccli == ccli {
                let entry = scores.entry(id).or_insert((0.0, 0));
                *entry = (entry.0 + 100.0, tokens.len());
            } else if doc.title.starts_with(&query_norm) {
                if let Some(entry) = scores.get_mut(&id) {
                    entry.0 += 10.0;
                }
            }
        }

        let mut hits: Vec<(u32, f32, usize)> = scores.into_iter().map(|(d, (s, m))| (d, s, m)).collect();
        hits.sort_by(|a, b| b.2.cmp(&a.2).then(b.1.total_cmp(&a.1)).then(a.0.cmp(&b.0)));
        hits.into_iter()
            .take(limit)
            .filter_map(|(d, score, _)| {
                let doc = self.docs[d as usize].as_ref()?;
                Some(Hit { key: doc.key.clone(), score })
            })
            .collect()
    }

    /// Vocabulary terms that match a query word, with a match quality in
    /// (0, 1]: exact 1.0, prefix 0.8, one typo 0.6, two typos 0.4.
    fn candidates(&self, token: &str, prefix: bool) -> Vec<(usize, f32)> {
        let len = token.chars().count();
        let max_edits = match len {
            0..=3 => 0,
            4..=7 => 1,
            _ => 2,
        };
        let mut out = Vec::new();
        if let Some(&id) = self.term_ids.get(token) {
            out.push((id, 1.0));
        }
        if !prefix && max_edits == 0 {
            return out;
        }
        let token_chars: Vec<char> = token.chars().collect();
        for (id, term) in self.vocab.iter().enumerate() {
            if term == token {
                continue;
            }
            if prefix && len >= 2 && term.starts_with(token) {
                out.push((id, 0.8));
                continue;
            }
            if max_edits > 0 {
                if let Some(d) = edit_distance(&token_chars, term, max_edits) {
                    out.push((id, if d == 1 { 0.6 } else { 0.4 }));
                }
            }
        }
        out
    }
}

impl<K: Clone + Eq + Hash> FromIterator<(K, Song)> for SongIndex<K> {
    fn from_iter<I: IntoIterator<Item = (K, Song)>>(iter: I) -> Self {
        let mut index = SongIndex::new();
        for (key, song) in iter {
            index.insert(key, &song);
        }
        index
    }
}

/// Lower-cases, removes accents and apostrophes, and joins words with
/// single spaces.
fn normalise(text: &str) -> String {
    tokenize(text).join(" ")
}

fn tokenize(text: &str) -> Vec<String> {
    let folded: String = text
        .nfd()
        .filter(|c| !is_combining_mark(*c) && !matches!(c, '\'' | '\u{2019}' | '\u{2018}'))
        .flat_map(char::to_lowercase)
        .collect();
    folded.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

/// Optimal string alignment distance (Levenshtein plus adjacent swaps),
/// or `None` if it is more than `max`.
fn edit_distance(a: &[char], b: &str, max: usize) -> Option<usize> {
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > max {
        return None;
    }
    let width = b.len() + 1;
    let mut prev2 = vec![0usize; width];
    let mut prev: Vec<usize> = (0..width).collect();
    let mut cur = vec![0usize; width];
    for i in 1..=a.len() {
        cur[0] = i;
        let mut row_min = cur[0];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(prev2[j - 2] + 1);
            }
            cur[j] = v;
            row_min = row_min.min(v);
        }
        if row_min > max {
            return None;
        }
        std::mem::swap(&mut prev2, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    let d = prev[b.len()];
    (d <= max).then_some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn edit_distance_counts_swaps_as_one() {
        assert_eq!(edit_distance(&chars("grace"), "grace", 2), Some(0));
        assert_eq!(edit_distance(&chars("graec"), "grace", 2), Some(1));
        assert_eq!(edit_distance(&chars("amzing"), "amazing", 2), Some(1));
        assert_eq!(edit_distance(&chars("cat"), "dog", 2), None);
    }

    #[test]
    fn tokenize_folds_case_accents_and_apostrophes() {
        assert_eq!(tokenize("'Twas Grace, Jésu’s"), ["twas", "grace", "jesus"]);
    }
}
