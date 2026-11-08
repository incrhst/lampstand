//! Matches a plan's songs and passages against the church's own library,
//! and builds the service from the person's choices (T106, T108; the
//! contract's "Matching rules"). Nothing here knows about Notion: any plan
//! source, and decision 0003's "new service from a list", goes through it.
//!
//! [`resolve`] decides, per entry, what Lampstand would put there and how
//! sure it is. It writes nothing. The person reviews the result, and
//! [`build_service`] turns their choices into a `Service`.
//!
//! Songs, the first rule that gives exactly one song wins:
//!
//! 1. the CCLI number;
//! 2. a choice the person made for this song before (remembered on the
//!    device, keyed by [`song_key`]);
//! 3. the title, normalised (case, accents, punctuation and a leading
//!    "the" ignored), with the author breaking a tie;
//! 4. **likely**, which the person confirms: the title with any bracketed
//!    part dropped on both sides; else one library title whose words are all
//!    in the planned title or hold all of its words ("I Stand in Awe" for
//!    "I Stand in Awe of You"); else the best search hit, which tolerates
//!    typos, by a clear margin.
//!
//! Several songs is **choose one**, none is **not found**. Readings go
//! through `parse_reference`; an ambiguous book is **choose one**.

use std::collections::HashMap;

use lampstand_core::plan::{
    Choice, EntryType, MatchedBy, Outcome, PassageResult, Plan, PlanEntry, ResolvedEntry,
    ResolvedPlan, SongCandidate, SongResult,
};
use lampstand_core::scripture::{
    find_translation, parse_reference, Passage, ReferenceError, Translation, TranslationId,
};
use lampstand_core::service::{ItemKind, Position, Service, ServiceItem};
use lampstand_core::song::{Song, SongId};

use crate::search::{perfect_score, words, SongIndex};

/// A search hit counts as likely only at this fraction of a perfect title
/// match or above, and only this far ahead of the runner-up. Set from the
/// near-miss fixtures in this module's tests (see ARCHITECTURE.md, T106);
/// T112 re-checks them on the church's own library.
pub const LIKELY_AT: f32 = 0.8;
pub const LIKELY_MARGIN: f32 = 0.15;
/// How many songs a "choose one" offers.
const CANDIDATES: usize = 5;

/// What the matcher may look at.
pub struct Catalog {
    songs: Vec<Song>,
    index: SongIndex,
}

impl Catalog {
    pub fn new(songs: Vec<Song>) -> Self {
        let mut index = SongIndex::new();
        for song in &songs {
            index.upsert(song);
        }
        Self { songs, index }
    }

    pub fn song(&self, id: SongId) -> Option<&Song> {
        self.songs.iter().find(|s| s.id == id)
    }
}

/// Everything the matcher needs besides the songs.
#[derive(Debug, Clone, Default)]
pub struct Context {
    pub translations: Vec<Translation>,
    /// The device's choice when an entry names none or one not installed.
    /// `None` picks the first installed of KJV, WEB, then any.
    pub default_translation: Option<TranslationId>,
    /// Earlier choices: [`song_key`] → the song the person picked.
    pub remembered: HashMap<String, SongId>,
}

/// The key a song choice is remembered by: the source kind and the song's
/// normalised title, so "Indescribable" next week finds this week's pick.
pub fn song_key(source_kind: &str, title: &str) -> String {
    format!("{source_kind}:{}", normalise(title))
}

/// Matches every entry. Writes nothing.
pub fn resolve(plan: &Plan, catalog: &Catalog, context: &Context) -> ResolvedPlan {
    let entries = plan
        .ordered()
        .into_iter()
        .map(|entry| resolve_entry(entry, &plan.source.kind, catalog, context))
        .collect();
    ResolvedPlan {
        source: plan.source.clone(),
        title: plan.title.clone(),
        starts_at: plan.starts_at.clone(),
        time_zone: plan.time_zone.clone(),
        status: plan.status,
        entries,
    }
}

