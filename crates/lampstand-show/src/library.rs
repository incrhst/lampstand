//! The library on disk: songs, Bible translations, decks and the church logo.
//!
//! Layout of the data directory (see `docs/v0.1.md`):
//!
//! - `songs/<id>.json`: one [`Song`] each.
//! - `bibles/<file>`: the files as imported; parsed again at start-up, since
//!   parsing a whole Bible takes well under a second.
//! - `decks/<id>.pptx`, with the original file name in `decks/<id>.name`.
//! - `logo.png` (or `.jpg`).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lampstand_core::song::Song;
use lampstand_scripture::Translation;
use lampstand_slides::Deck;
use lampstand_songs::search::SongIndex;

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("could not read the songs: {0}")]
    Songs(String),
    #[error("could not read the Bible: {0}")]
    Bible(String),
    #[error("{0}")]
    Deck(#[from] lampstand_slides::DeckError),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
}

pub struct DeckEntry {
    pub name: String,
    pub path: PathBuf,
    /// `Err` holds why the file could not be opened, so the service can say so.
    pub deck: Result<Arc<Deck>, String>,
}

/// What an import added, and anything it could not carry over.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct ImportSummary {
    pub added: Vec<String>,
    pub notes: Vec<String>,
}

pub struct Library {
    dir: PathBuf,
    songs: BTreeMap<String, Song>,
    index: SongIndex<String>,
    bibles: BTreeMap<String, Arc<Translation>>,
    decks: BTreeMap<String, DeckEntry>,
}

impl Library {
    /// Open (and create if needed) the library in `dir`. Files that fail to
    /// load are skipped and returned as warnings rather than failing start-up.
    pub fn open(dir: &Path) -> Result<(Library, Vec<String>), LibraryError> {
        for sub in ["songs", "bibles", "decks", "services"] {
            fs::create_dir_all(dir.join(sub))?;
        }
        let mut lib = Library {
            dir: dir.to_path_buf(),
            songs: BTreeMap::new(),
            index: SongIndex::new(),
            bibles: BTreeMap::new(),
            decks: BTreeMap::new(),
        };
        let mut warnings = Vec::new();
        for path in sorted_files(&dir.join("songs"))? {
            match fs::read_to_string(&path).map_err(LibraryError::from).and_then(|t| Ok(serde_json::from_str::<Song>(&t)?)) {
                Ok(song) => {
                    let id = stem(&path);
                    lib.index.insert(id.clone(), &song);
                    lib.songs.insert(id, song);
                }
                Err(e) => warnings.push(format!("{}: {e}", path.display())),
            }
        }
        for path in sorted_files(&dir.join("bibles"))? {
            match fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| parse_bible(&t)) {
                Ok(t) => lib.add_bible(t),
                Err(e) => warnings.push(format!("{}: {e}", path.display())),
            }
        }
        for path in sorted_files(&dir.join("decks"))? {
            if path.extension().and_then(|e| e.to_str()) != Some("pptx") {
                continue;
            }
            let id = stem(&path);
            let name = fs::read_to_string(path.with_extension("name")).unwrap_or_else(|_| id.clone());
            let deck = Deck::open(&path).map(Arc::new).map_err(|e| e.to_string());
            if let Err(e) = &deck {
                warnings.push(format!("{name}: {e}"));
            }
            lib.decks.insert(id, DeckEntry { name: name.trim().to_string(), path, deck });
        }
        Ok((lib, warnings))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn song(&self, id: &str) -> Option<&Song> {
        self.songs.get(id)
    }

    pub fn songs(&self) -> impl Iterator<Item = (&String, &Song)> {
        self.songs.iter()
    }

    /// Ranked song ids for a typed query; every song, by title, when the query is empty.
    pub fn search_songs(&self, query: &str, limit: usize) -> Vec<String> {
        if query.trim().is_empty() {
            let mut all: Vec<_> = self.songs.iter().collect();
            all.sort_by_key(|(_, s)| s.title.to_lowercase());
            return all.into_iter().take(limit).map(|(id, _)| id.clone()).collect();
        }
        self.index.search(query, limit).into_iter().map(|h| h.key).collect()
    }

    /// Import song files (OpenLyrics, OpenSong or ChordPro, detected from content).
    pub fn import_songs(&mut self, files: &[(String, Vec<u8>)]) -> Result<ImportSummary, LibraryError> {
        let mut summary = ImportSummary::default();
        for (name, bytes) in files {
            let text = String::from_utf8_lossy(bytes);
            let Some(format) = lampstand_songs::detect(&text) else {
                summary.notes.push(format!("{name}: not a song file Lampstand recognises"));
                continue;
            };
            let imported = lampstand_songs::import_str(&text, format).map_err(|e| LibraryError::Songs(format!("{name}: {e}")))?;
            for note in &imported.report.notes {
                summary.notes.push(format!("{name}: {}", note.message));
            }
            for song in imported.songs {
                let id = song_id(&song);
                fs::write(self.dir.join("songs").join(format!("{id}.json")), serde_json::to_string_pretty(&song)?)?;
                self.index.remove(&id);
                self.index.insert(id.clone(), &song);
                summary.added.push(song.title.clone());
                self.songs.insert(id, song);
            }
        }
        Ok(summary)
    }

    pub fn bible(&self, abbreviation: &str) -> Option<&Arc<Translation>> {
        self.bibles.get(abbreviation).or_else(|| {
            self.bibles.iter().find(|(k, _)| k.eq_ignore_ascii_case(abbreviation)).map(|(_, v)| v)
        })
    }

