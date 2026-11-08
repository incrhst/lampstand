//! CCLI SongSelect downloads: `.usr` (key=value with `/t` section separators) and `.txt` lyrics files.
//!
//! A `.usr` file is INI-like. The `[S A<ccli>]` header carries the CCLI song
//! number; `Fields=` lists the section labels and `Words=` the section texts,
//! both separated by `/t`, with `/n` between lines. A `.txt` export has the
//! title first, section headings ("Verse 1", "Chorus") and a footer starting
//! at `CCLI Song #` with authors, copyright and the licence line.
//!
//! Section labels are normalised to the short forms the rest of Lampstand
//! uses (`V1`, `C`, `P`, `B`, `T`, `I`, `E`). A label that comes up twice is
//! one section sung twice, so the song gets an arrangement.

use std::path::{Path, PathBuf};

use lampstand_core::song::{Section, Song};

use crate::{ImportError, Skipped, SongImport};

pub fn import(path: &Path) -> Result<SongImport, ImportError> {
    let mut out = SongImport::default();
    if path.is_dir() {
        let mut files = Vec::new();
        collect(path, &mut files)?;
        files.sort();
        for file in files {
            import_file(&file, &mut out);
        }
        return Ok(out);
    }
    match extension(path).as_deref() {
        Some("usr" | "txt") => {
            import_file(path, &mut out);
            Ok(out)
        }
        _ => Err(ImportError::WrongFormat),
    }
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), ImportError> {
    for entry in std::fs::read_dir(dir)? {
        let p = entry?.path();
        if p.is_dir() {
            collect(&p, files)?;
        } else if matches!(extension(&p).as_deref(), Some("usr" | "txt")) {
            files.push(p);
        }
    }
    Ok(())
}

fn import_file(path: &Path, out: &mut SongImport) {
    let source = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let result = std::fs::read(path)
        .map_err(|e| e.to_string())
        .map(|b| decode(&b))
        .and_then(|text| {
            if extension(path).as_deref() == Some("usr") {
                parse_usr(&text)
            } else {
                parse_txt(&text)
            }
        });
    match result {
        Ok(song) => out.songs.push(song),
        Err(reason) => out.skipped.push(Skipped { source, reason }),
    }
}

/// UTF-8 (with or without a BOM), else Windows-1252, which older SongSelect
/// downloads use.
fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| cp1252(b)).collect(),
    }
}

fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž',
        '\u{8F}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}',
        'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9F => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

// ---------------------------------------------------------------- .usr

fn parse_usr(text: &str) -> Result<Song, String> {
    let mut ccli = None;
    let (mut title, mut author, mut copyright, mut key) = (None, None, None, None);
    let (mut fields, mut words) = (None, None);
    let mut in_song = false;
    let mut saw_header = false;

    for line in text.lines() {
        let line = line.trim();
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_song = header.starts_with("S ");
            if in_song {
                saw_header = true;
                ccli = header
                    .rsplit(|c: char| !c.is_ascii_digit())
                    .next()
                    .and_then(|d| d.parse::<u32>().ok());
            }
            continue;
        }
        if !in_song {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim();
        match k.trim().to_ascii_lowercase().as_str() {
            "title" => title = Some(v.to_string()),
            "author" => author = Some(v.to_string()),
            "copyright" => copyright = Some(v.to_string()),
            "keys" => key = Some(v.to_string()),
            "fields" => fields = Some(v.to_string()),
            "words" => words = Some(v.to_string()),
            _ => {}
        }
    }

    if !saw_header {
        return Err("no [S A…] song section; not a SongSelect .usr file".into());
    }
    let title = title.filter(|t| !t.is_empty()).ok_or("song has no Title")?;
    let words = words
        .filter(|w| !w.trim().is_empty())
        .ok_or("song has no Words")?;
    let texts: Vec<&str> = words.split("/t").collect();
    let labels: Vec<String> = match &fields {
        Some(f) if !f.trim().is_empty() => f.split("/t").map(|s| s.trim().to_string()).collect(),
        _ => Vec::new(),
    };
    if !labels.is_empty() && labels.len() != texts.len() {
        return Err(format!(
            "Fields lists {} sections but Words has {}",
            labels.len(),
            texts.len()
        ));
    }

    let mut builder = SectionBuilder::default();
    for (i, text) in texts.iter().enumerate() {
        let lines: Vec<String> = text.split("/n").map(|l| l.trim_end().to_string()).collect();
        let lines = trim_blank(lines);
        let label = labels.get(i).map(|l| l.as_str()).unwrap_or("");
        builder.push(label, lines);
    }

    let mut song = builder.finish(Song::new(title));
    song.ccli_number = ccli;
    song.author = author.as_deref().and_then(join_authors);
    song.copyright = copyright.filter(|c| !c.is_empty());
    song.key = key.filter(|k| !k.is_empty());
    Ok(song)
}

// ---------------------------------------------------------------- .txt