fn resolve_entry(
    entry: &PlanEntry,
    source_kind: &str,
    catalog: &Catalog,
    context: &Context,
) -> ResolvedEntry {
    let mut warnings = Vec::new();
    let plan_notes = entry.notes.clone().unwrap_or_default();
    let mut notes = plan_notes.clone();
    let outcome = match entry.entry_type {
        EntryType::Header | EntryType::Slides | EntryType::Announcement | EntryType::Other => {
            if notes.contains("Not chosen yet") {
                Outcome::Placeholder {
                    reason: notes.clone(),
                }
            } else {
                Outcome::Header
            }
        }
        EntryType::Song => {
            let hint = entry.song.clone();
            let hint_title = hint
                .as_ref()
                .map_or_else(|| entry.title.clone(), |h| h.title.clone());
            let hint_author = hint.as_ref().and_then(|h| h.author.clone());
            let hint_ccli = hint.as_ref().and_then(|h| h.ccli);
            let remember_as = song_key(source_kind, &hint_title);
            let result = match_song(
                &hint_title,
                hint_author.as_deref(),
                hint_ccli,
                &remember_as,
                catalog,
                context,
            );
            let key = entry
                .key
                .clone()
                .or_else(|| hint.as_ref().and_then(|h| h.key.clone()));
            let chosen = match &result {
                SongResult::Matched { song, .. } | SongResult::Likely { song, .. } => {
                    catalog.song(*song)
                }
                _ => None,
            };
            notes = join_notes(key_note(key.as_deref(), chosen), &plan_notes);
            let arrangement = match (entry.arrangement.as_deref(), chosen) {
                (Some(a), Some(song)) => match arrange(a, song) {
                    Ok(order) => order,
                    Err(unknown) => {
                        warnings.push(format!(
                            "The arrangement names {unknown}, which \"{}\" does not have, so the song's own order is used.",
                            song.title
                        ));
                        None
                    }
                },
                _ => None,
            };
            Outcome::Song {
                result,
                arrangement,
                hint_title,
                hint_author,
                hint_ccli,
                hint_key: key,
                remember_as,
            }
        }
        EntryType::Scripture => {
            let reference = entry
                .reference
                .clone()
                .unwrap_or_else(|| entry.title.clone());
            match match_passage(
                &reference,
                entry.translation.as_deref(),
                context,
                &mut warnings,
            ) {
                Ok(result) => Outcome::Scripture { result },
                Err(reason) => Outcome::Placeholder { reason },
            }
        }
    };
    ResolvedEntry {
        source_id: entry.source_id.clone(),
        entry_type: entry.entry_type,
        title: entry.title.clone(),
        notes,
        outcome,
        warnings,
    }
}

