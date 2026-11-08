//! Reads a service plan written as an ordinary Notion page: the
//! chairperson's order of service that Mona Heights Chapel keeps, one page
//! per Sunday (decision 0007, revised).
//!
//! The page is a script, not a table. What it reads:
//!
//! - **The title** "Service Plan – November 8, 2026 – Daniel Fokum". The
//!   service is named by the segments up to and including the date; the
//!   rest is a person's name and is dropped.
//! - **"Status: Draft"** anywhere before the order of service.
//! - **Timed headings**, "10:05 am – Opening Songs". Each one starts a
//!   section and becomes a header item. The first one's time is when the
//!   service starts. Sermon and announcement sections become placeholders
//!   for a deck.
//! - **Song lines** in a section: "Opening Song: Come Thou Almighty King
//!   (C major)", "Communion Hymn: Man of Sorrows, What a Name", and an
//!   unlabelled list directly under a song or worship heading.
//! - **Readings**: a line that starts with a reference, "Romans 8:18-21
//!   (NKJV) — est. 1 minute", or a labelled one, "Communion Passages: Mark
//!   14:12-26; Exodus 12:7-20 (NIV)".
//! - **"TBD" or "TBA"** for a song or reading, also in a callout that says
//!   nothing else (the template's 🔶 marker): a placeholder that says it has
//!   not been chosen yet.
//!
//! What it never reads: callouts and quotes (notes for the chairperson and
//! printed verses), untimed subheadings and their lists (birthdays, the
//! prayer diary, the weekly schedule), and any free text. Nothing from the
//! page is copied into an item's notes except what this reader understood,
//! so names on the page do not reach Lampstand.

use std::collections::HashSet;

use lampstand_core::plan::{
    EntryType, Plan, PlanEntry, PlanSource, PlanStatus, SongHint, CONTRACT,
};
use lampstand_core::scripture::parse_reference_versioned;

/// One Notion block reduced to what the reader needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub id: String,
    pub kind: BlockKind,
    /// The block's own text, rich text joined as plain text.
    pub text: String,
    pub children: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    /// 1, 2 or 3.
    Heading(u8),
    Paragraph,
    Bulleted,
    Numbered,
    ToDo,
    Toggle,
    Quote,
    Callout,
    Divider,
    /// Anything else (an image, a table, a child page), by its API type.
    Other(String),
}

/// The page itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageMeta {
    pub id: String,
    pub title: String,
    /// RFC 3339.
    pub last_edited: Option<String>,
}

/// What the page cannot say for itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadOptions {
    /// The church's IANA zone ("America/Jamaica"), carried into the plan.
    pub time_zone: Option<String>,
    /// The zone's offset from UTC on the service date, in minutes (-300
    /// for Jamaica). Without it the plan has no start time.
    pub utc_offset_minutes: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// No heading like "10:00 am – Welcome": this is not an order of service.
    NoOrderOfService,
    EmptyTitle,
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoOrderOfService => write!(
                f,
                "This page has no order of service. Lampstand looks for headings that start with a time, such as \"10:00 am – Welcome\"."
            ),
            Self::EmptyTitle => write!(f, "This page has no title."),
        }
    }
}

impl std::error::Error for ReadError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadWarning {
    /// Neither the title nor a "Date:" line gave a date.
    NoDate,
    /// There is a date and time but no offset, so no start time.
    NoTimeZone,
    /// A section's time is earlier than the one before it. The page order
    /// is kept, since it is the order the chairperson reads.
    TimeOutOfOrder { heading: String },
    /// A reading line named a reference that did not parse.
    UnreadReference { text: String },
}

