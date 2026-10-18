//! The Lampstand song library: importers and search.
//!
//! PRD: SL-3 (search), SL-4 (import), SL-2 (chords stored inline).
//!
//! # Importers
//!
//! [`openlyrics`], [`opensong`] and [`chordpro`] each turn one file's text
//! into an [`Imported`]: the songs found plus a [`Report`] of anything that
//! could not be carried over (extra languages, comments, formatting, fields
//! the model has no place for). A file that cannot be read at all is an
//! [`ImportError`]. [`import_file`] and [`import_dir`] detect the format from
//! the contents, since OpenSong files usually have no extension.
//!
//! Chords from every format end up inline in ChordPro form (`[G]Amazing`);
//! see [`lampstand_core::song`].
//!
//! # Search
//!
//! [`search::SongIndex`] is a small in-memory inverted index with BM25-style
//! scoring, a title boost, prefix matching on the last word typed and typo
//! tolerance by bounded Levenshtein distance over the vocabulary.
//!
//! We chose this over tantivy on purpose. A church library is a few thousand
//! songs and at most tens of thousands of distinct words, so a full scan of
//! the vocabulary per query word takes well under a millisecond, and the
//! whole index rebuilds from the song store in milliseconds at start-up.
//! That means no on-disk index to keep consistent with the synced song store,
//! no schema migrations, and no large dependency tree in the desktop app.
//! If libraries grow far past that, the vocabulary scan can be swapped for a
//! trigram or BK-tree lookup without changing the API.

pub mod chordpro;
pub mod openlyrics;
pub mod opensong;
pub mod search;

use std::fmt;
use std::path::{Path, PathBuf};

use lampstand_core::song::{SectionKind, Song};

/// The songs read from one file and what was lost on the way.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Imported {
    pub songs: Vec<Song>,
    pub report: Report,
}

/// Things an importer could not carry over, for the migration report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Title of the song the note is about, when known.
    pub song: Option<String>,
    pub message: String,
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.song {
            Some(title) => write!(f, "{title}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl Report {
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    /// Adds a note unless the same one is already there, so a song with forty
    /// formatting tags gets one line in the report, not forty.
    pub fn note(&mut self, song: Option<&str>, message: impl Into<String>) {
        let note = Note { song: song.map(str::to_string), message: message.into() };
        if !self.notes.contains(&note) {
            self.notes.push(note);
        }
    }

    pub fn extend(&mut self, other: Report) {
        for n in other.notes {
            if !self.notes.contains(&n) {
                self.notes.push(n);
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("could not read file: {0}")]
    Io(#[from] std::io::Error),
    #[error("not valid XML: {0}")]
    Xml(#[from] roxmltree::Error),
    #[error("not a {expected} song file")]
    WrongFormat { expected: &'static str },
    #[error("format not recognised (expected OpenLyrics, OpenSong or ChordPro)")]
    UnknownFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    OpenLyrics,
    OpenSong,
    ChordPro,
}

/// Guesses the format from a file's contents.
pub fn detect(text: &str) -> Option<Format> {
    let head = text.trim_start_matches('\u{feff}').trim_start();
    if head.starts_with('<') {
        if head.contains("openlyrics.info/namespace") || head.contains("<properties") {
            return Some(Format::OpenLyrics);
        }
        if head.contains("<song") && head.contains("<lyrics") {
            return Some(Format::OpenSong);
        }
        return None;
    }
    let looks_like_chordpro = head.lines().any(|l| {
        let l = l.trim_start();
        l.starts_with('{') && l.contains('}')
    });
    looks_like_chordpro.then_some(Format::ChordPro)
}

/// Parses text in the given format.
pub fn import_str(text: &str, format: Format) -> Result<Imported, ImportError> {
    match format {
        Format::OpenLyrics => openlyrics::parse(text),
        Format::OpenSong => opensong::parse(text),
        Format::ChordPro => Ok(chordpro::parse(text)),
    }
}

/// Reads one file, detecting its format.
pub fn import_file(path: &Path) -> Result<Imported, ImportError> {
    let bytes = std::fs::read(path)?;
    // Old OpenSong and ChordPro files are often Latin-1; keep going rather
    // than refusing the file.
    let text = match String::from_utf8(bytes) {
        Ok(t) => t,
        Err(e) => e.into_bytes().iter().map(|&b| b as char).collect(),
    };
    let format = detect(&text).ok_or(ImportError::UnknownFormat)?;
    import_str(&text, format)
}

/// Imports every file under a directory, recursively, skipping hidden files.
/// Returns one result per file so the caller can build the migration report.
pub fn import_dir(dir: &Path) -> std::io::Result<Vec<(PathBuf, Result<Imported, ImportError>)>> {
    let mut results = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&dir)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.path());
        for entry in entries {
            let path = entry.path();
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if entry.file_type()?.is_dir() {
                pending.push(path);
            } else {
                let result = import_file(&path);
                results.push((path, result));
            }
        }
    }
    Ok(results)
}

/// Shared clean-up every importer runs on a finished song: drops
/// arrangement entries that name no section, and empties an arrangement
/// that just repeats the written order.
pub(crate) fn finish(mut song: Song, report: &mut Report) -> Song {
    let title = song.title.clone();
    song.arrangement.retain(|label| {
        let known = song.sections.iter().any(|s| &s.label == label);
        if !known {
            report.note(Some(&title), format!("arrangement names missing section {label}; skipped it"));
        }
        known
    });
    let written: Vec<&String> = song.sections.iter().map(|s| &s.label).collect();
    if song.arrangement.iter().collect::<Vec<_>>() == written {
        song.arrangement.clear();
    }
    if song.title.trim().is_empty() {
        song.title = song
            .sections
            .first()
            .and_then(|s| s.lyrics().into_iter().find(|l| !l.trim().is_empty()))
            .unwrap_or_else(|| "Untitled".into());
        report.note(Some(&song.title), "no title; used the first line");
    }
    song
}

/// Splits an arrangement string such as "V1 C V2 C B C" into normalised
/// labels.
pub(crate) fn parse_arrangement(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(|s| SectionKind::parse_label(s).1)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_formats() {
        assert_eq!(detect("<?xml version='1.0'?><song xmlns=\"http://openlyrics.info/namespace/2009/song\">"), Some(Format::OpenLyrics));
        assert_eq!(detect("<?xml version='1.0'?>\n<song><title>A</title><lyrics>[V]</lyrics></song>"), Some(Format::OpenSong));
        assert_eq!(detect("{title: A}\n[G]Hi"), Some(Format::ChordPro));
        assert_eq!(detect("just some text"), None);
    }

    #[test]
    fn report_deduplicates() {
        let mut r = Report::default();
        r.note(Some("A"), "x");
        r.note(Some("A"), "x");
        r.note(None, "x");
        assert_eq!(r.notes.len(), 2);
    }
}