fn match_song(
    title: &str,
    author: Option<&str>,
    ccli: Option<u64>,
    remember_as: &str,
    catalog: &Catalog,
    context: &Context,
) -> SongResult {
    let found = |song: &Song, how| SongResult::Matched {
        song: song.id,
        title: song.title.clone(),
        how,
    };

    // 1. CCLI number.
    if let Some(ccli) = ccli {
        let by_number: Vec<&Song> = catalog
            .songs
            .iter()
            .filter(|s| s.ccli_number.map(u64::from) == Some(ccli))
            .collect();
        match by_number.as_slice() {
            [one] => return found(one, MatchedBy::Ccli),
            [] => {}
            several => return choose(several),
        }
    }
    // 2. What the person picked last time, if the song still exists.
    if let Some(song) = context
        .remembered
        .get(remember_as)
        .and_then(|id| catalog.song(*id))
    {
        return found(song, MatchedBy::Remembered);
    }
    // 3. The title, with the author breaking a tie.
    let want = normalise(title);
    if want.is_empty() {
        return SongResult::NotFound;
    }
    let same: Vec<&Song> = catalog
        .songs
        .iter()
        .filter(|s| normalise(&s.title) == want)
        .collect();
    match same.as_slice() {
        [one] => return found(one, MatchedBy::Title),
        [] => {}
        several => {
            let by_author: Vec<&Song> = match author {
                Some(a) => several
                    .iter()
                    .copied()
                    .filter(|s| s.author.as_deref().is_some_and(|sa| same_author(sa, a)))
                    .collect(),
                None => Vec::new(),
            };
            return match by_author.as_slice() {
                [one] => found(one, MatchedBy::Title),
                _ => choose(several),
            };
        }
    }
    // 4. Likely: the same title once brackets are dropped on both sides…
    let bare = normalise(&drop_brackets(title));
    let loose: Vec<&Song> = catalog
        .songs
        .iter()
        .filter(|s| !bare.is_empty() && normalise(&drop_brackets(&s.title)) == bare)
        .collect();
    match loose.as_slice() {
        [one] => {
            return SongResult::Likely {
                song: one.id,
                title: one.title.clone(),
            }
        }
        [] => {}
        several => return choose(several),
    }
    // …or one title that holds the other's words…
    let planned = word_set(&bare);
    let mut contained: Vec<(f32, &Song)> = catalog
        .songs
        .iter()
        .filter_map(|s| {
            let their_title = normalise(&drop_brackets(&s.title));
            let theirs = word_set(&their_title);
            let small = planned.len().min(theirs.len());
            let holds = theirs.is_subset(&planned) || planned.is_subset(&theirs);
            (small >= 2 && holds)
                .then(|| (small as f32 / planned.len().max(theirs.len()) as f32, s))
        })
        .collect();
    contained.sort_by(|a, b| b.0.total_cmp(&a.0));
    match contained.as_slice() {
        [(_, one)] => {
            return SongResult::Likely {
                song: one.id,
                title: one.title.clone(),
            }
        }
        [(a, one), (b, _), ..] if a - b >= LIKELY_MARGIN => {
            return SongResult::Likely {
                song: one.id,
                title: one.title.clone(),
            }
        }
        [] => {}
        several => return choose(&several.iter().map(|(_, s)| *s).collect::<Vec<_>>()),
    }
    // …or one clear best search hit.
    let query = drop_brackets(title);
    let perfect = perfect_score(&query);
    let hits = catalog.index.search(&query, CANDIDATES);
    if perfect <= 0.0 || hits.is_empty() {
        return SongResult::NotFound;
    }
    let first = hits[0].score / perfect;
    let second = hits.get(1).map_or(0.0, |h| h.score / perfect);
    if first >= LIKELY_AT && first - second >= LIKELY_MARGIN {
        return SongResult::Likely {
            song: hits[0].song,
            title: hits[0].title.clone(),
        };
    }
    let candidates: Vec<&Song> = hits
        .iter()
        .filter(|h| h.score / perfect >= LIKELY_AT / 2.0)
        .filter_map(|h| catalog.song(h.song))
        .collect();
    if candidates.is_empty() {
        SongResult::NotFound
    } else {
        choose(&candidates)
    }
}

fn choose(songs: &[&Song]) -> SongResult {
    SongResult::Choose {
        candidates: songs
            .iter()
            .take(CANDIDATES)
            .map(|s| SongCandidate {
                song: s.id,
                title: s.title.clone(),
                author: s.author.clone(),
            })
            .collect(),
    }
}

fn match_passage(
    reference: &str,
    translation: Option<&str>,
    context: &Context,
    warnings: &mut Vec<String>,
) -> Result<PassageResult, String> {
    let asked = translation.map(str::trim).filter(|t| !t.is_empty());
    let chosen = asked.and_then(|t| find_translation(&context.translations, t));
    let fallback = context
        .default_translation
        .and_then(|id| context.translations.iter().find(|t| t.id == id))
        .or_else(|| {
            ["KJV", "WEB"]
                .iter()
                .find_map(|a| find_translation(&context.translations, a))
        })
        .or_else(|| context.translations.first());
    let Some(used) = chosen.or(fallback) else {
        return Err(format!(
            "{reference}: no Bible translation is installed. Add one in the Library."
        ));
    };
    if let (Some(asked), None) = (asked, chosen) {
        warnings.push(format!(
            "{asked} is not installed, so {} is used.",
            abbreviation(used)
        ));
    }
    let translation_name = abbreviation(used);
    match parse_reference(reference) {
        Ok(passage) => Ok(PassageResult::Found {
            passage,
            translation: used.id,
            translation_name,
        }),
        Err(ReferenceError::AmbiguousBook(_, books)) => {
            // The chapter and verses: from the first digit after the book.
            let r = reference.trim();
            let at = r
                .char_indices()
                .skip(1)
                .find(|(_, c)| c.is_ascii_digit())
                .map_or(r.len(), |(i, _)| i);
            let rest = r[at..].trim();
            let candidates: Vec<Passage> = books
                .iter()
                .filter_map(|b| parse_reference(&format!("{} {rest}", b.name())).ok())
                .collect();
            if candidates.is_empty() {
                Err(format!(
                    "{reference}: the book could be several. Fix it in the plan."
                ))
            } else {
                Ok(PassageResult::Choose {
                    candidates,
                    translation: used.id,
                    translation_name,
                })
            }
        }
        Err(_) => Err(format!(
            "{reference}: Lampstand could not read this reference."
        )),
    }
}