fn parse_txt(text: &str) -> Result<Song, String> {
    let lines: Vec<&str> = text.lines().map(|l| l.trim_end()).collect();
    let footer_at = lines
        .iter()
        .position(|l| {
            l.trim_start()
                .to_ascii_lowercase()
                .starts_with("ccli song #")
        })
        .ok_or("no 'CCLI Song #' footer; not a SongSelect lyrics export")?;

    let body = &lines[..footer_at];
    let mut blocks = body
        .split(|l| l.trim().is_empty())
        .filter(|b| !b.is_empty());
    let title_block = blocks.next().ok_or("no title")?;
    let title = title_block[0].trim().to_string();

    let mut builder = SectionBuilder::default();
    // A title block with more than one line has the first section right
    // behind it, with no blank line between.
    let rest_of_title = &title_block[1..];
    let pending: Vec<&[&str]> = if rest_of_title.is_empty() {
        Vec::new()
    } else {
        vec![rest_of_title]
    };
    for block in pending.into_iter().chain(blocks) {
        let first = block[0].trim();
        let (label, rest) = if is_heading(first) {
            (first, &block[1..])
        } else {
            ("", block)
        };
        let lines = trim_blank(rest.iter().map(|l| l.trim().to_string()).collect());
        if lines.is_empty() {
            continue;
        }
        builder.push(label, lines);
    }

    let mut song = builder.finish(Song::new(title));
    if song.sections.is_empty() {
        return Err("no lyrics".into());
    }

    let first_footer = lines[footer_at].trim();
    song.ccli_number = first_footer
        .trim_start_matches(|c: char| !c.is_ascii_digit())
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|d| d.parse().ok());

    let mut authors = None;
    let mut copyright: Vec<&str> = Vec::new();
    for line in lines[footer_at + 1..].iter().map(|l| l.trim()) {
        let lower = line.to_ascii_lowercase();
        if line.is_empty() || lower.starts_with("ccli license") {
            continue;
        }
        if lower.starts_with("for use solely") {
            break;
        }
        if line.starts_with('©') || lower.starts_with("copyright") || !copyright.is_empty() {
            copyright.push(line);
        } else if authors.is_none() {
            authors = join_authors(line);
        }
    }
    song.author = authors;
    if !copyright.is_empty() {
        song.copyright = Some(copyright.join(" "));
    }
    Ok(song)
}

/// "Verse 1", "Chorus", "Pre-Chorus 2", "Bridge"…, but not a lyric line.
pub(crate) fn is_heading(line: &str) -> bool {
    normalise_label(line).is_some()
}

// ---------------------------------------------------------------- shared

/// Short label for a heading, or `None` when the text is not a known one.
pub(crate) fn normalise_label(heading: &str) -> Option<String> {
    let heading = heading.trim().trim_end_matches(':').trim();
    let split = heading
        .find(|c: char| c.is_ascii_digit())
        .unwrap_or(heading.len());
    let (word, num) = heading.split_at(split);
    let num = num.trim();
    if !num.chars().all(|c| c.is_ascii_alphanumeric()) || num.len() > 2 {
        return None;
    }
    let word = word.trim().to_ascii_lowercase().replace(['-', ' '], "");
    let base = match word.as_str() {
        "verse" | "v" => "V",
        "chorus" | "refrain" | "c" => "C",
        "prechorus" | "p" => "P",
        "bridge" | "b" => "B",
        "tag" | "t" => "T",
        "intro" | "introduction" | "i" => "I",
        "ending" | "outro" | "e" => "E",
        "interlude" | "instrumental" => return Some(capitalise(&word) + num),
        _ => return None,
    };
    Some(format!("{base}{num}"))
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
        .unwrap_or_default()
}

fn join_authors(s: &str) -> Option<String> {
    let names: Vec<&str> = s
        .split('|')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .collect();
    (!names.is_empty()).then(|| names.join(", "))
}

pub(crate) fn trim_blank(mut lines: Vec<String>) -> Vec<String> {
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let lead = lines.iter().take_while(|l| l.trim().is_empty()).count();
    lines.drain(..lead);
    lines
}

/// Collects sections in written order. A label seen again with the same text
/// is a repeat; with different text it gets a distinct label. Unlabelled
/// sections are numbered as verses.
#[derive(Default)]
pub(crate) struct SectionBuilder {
    sections: Vec<Section>,
    sung: Vec<String>,
    verses: usize,
}

impl SectionBuilder {
    pub(crate) fn push(&mut self, heading: &str, lines: Vec<String>) {
        let mut label = match normalise_label(heading) {
            Some(l) => l,
            None if heading.trim().is_empty() => String::new(),
            None => heading.trim().to_string(),
        };
        if label.is_empty() {
            self.verses += 1;
            label = format!("V{}", self.verses);
        }
        let mut candidate = label.clone();
        let mut n = 1;
        loop {
            match self.sections.iter().find(|s| s.label == candidate) {
                Some(s) if s.lines == lines => break,
                Some(_) => {
                    n += 1;
                    candidate = format!("{label}.{n}");
                }
                None => {
                    self.sections.push(Section {
                        label: candidate.clone(),
                        lines,
                    });
                    break;
                }
            }
        }
        self.sung.push(candidate);
    }

    /// A heading with no words under it ("Chorus" again) sings the section
    /// it names once more. False when no section has that label yet.
    pub(crate) fn repeat(&mut self, heading: &str) -> bool {
        let Some(label) = normalise_label(heading) else {
            return false;
        };
        if !self.sections.iter().any(|s| s.label == label) {
            return false;
        }
        self.sung.push(label);
        true
    }

    pub(crate) fn finish(self, mut song: Song) -> Song {
        if self.sung.len() != self.sections.len() {
            song.arrangement = self.sung;
        }
        song.sections = self.sections;
        song
    }
}