/// Reads one page into a plan.
pub fn page_to_plan(
    page: &PageMeta,
    blocks: &[Block],
    options: &ReadOptions,
) -> Result<(Plan, Vec<ReadWarning>), ReadError> {
    let mut warnings = Vec::new();
    let segments = title_segments(&page.title);
    if segments.is_empty() {
        return Err(ReadError::EmptyTitle);
    }

    let lines = flatten(blocks);
    let order_start = lines
        .iter()
        .position(|l| matches!(l.kind, LineKind::Heading) && squash(&l.text) == "orderofservice")
        .map(|i| i + 1)
        .or_else(|| {
            lines
                .iter()
                .position(|l| matches!(l.kind, LineKind::Heading) && timed(&l.text).is_some())
        })
        .ok_or(ReadError::NoOrderOfService)?;

    // The heading of the service, before the order.
    let mut status = None;
    let mut dated_line = None;
    for line in &lines[..order_start] {
        if let Some(v) = labelled(&line.text, "status") {
            status = match squash(v).as_str() {
                "draft" => Some(PlanStatus::Draft),
                "ready" | "final" | "done" => Some(PlanStatus::Ready),
                _ => status,
            };
        }
        if let Some(v) = labelled(&line.text, "date") {
            dated_line = dated_line.or_else(|| numeric_date(v));
        }
    }

    // The service name stops at the date; what follows is a person.
    let date_at = segments.iter().position(|s| long_date(s).is_some());
    let date = date_at.and_then(|i| long_date(&segments[i])).or(dated_line);
    let title = match date_at {
        Some(i) => segments[..=i].join(" – "),
        None => segments.join(" – "),
    };

    let mut reader = Reader::default();
    for line in &lines[order_start..] {
        reader.line(line, &mut warnings);
    }
    if reader.sections == 0 {
        return Err(ReadError::NoOrderOfService);
    }

    let starts_at = match (date, reader.first_time) {
        (Some((y, m, d)), Some((h, min))) => match options.utc_offset_minutes {
            Some(offset) => Some(format!(
                "{y:04}-{m:02}-{d:02}T{h:02}:{min:02}:00{}",
                offset_text(offset)
            )),
            None => {
                warnings.push(ReadWarning::NoTimeZone);
                None
            }
        },
        (None, _) => {
            warnings.push(ReadWarning::NoDate);
            None
        }
        (Some(_), None) => None,
    };

    let plan = Plan {
        contract: CONTRACT.to_string(),
        source: PlanSource {
            kind: "notion".into(),
            id: page.id.clone(),
            edited: page.last_edited.clone(),
        },
        title,
        starts_at,
        time_zone: options.time_zone.clone(),
        status,
        entries: reader.entries,
    };
    Ok((plan, warnings))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    Heading,
    Text,
    List,
}

struct Line {
    id: String,
    kind: LineKind,
    text: String,
}

/// Blocks in reading order, one line per line of text, leaving out
/// callouts and quotes with everything inside them.
fn flatten(blocks: &[Block]) -> Vec<Line> {
    fn walk(blocks: &[Block], out: &mut Vec<Line>) {
        for b in blocks {
            let kind = match b.kind {
                // A callout is a note for the chairperson, except the
                // template's "🔶 TBA" marker for something not chosen yet.
                BlockKind::Callout => {
                    if let Some(marker) = not_chosen_marker(b) {
                        out.push(Line {
                            id: b.id.clone(),
                            kind: LineKind::Text,
                            text: marker,
                        });
                    }
                    continue;
                }
                BlockKind::Quote | BlockKind::Divider => continue,
                BlockKind::Heading(_) => LineKind::Heading,
                BlockKind::Bulleted | BlockKind::Numbered | BlockKind::ToDo => LineKind::List,
                BlockKind::Paragraph | BlockKind::Toggle => LineKind::Text,
                BlockKind::Other(_) => {
                    walk(&b.children, out);
                    continue;
                }
            };
            let parts: Vec<&str> = b
                .text
                .lines()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .collect();
            for (i, text) in parts.iter().enumerate() {
                let id = if i == 0 {
                    b.id.clone()
                } else {
                    format!("{}#{}", b.id, i + 1)
                };
                out.push(Line {
                    id,
                    kind,
                    text: (*text).to_string(),
                });
            }
            walk(&b.children, out);
        }
    }
    let mut out = Vec::new();
    walk(blocks, &mut out);
    out
}

/// "TBA" when a callout says nothing but TBD or TBA, in itself or its
/// children.
fn not_chosen_marker(callout: &Block) -> Option<String> {
    let mut texts = vec![callout.text.as_str()];
    texts.extend(
        callout
            .children
            .iter()
            .filter(|c| c.children.is_empty())
            .map(|c| c.text.as_str()),
    );
    let said: Vec<&str> = texts
        .into_iter()
        .filter(|t| !squash(t).is_empty())
        .collect();
    (!said.is_empty() && said.iter().all(|t| not_chosen(t))).then(|| "TBA".to_string())
}