    /// Translation abbreviations, in order.
    pub fn bibles(&self) -> impl Iterator<Item = &Arc<Translation>> {
        self.bibles.values()
    }

    /// Import one Bible file. `abbreviation` names translations whose file carries none (USFM).
    pub fn import_bible(&mut self, file_name: &str, bytes: &[u8], abbreviation: Option<&str>) -> Result<ImportSummary, LibraryError> {
        let text = String::from_utf8_lossy(bytes).into_owned();
        let mut t = parse_bible(&text).map_err(LibraryError::Bible)?;
        if let Some(a) = abbreviation.filter(|a| !a.trim().is_empty()) {
            t.meta.abbreviation = a.trim().to_string();
        }
        if t.meta.abbreviation.is_empty() {
            t.meta.abbreviation = Path::new(file_name).file_stem().and_then(|s| s.to_str()).unwrap_or("BIBLE").to_uppercase();
        }
        // Keep the source. A USFM file carries no abbreviation, so it is stored with
        // ours on its first line. Files of one translation (often one per book) merge.
        let safe: String = t.meta.abbreviation.chars().filter(|c| c.is_alphanumeric()).collect();
        let mut stored = text;
        if lampstand_scripture::import::detect(&stored) == Some(lampstand_scripture::import::Format::Usfm) {
            stored = format!("\\id_lampstand {}\n{stored}", t.meta.abbreviation);
        }
        let name = format!("{safe}-{:016x}.txt", fnv(stored.as_bytes()));
        fs::write(self.dir.join("bibles").join(name), stored)?;
        let added = t.verse_count();
        let abbr = t.meta.abbreviation.clone();
        self.add_bible(t);
        let total = self.bibles[&abbr].verse_count();
        Ok(ImportSummary { added: vec![format!("{abbr}: {added} verses ({total} in all)")], notes: Vec::new() })
    }

    /// Add a translation, merging it into one already loaded under the same abbreviation.
    fn add_bible(&mut self, t: Translation) {
        let Some(existing) = self.bibles.get_mut(&t.meta.abbreviation) else {
            self.bibles.insert(t.meta.abbreviation.clone(), Arc::new(t));
            return;
        };
        let merged = Arc::make_mut(existing);
        if merged.meta.name.is_empty() {
            merged.meta.name = t.meta.name.clone();
        }
        if merged.meta.copyright.is_none() {
            merged.meta.copyright = t.meta.copyright.clone();
        }
        for book in t.books().collect::<Vec<_>>() {
            for chapter in 1..=MAX_CHAPTERS {
                let Some(last) = t.last_verse(book, chapter) else { continue };
                for verse in 1..=last {
                    if let Some(text) = t.verse(book, chapter, verse) {
                        merged.insert(book, chapter, verse, text.to_string());
                    }
                }
            }
        }
    }

    pub fn deck(&self, id: &str) -> Option<&DeckEntry> {
        self.decks.get(id)
    }

    pub fn decks(&self) -> impl Iterator<Item = (&String, &DeckEntry)> {
        self.decks.iter()
    }

    /// Store a `.pptx` and open it. Returns the deck id even when it can't be read,
    /// so the operator sees the problem on the service item.
    pub fn import_deck(&mut self, file_name: &str, bytes: &[u8]) -> Result<String, LibraryError> {
        let id = format!("{:016x}", fnv(bytes));
        let path = self.dir.join("decks").join(format!("{id}.pptx"));
        fs::write(&path, bytes)?;
        let name = Path::new(file_name).file_stem().and_then(|s| s.to_str()).unwrap_or(file_name).to_string();
        fs::write(path.with_extension("name"), &name)?;
        let deck = Deck::from_bytes(bytes).map(Arc::new).map_err(|e| e.to_string());
        self.decks.insert(id.clone(), DeckEntry { name, path, deck });
        Ok(id)
    }

    pub fn logo_path(&self) -> Option<PathBuf> {
        ["logo.png", "logo.jpg"].iter().map(|n| self.dir.join(n)).find(|p| p.exists())
    }

    pub fn set_logo(&mut self, bytes: &[u8], jpeg: bool) -> Result<(), LibraryError> {
        for n in ["logo.png", "logo.jpg"] {
            let _ = fs::remove_file(self.dir.join(n));
        }
        fs::write(self.dir.join(if jpeg { "logo.jpg" } else { "logo.png" }), bytes)?;
        Ok(())
    }
}

/// Psalms has the most chapters, 150.
const MAX_CHAPTERS: u16 = 150;

fn parse_bible(text: &str) -> Result<Translation, String> {
    // A stored USFM file starts with our abbreviation line.
    if let Some(rest) = text.strip_prefix("\\id_lampstand ") {
        let (abbr, body) = rest.split_once('\n').unwrap_or((rest, ""));
        let mut imp = lampstand_scripture::import::import(body).map_err(|e| e.to_string())?;
        imp.translation.meta.abbreviation = abbr.trim().to_string();
        return Ok(imp.translation);
    }
    lampstand_scripture::import::import(text).map(|i| i.translation).map_err(|e| e.to_string())
}

/// A stable id from the title and words, so importing the same song twice replaces it.
fn song_id(song: &Song) -> String {
    let key = format!("{}\n{}", song.title.to_lowercase(), song.lyrics_text().to_lowercase());
    format!("{:016x}", fnv(key.as_bytes()))
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn stem(path: &Path) -> String {
    path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string()
}

fn sorted_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)?.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_file()).collect();
    v.sort();
    Ok(v)
}