fn abbreviation(t: &Translation) -> String {
    if t.abbreviation.is_empty() {
        t.name.clone()
    } else {
        t.abbreviation.clone()
    }
}

/// "Key: A", or "Key: A (song is in Bb)" when the song is written in another key.
fn key_note(key: Option<&str>, song: Option<&Song>) -> Option<String> {
    let key = key?.trim();
    if key.is_empty() {
        return None;
    }
    match song.and_then(|s| s.key.as_deref()).map(str::trim) {
        Some(own) if !own.is_empty() && !own.eq_ignore_ascii_case(key) => {
            Some(format!("Key: {key} (song is in {own})"))
        }
        _ => Some(format!("Key: {key}")),
    }
}

fn join_notes(first: Option<String>, rest: &str) -> String {
    match (first, rest.trim()) {
        (Some(a), "") => a,
        (Some(a), b) => format!("{a}\n{b}"),
        (None, b) => b.to_string(),
    }
}

/// "V1 C v2, C" → the song's own labels in that order. `Err` names the first
/// token the song has no section for.
fn arrange(arrangement: &str, song: &Song) -> Result<Option<Vec<String>>, String> {
    let tokens: Vec<&str> = arrangement
        .split([' ', ','])
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        return Ok(None);
    }
    let mut order = Vec::with_capacity(tokens.len());
    for token in tokens {
        let label = song
            .sections
            .iter()
            .find(|s| s.label.eq_ignore_ascii_case(token))
            .ok_or_else(|| format!("\"{token}\""))?;
        order.push(label.label.clone());
    }
    Ok(Some(order))
}

/// Lower case, no accents or punctuation, no leading "the", single spaces.
/// Digit grouping is ignored: "10,000" and "10000" are the same.
pub fn normalise(title: &str) -> String {
    let chars: Vec<char> = title.chars().collect();
    let ungrouped: String = chars
        .iter()
        .enumerate()
        .filter(|&(i, c)| {
            let between_digits = i > 0
                && chars.get(i + 1).is_some_and(char::is_ascii_digit)
                && chars[i - 1].is_ascii_digit();
            !(matches!(c, ',' | '.' | '\u{2009}' | '\u{202f}') && between_digits)
        })
        .map(|(_, c)| *c)
        .collect();
    let mut w = words(&ungrouped);
    if w.first().is_some_and(|f| f == "the") && w.len() > 1 {
        w.remove(0);
    }
    w.join(" ")
}

fn word_set(normalised: &str) -> std::collections::HashSet<&str> {
    normalised.split(' ').filter(|w| !w.is_empty()).collect()
}

fn drop_brackets(title: &str) -> String {
    let mut out = String::new();
    let mut depth = 0u32;
    for c in title.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

fn same_author(a: &str, b: &str) -> bool {
    let (a, b) = (normalise(a), normalise(b));
    !a.is_empty() && (a == b || a.contains(&b) || b.contains(&a))
}

// --- Building the service.

/// What `build_service` produced besides the service.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Built {
    /// Songs to write before the service: the blank ones the person asked for.
    pub new_songs: Vec<Song>,
    /// Choices to remember: [`song_key`] → song.
    pub remember: HashMap<String, SongId>,
}