#[derive(Default)]
struct Reader {
    entries: Vec<PlanEntry>,
    ids: HashSet<String>,
    sections: usize,
    first_time: Option<(u32, u32)>,
    last_time: Option<u32>,
    /// The current section's title and what it is about.
    section: Option<Section>,
    /// An untimed subheading has appeared since the section began.
    in_subsection: bool,
}

struct Section {
    songs: bool,
    readings: bool,
}

impl Reader {
    fn line(&mut self, line: &Line, warnings: &mut Vec<ReadWarning>) {
        if line.kind == LineKind::Heading {
            match timed(&line.text) {
                Some((time, rest)) => self.section(line, time, rest, warnings),
                None => self.in_subsection = true,
            }
            return;
        }
        if self.section.is_none() || self.in_subsection {
            return;
        }
        self.content(&line.id, line.kind, &line.text, warnings);
    }

    fn section(
        &mut self,
        line: &Line,
        (h, m): (u32, u32),
        rest: &str,
        warnings: &mut Vec<ReadWarning>,
    ) {
        // "First Scripture Reading: TBD" names the section and says what is in it.
        let (title, value) = match split_label(rest) {
            Some((label, value)) => (label.to_string(), Some(value)),
            None => (rest.to_string(), None),
        };
        let minutes = h * 60 + m;
        if self.last_time.is_some_and(|last| minutes < last) {
            warnings.push(ReadWarning::TimeOutOfOrder {
                heading: line.text.clone(),
            });
        }
        self.last_time = Some(minutes);
        self.first_time.get_or_insert((h, m));
        self.sections += 1;
        self.in_subsection = false;

        let key = squash(&title);
        let entry_type = if key.contains("sermon") || key.contains("message") {
            EntryType::Slides
        } else if key.contains("announcement") || key.contains("notices") {
            EntryType::Announcement
        } else {
            EntryType::Header
        };
        let mut notes = vec![format!("Planned for {}", clock(h, m))];
        if matches!(entry_type, EntryType::Slides | EntryType::Announcement) {
            notes.push("Deck to attach".into());
        }
        let mut entry = PlanEntry::new(self.id(&line.id), 0.0, entry_type, title.clone());
        entry.notes = Some(notes.join("\n"));
        self.push(entry);

        self.section = Some(Section {
            songs: ["song", "hymn", "worship", "praise"]
                .iter()
                .any(|w| key.contains(w)),
            readings: ["scripture", "reading", "passage"]
                .iter()
                .any(|w| key.contains(w)),
        });
        if let Some(value) = value {
            self.content(
                &format!("{}#value", line.id),
                LineKind::Text,
                value,
                warnings,
            );
        }
    }

    fn content(&mut self, id: &str, kind: LineKind, text: &str, warnings: &mut Vec<ReadWarning>) {
        let Some(section) = &self.section else { return };
        let (label, value) = match split_label(text) {
            Some((l, v)) => (Some(l), v),
            None => (None, text),
        };
        let label_key = label.map(squash).unwrap_or_default();
        let songish = |k: &str| k.contains("song") || k.contains("hymn");
        let readingish = |k: &str| {
            ["scripture", "reading", "passage"]
                .iter()
                .any(|w| k.contains(w))
        };

        let is_song = match label {
            Some(_) => songish(&label_key),
            None => section.songs && kind == LineKind::List,
        };
        let is_reading = match label {
            Some(_) => readingish(&label_key),
            None => section.readings || starts_with_reference(value),
        };

        if not_chosen(value) {
            // "Leader: TBD" and the like are about people and are skipped.
            let title = match label {
                Some(l) if is_song || is_reading => l.to_string(),
                // The section's header comes just before, so "Song" or
                // "Reading" says enough.
                None if section.songs => "Song".to_string(),
                None if section.readings => "Reading".to_string(),
                _ => return,
            };
            let mut entry = PlanEntry::new(self.id(id), 0.0, EntryType::Other, title);
            entry.notes = Some("Not chosen yet in Notion".into());
            self.push(entry);
            return;
        }

        if is_song {
            let (title, key) = song_and_key(value);
            if title.is_empty() {
                return;
            }
            let mut entry = PlanEntry::new(self.id(id), 0.0, EntryType::Song, title.clone());
            entry.song = Some(SongHint {
                title,
                author: None,
                ccli: None,
                key: key.clone(),
            });
            entry.key = key;
            self.push(entry);
            return;
        }

        if is_reading {
            match references(value) {
                Some(found) => {
                    for (i, (reference, translation)) in found.into_iter().enumerate() {
                        let source = if i == 0 {
                            id.to_string()
                        } else {
                            format!("{id}#{}", i + 1)
                        };
                        let mut entry = PlanEntry::new(
                            self.id(&source),
                            0.0,
                            EntryType::Scripture,
                            reference.clone(),
                        );
                        entry.reference = Some(reference);
                        entry.translation = translation;
                        self.push(entry);
                    }
                }
                None if label.is_some() || starts_with_reference(value) => {
                    warnings.push(ReadWarning::UnreadReference {
                        text: value.to_string(),
                    });
                }
                None => {}
            }
        }
    }

    /// A source ID not used yet in this plan.
    fn id(&mut self, wanted: &str) -> String {
        let mut id = wanted.to_string();
        let mut n = 2;
        while self.ids.contains(&id) {
            id = format!("{wanted}~{n}");
            n += 1;
        }
        self.ids.insert(id.clone());
        id
    }

    fn push(&mut self, mut entry: PlanEntry) {
        entry.order = (self.entries.len() + 1) as f64;
        self.entries.push(entry);
    }
}

/// "10:05 am – Opening Songs" → ((10, 5), "Opening Songs"), in 24-hour time.
fn timed(text: &str) -> Option<((u32, u32), &str)> {
    let text = text.trim();
    let colon = text.find(':')?;
    let hour: u32 = text[..colon].trim().parse().ok()?;
    let after = &text[colon + 1..];
    let digits = after.chars().take_while(char::is_ascii_digit).count();
    if digits != 2 {
        return None;
    }
    let minute: u32 = after[..2].parse().ok()?;
    let mut rest = after[2..].trim_start();
    let lower = rest.to_ascii_lowercase();
    let mut hour24 = hour;
    for (mark, pm) in [("a.m.", false), ("p.m.", true), ("am", false), ("pm", true)] {
        if lower.starts_with(mark) {
            hour24 = match (hour, pm) {
                (12, false) => 0,
                (12, true) => 12,
                (h, true) => h + 12,
                (h, false) => h,
            };
            rest = rest[mark.len()..].trim_start();
            break;
        }
    }
    if hour24 > 23 || minute > 59 {
        return None;
    }
    let rest = rest.trim_start_matches(['–', '—', '-', ':', '|']).trim();
    if rest.is_empty() {
        return None;
    }
    Some(((hour24, minute), rest))
}

fn clock(h: u32, m: u32) -> String {
    let (h12, mark) = match h {
        0 => (12, "am"),
        1..=11 => (h, "am"),
        12 => (12, "pm"),
        _ => (h - 12, "pm"),
    };
    format!("{h12}:{m:02} {mark}")
}

/// "Opening Song: X" → ("Opening Song", "X"). Not a label when the colon is
/// part of a reference or a time ("Romans 8:18", "10:05").
fn split_label(text: &str) -> Option<(&str, &str)> {
    let colon = text.find(':')?;
    let (label, value) = (text[..colon].trim(), text[colon + 1..].trim());
    let label_ok = !label.is_empty()
        && label.chars().count() <= 40
        && !label.ends_with(|c: char| c.is_ascii_digit());
    let value_ok = !value.starts_with(|c: char| c.is_ascii_digit());
    (label_ok && value_ok).then_some((label, value))
}

/// The value of "Label: value" when the label is `name`, ignoring case.
fn labelled<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    // "Date: 11/08/2026 Status: Draft" on one line holds two labels.
    let lower = text.to_ascii_lowercase();
    let at = lower.find(&format!("{name}:"))?;
    if at > 0
        && lower[..at]
            .chars()
            .last()
            .is_some_and(char::is_alphanumeric)
    {
        return None;
    }
    let value = &text[at + name.len() + 1..];
    let end = value.find('\n').unwrap_or(value.len());
    let value = value[..end].trim();
    // Stop at the next "Word:" label on the same line.
    let cut = value
        .match_indices(':')
        .filter_map(|(i, _)| value[..i].rfind(' ').map(|s| (s, i)))
        .find(|&(s, i)| value[s + 1..i].chars().all(char::is_alphabetic))
        .map_or(value.len(), |(s, _)| s);
    Some(value[..cut].trim())
}