/// The service the review describes. A song or passage without a settled
/// choice (a likely match nobody confirmed, a "choose one", not found)
/// becomes a placeholder header, never a guess. Each item records the entry
/// it came from and its import hash; the service records the plan.
pub fn build_service(
    resolved: &ResolvedPlan,
    choices: &HashMap<String, Choice>,
    catalog: &Catalog,
) -> (Service, Built) {
    let mut service = Service::new(resolved.title.clone());
    service.starts_at = resolved
        .starts_at
        .as_deref()
        .and_then(lampstand_core::plan::rfc3339_millis)
        .map(lampstand_core::id::Timestamp);
    service.time_zone = resolved.time_zone.clone();
    service.source = Some(resolved.source.clone());
    let mut built = Built::default();
    let mut last: Option<Position> = None;

    for entry in &resolved.entries {
        let choice = choices
            .get(&entry.source_id)
            .cloned()
            .unwrap_or(Choice::Accept);
        let (kind, notes) = item_for(entry, &choice, catalog, &mut built);
        let position = Position::between(last.as_ref(), None);
        last = Some(position.clone());
        let mut item = ServiceItem::new(position, kind);
        item.notes = notes;
        item.source_id = Some(entry.source_id.clone());
        item.import_hash = Some(item.content_hash());
        service
            .items
            .insert(lampstand_core::service::ItemId::generate(), item);
    }
    (service, built)
}

fn item_for(
    entry: &ResolvedEntry,
    choice: &Choice,
    catalog: &Catalog,
    built: &mut Built,
) -> (ItemKind, String) {
    let header = |title: String| ItemKind::Header { title };
    let placeholder = |mark: &str, why: &str| {
        (
            header(format!("{mark} {}", entry.title)),
            join_notes(Some(why.to_string()), &entry.notes),
        )
    };
    match &entry.outcome {
        Outcome::Header => (header(entry.title.clone()), entry.notes.clone()),
        Outcome::Placeholder { reason } => {
            let title = match entry.entry_type {
                EntryType::Scripture => format!("📖 {}", entry.title),
                EntryType::Song => format!("♪ {}", entry.title),
                _ => entry.title.clone(),
            };
            let notes = if entry.notes.contains(reason.as_str()) {
                entry.notes.clone()
            } else {
                join_notes(Some(reason.clone()), &entry.notes)
            };
            (header(title), notes)
        }
        Outcome::Song {
            result,
            arrangement,
            hint_title,
            hint_author,
            hint_ccli,
            hint_key,
            remember_as,
        } => {
            let picked = match (choice, result) {
                (Choice::Song { song }, _) => catalog.song(*song).map(|s| s.id),
                (Choice::Accept, SongResult::Matched { song, .. }) => Some(*song),
                // A likely match is only used when the person confirms it.
                (Choice::Accept, _) => None,
                (Choice::CreateBlank, _) => {
                    let mut song = Song::new(hint_title.clone());
                    song.author = hint_author.clone();
                    song.ccli_number = hint_ccli.and_then(|n| u32::try_from(n).ok());
                    song.key = hint_key.clone();
                    let id = song.id;
                    built.new_songs.push(song);
                    Some(id)
                }
                _ => None,
            };
            match picked {
                Some(song) => {
                    let confirmed = !matches!(
                        (choice, result),
                        (
                            Choice::Accept,
                            SongResult::Matched {
                                how: MatchedBy::Remembered | MatchedBy::Ccli,
                                ..
                            }
                        )
                    );
                    if confirmed || matches!(choice, Choice::Song { .. }) {
                        built.remember.insert(remember_as.clone(), song);
                    }
                    // The arrangement was checked against the matched song only.
                    let same_song = matches!(result, SongResult::Matched { song: s, .. } | SongResult::Likely { song: s, .. } if *s == song);
                    let arrangement = if same_song { arrangement.clone() } else { None };
                    (ItemKind::Song { song, arrangement }, entry.notes.clone())
                }
                None => placeholder(
                    "♪",
                    "Song not chosen yet. Pick it in the review or in Plan.",
                ),
            }
        }
        Outcome::Scripture { result } => match (choice, result) {
            (Choice::Placeholder, _) => placeholder("📖", "Reading left as a placeholder."),
            (
                _,
                PassageResult::Found {
                    passage,
                    translation,
                    ..
                },
            ) => (
                ItemKind::Scripture {
                    passage: *passage,
                    translation: *translation,
                },
                entry.notes.clone(),
            ),
            (
                Choice::Passage { index },
                PassageResult::Choose {
                    candidates,
                    translation,
                    ..
                },
            ) => match candidates.get(*index as usize) {
                Some(passage) => (
                    ItemKind::Scripture {
                        passage: *passage,
                        translation: *translation,
                    },
                    entry.notes.clone(),
                ),
                None => placeholder("📖", "Passage not chosen yet."),
            },
            _ => placeholder("📖", "Passage not chosen yet."),
        },
    }
}