/// "TBD", "TBA 🔶", "To be decided".
fn not_chosen(value: &str) -> bool {
    let k = squash(value);
    matches!(
        k.as_str(),
        "tbd" | "tba" | "tbc" | "tobedecided" | "tobeannounced" | "tobeconfirmed" | ""
    )
}

fn starts_with_reference(text: &str) -> bool {
    references(text).is_some()
}

/// "Mark 14:12-26; Exodus 12:7-20 (NIV)" → both passages, each in NIV.
/// Anything after a spaced dash is commentary ("— est. 1 minute").
fn references(text: &str) -> Option<Vec<(String, Option<String>)>> {
    let mut head = text.trim();
    for cut in [" — ", " – ", " - ", " —", " –"] {
        if let Some(i) = head.find(cut) {
            head = &head[..i];
        }
    }
    let parts: Vec<&str> = head
        .split(';')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty() {
        return None;
    }
    let mut found = Vec::new();
    for part in &parts {
        let (passage, version) = parse_reference_versioned(part).ok()?;
        found.push((passage.to_string(), version));
    }
    // A version given once, at the end, is for every passage.
    let shared = found.last().and_then(|(_, v)| v.clone());
    Some(
        found
            .into_iter()
            .map(|(p, v)| (p, v.or_else(|| shared.clone())))
            .collect(),
    )
}

/// "Come Thou Almighty King (C major)" → ("Come Thou Almighty King", "C").
/// A bracket that is not a key stays part of the title.
fn song_and_key(value: &str) -> (String, Option<String>) {
    let value = value.trim().trim_matches(['“', '”', '"']).trim();
    if let (Some(open), true) = (value.rfind('('), value.ends_with(')')) {
        if let Some(key) = key_name(&value[open + 1..value.len() - 1]) {
            let title = value[..open].trim().trim_matches(['“', '”', '"']).trim();
            return (title.to_string(), Some(key));
        }
    }
    (value.to_string(), None)
}

/// "C major" → "C", "Key of Bb" → "Bb", "F# minor" → "F#m", "E♭" → "Eb".
fn key_name(text: &str) -> Option<String> {
    let t = text.trim().replace('♭', "b").replace('♯', "#");
    let t = t
        .strip_prefix("Key of ")
        .or_else(|| t.strip_prefix("key of "))
        .or_else(|| t.strip_prefix("Key: "))
        .unwrap_or(&t);
    let mut chars = t.chars();
    let note = chars.next()?.to_ascii_uppercase();
    if !('A'..='G').contains(&note) {
        return None;
    }
    let rest: String = chars.collect();
    let (accidental, rest) = match rest.chars().next() {
        Some(c @ ('#' | 'b')) => (c.to_string(), rest[1..].to_string()),
        _ => (String::new(), rest),
    };
    let mode = match squash(&rest).as_str() {
        "" | "major" | "maj" => "",
        "minor" | "min" | "m" => "m",
        _ => return None,
    };
    Some(format!("{note}{accidental}{mode}"))
}

/// The title split at spaced dashes: "A – B – C" → ["A", "B", "C"].
fn title_segments(title: &str) -> Vec<String> {
    let mut s = title.to_string();
    for dash in [" — ", " - "] {
        s = s.replace(dash, " – ");
    }
    s.split(" – ")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect()
}

const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

/// "November 8, 2026", "Nov 8 2026", "8 November 2026".
fn long_date(text: &str) -> Option<(u32, u32, u32)> {
    let words: Vec<String> = text
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|w| !w.is_empty())
        .map(|w| w.trim_end_matches(['.']).to_ascii_lowercase())
        .collect();
    if words.len() != 3 {
        return None;
    }
    let month_of = |w: &str| {
        (w.len() >= 3)
            .then(|| MONTHS.iter().position(|m| m.starts_with(w)))
            .flatten()
            .map(|i| i as u32 + 1)
    };
    let day_of = |w: &str| {
        w.trim_end_matches(|c: char| c.is_alphabetic())
            .parse::<u32>()
            .ok()
            .filter(|d| (1..=31).contains(d))
    };
    let year: u32 = words[2]
        .parse()
        .ok()
        .filter(|y| (1900..=2200).contains(y))?;
    if let (Some(m), Some(d)) = (month_of(&words[0]), day_of(&words[1])) {
        return Some((year, m, d));
    }
    if let (Some(d), Some(m)) = (day_of(&words[0]), month_of(&words[1])) {
        return Some((year, m, d));
    }
    None
}

/// "11/08/2026", as Notion shows a date in US format, or "2026-11-08".
fn numeric_date(text: &str) -> Option<(u32, u32, u32)> {
    let t = text.split_whitespace().next()?;
    if let [y, m, d] = t.split('-').collect::<Vec<_>>()[..] {
        let (y, m, d) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
        return valid_date(y, m, d);
    }
    if let [m, d, y] = t.split('/').collect::<Vec<_>>()[..] {
        let (y, m, d) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
        return valid_date(y, m, d);
    }
    None
}

fn valid_date(y: u32, m: u32, d: u32) -> Option<(u32, u32, u32)> {
    ((1900..=2200).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d))
        .then_some((y, m, d))
}

fn offset_text(minutes: i32) -> String {
    let sign = if minutes < 0 { '-' } else { '+' };
    let m = minutes.unsigned_abs();
    format!("{sign}{:02}:{:02}", m / 60, m % 60)
}

/// Lower case, letters and digits only.
fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_timed_headings() {
        assert_eq!(
            timed("10:05 am – Opening Songs"),
            Some(((10, 5), "Opening Songs"))
        );
        assert_eq!(
            timed("12:05 pm – Call for Response"),
            Some(((12, 5), "Call for Response"))
        );
        assert_eq!(timed("1:30 PM - Lunch"), Some(((13, 30), "Lunch")));
        assert_eq!(timed("Opening Remarks"), None);
        assert_eq!(timed("10:05 am"), None);
    }

    #[test]
    fn labels_are_not_references() {
        assert_eq!(
            split_label("Opening Song: Come Thou Almighty King"),
            Some(("Opening Song", "Come Thou Almighty King"))
        );
        assert_eq!(split_label("Romans 8:18-21 (NKJV)"), None);
        assert_eq!(
            split_label("Communion Passages: Mark 14:12-26"),
            Some(("Communion Passages", "Mark 14:12-26"))
        );
    }

    #[test]
    fn reads_keys_only_when_they_are_keys() {
        assert_eq!(
            song_and_key("Come Thou Almighty King (C major)"),
            ("Come Thou Almighty King".into(), Some("C".into()))
        );
        assert_eq!(
            song_and_key("Be Still (Key of Bb)"),
            ("Be Still".into(), Some("Bb".into()))
        );
        assert_eq!(
            song_and_key("Lament (F# minor)"),
            ("Lament".into(), Some("F#m".into()))
        );
        let alt = "I Exalt You (Thou Art High, O Lord, Above All the Earth)";
        assert_eq!(song_and_key(alt), (alt.into(), None));
    }

    #[test]
    fn reads_several_passages_with_one_version() {
        let found = references("Mark 14:12-26; Exodus 12:7-20 (NIV)").unwrap();
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|(_, v)| v.as_deref() == Some("NIV")));
        let found = references("Romans 8:18-21 (NKJV) — est. 1 minute, read alternately").unwrap();
        assert_eq!(found[0].1.as_deref(), Some("NKJV"));
        assert!(references("Nov 9 – a birthday").is_none());
    }

    #[test]
    fn reads_dates() {
        assert_eq!(long_date("November 8, 2026"), Some((2026, 11, 8)));
        assert_eq!(long_date("8 Nov 2026"), Some((2026, 11, 8)));
        assert_eq!(long_date("Daniel Fokum"), None);
        assert_eq!(numeric_date("11/08/2026"), Some((2026, 11, 8)));
        assert_eq!(
            labelled("Date: 11/08/2026 Status: Draft", "date"),
            Some("11/08/2026")
        );
        assert_eq!(
            labelled("Date: 11/08/2026 Status: Draft", "status"),
            Some("Draft")
        );
    }
}
