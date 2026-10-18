//! The library: songs, services, themes, decks and translations as the app
//! sees them, on top of the sync repo. Adds search, service packages, the
//! readiness check and CCLI usage logging.
//!
//! PRD: SL-1, SL-3, BI-2, CD-8, CD-10, PS-6, RP-1.

pub mod package;
pub mod readiness;
pub mod search;
pub mod usage;

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard};

use lampstand_core::deck::{Deck, DeckId};
use lampstand_core::geom::{Color, Rect};
use lampstand_core::id::BlobHash;
use lampstand_core::scene::{HAlign, Transition, VAlign};
use lampstand_core::scripture::{BibleText, Translation, TranslationId};
use lampstand_core::service::{Service, ServiceId};
use lampstand_core::song::{Song, SongId};
use lampstand_core::theme::{Background, TextStyle, Theme, ThemeId};
use lampstand_sync::{Doc, DocChanged, DocKind, Repo, SyncError};
use uuid::Uuid;

use crate::search::{Hit, SongIndex};

/// zstd level for Bible blobs: they are written once and read often.
const BIBLE_ZSTD_LEVEL: i32 = 19;

/// The starter theme has a fixed ID, so two devices that each create it on
/// first run end up with one document, not two.
const STARTER_THEME_ID: Uuid = Uuid::from_u128(0x0192_0000_0000_7000_8000_6c61_6d70_7374);

/// Encodes Bible text as the bytes of a translation blob: postcard, then
/// zstd. Store the result with `repo().blobs().put` and point
/// `Translation::text` at the hash.
pub fn encode_bible_text(text: &BibleText) -> Result<Vec<u8>, SyncError> {
    let raw = postcard::to_stdvec(text)
        .map_err(|e| SyncError::Corrupt(format!("encoding Bible text: {e}")))?;
    zstd::encode_all(raw.as_slice(), BIBLE_ZSTD_LEVEL).map_err(SyncError::Io)
}

/// The inverse of [`encode_bible_text`].
pub fn decode_bible_text(bytes: &[u8]) -> Result<BibleText, SyncError> {
    let raw = zstd::decode_all(bytes)
        .map_err(|e| SyncError::Corrupt(format!("Bible text is not valid zstd: {e}")))?;
    postcard::from_bytes(&raw)
        .map_err(|e| SyncError::Corrupt(format!("Bible text is not valid postcard: {e}")))
}

/// Songs are searched through an index that is brought up to date from the
/// repo's change feed whenever it is used.
struct Search {
    changes: Receiver<DocChanged>,
    index: SongIndex,
}

pub struct Library {
    repo: Repo,
    search: Mutex<Search>,
    /// Decoded Bible text by blob hash. Blobs never change, so an entry
    /// is never stale.
    bibles: Mutex<HashMap<BlobHash, Arc<BibleText>>>,
}

impl Library {
    /// Opens the library under `dir`. On first run, creates the starter
    /// theme.
    pub fn open(
        dir: &Path,
        device: lampstand_sync::team::DeviceKeypair,
    ) -> Result<Self, SyncError> {
        let repo = Repo::open(dir.join("repo"), device)?;
        // Subscribe before reading, so a change between the two is seen
        // (applying one twice is harmless).
        let changes = repo.subscribe();
        let mut index = SongIndex::new();
        for song in all::<Song>(&repo)? {
            index.upsert(&song);
        }
        let library = Self {
            repo,
            search: Mutex::new(Search { changes, index }),
            bibles: Mutex::new(HashMap::new()),
        };
        library.ensure_starter_theme()?;
        Ok(library)
    }

    pub fn repo(&self) -> &Repo {
        &self.repo
    }

    fn ensure_starter_theme(&self) -> Result<(), SyncError> {
        if self.repo.list(DocKind::Theme).is_empty() {
            self.repo.put(&starter_theme())?;
        }
        Ok(())
    }

    pub fn song(&self, id: SongId) -> Result<Option<Song>, SyncError> {
        self.repo.get(id.uuid())
    }

    pub fn songs(&self) -> Result<Vec<Song>, SyncError> {
        all(&self.repo)
    }

    pub fn put_song(&self, song: &Song) -> Result<(), SyncError> {
        self.repo.put(song)
    }

    pub fn delete_song(&self, id: SongId) -> Result<(), SyncError> {
        self.repo.delete(DocKind::Song, id.uuid())
    }

    /// Searches song titles and lyrics (SL-3).
    pub fn search_songs(&self, query: &str, limit: usize) -> Result<Vec<Hit>, SyncError> {
        let search = self.refreshed_search()?;
        Ok(search.index.search(query, limit))
    }

    /// Applies the song changes seen since the index was last used.
    fn refreshed_search(&self) -> Result<MutexGuard<'_, Search>, SyncError> {
        let mut search = self.search.lock().unwrap_or_else(|e| e.into_inner());
        while let Ok(change) = search.changes.try_recv() {
            if change.kind != DocKind::Song {
                continue;
            }
            let id = SongId::from_uuid(change.id);
            match self.repo.get::<Song>(change.id)? {
                Some(song) => search.index.upsert(&song),
                None => search.index.remove(id),
            }
        }
        Ok(search)
    }

    pub fn service(&self, id: ServiceId) -> Result<Option<Service>, SyncError> {
        self.repo.get(id.uuid())
    }

    pub fn services(&self) -> Result<Vec<Service>, SyncError> {
        all(&self.repo)
    }

    pub fn put_service(&self, service: &Service) -> Result<(), SyncError> {
        self.repo.put(service)
    }

    pub fn delete_service(&self, id: ServiceId) -> Result<(), SyncError> {
        self.repo.delete(DocKind::Service, id.uuid())
    }

    pub fn theme(&self, id: ThemeId) -> Result<Option<Theme>, SyncError> {
        self.repo.get(id.uuid())
    }

    pub fn themes(&self) -> Result<Vec<Theme>, SyncError> {
        all(&self.repo)
    }

    pub fn put_theme(&self, theme: &Theme) -> Result<(), SyncError> {
        self.repo.put(theme)
    }

    pub fn delete_theme(&self, id: ThemeId) -> Result<(), SyncError> {
        self.repo.delete(DocKind::Theme, id.uuid())
    }

    pub fn deck(&self, id: DeckId) -> Result<Option<Deck>, SyncError> {
        self.repo.get(id.uuid())
    }

    pub fn decks(&self) -> Result<Vec<Deck>, SyncError> {
        all(&self.repo)
    }

    pub fn put_deck(&self, deck: &Deck) -> Result<(), SyncError> {
        self.repo.put(deck)
    }

    pub fn delete_deck(&self, id: DeckId) -> Result<(), SyncError> {
        self.repo.delete(DocKind::Deck, id.uuid())
    }

    pub fn translation(&self, id: TranslationId) -> Result<Option<Translation>, SyncError> {
        self.repo.get(id.uuid())
    }

    pub fn translations(&self) -> Result<Vec<Translation>, SyncError> {
        all(&self.repo)
    }

    pub fn put_translation(&self, translation: &Translation) -> Result<(), SyncError> {
        self.repo.put(translation)
    }

    pub fn delete_translation(&self, id: TranslationId) -> Result<(), SyncError> {
        self.repo.delete(DocKind::Translation, id.uuid())
    }

    /// Decoded Bible text, cached in memory after the first call. `None`
    /// when the translation is unknown, or its blob has not reached this
    /// device yet.
    pub fn bible_text(&self, id: TranslationId) -> Result<Option<Arc<BibleText>>, SyncError> {
        let Some(translation) = self.translation(id)? else {
            return Ok(None);
        };
        let hash = translation.text;
        if let Some(text) = self
            .bibles
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&hash)
        {
            return Ok(Some(text.clone()));
        }
        let Some(path) = self.repo.blobs().path(&hash) else {
            return Ok(None);
        };
        let text = Arc::new(decode_bible_text(&std::fs::read(path)?)?);
        self.bibles
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(hash, text.clone());
        Ok(Some(text))
    }

    pub fn subscribe(&self) -> Receiver<DocChanged> {
        self.repo.subscribe()
    }
}

/// Every live document of a type, in creation order.
fn all<T: Doc>(repo: &Repo) -> Result<Vec<T>, SyncError> {
    let mut out = Vec::new();
    for id in repo.list(T::KIND) {
        if let Some(doc) = repo.get(id)? {
            out.push(doc);
        }
    }
    Ok(out)
}

/// White Noto Sans on black, full screen, four lines a slide.
fn starter_theme() -> Theme {
    Theme {
        id: ThemeId::from_uuid(STARTER_THEME_ID),
        name: "Starter".into(),
        text: TextStyle {
            font_family: "Noto Sans".into(),
            size: 72.0,
            min_size: 36.0,
            weight: 400,
            italic: false,
            color: Color::WHITE,
            align: HAlign::Center,
            valign: VAlign::Middle,
            line_spacing: 1.2,
            uppercase: false,
            shadow: None,
            outline: None,
        },
        background: Background::Color(Color::BLACK),
        text_area: Rect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        },
        max_lines: 4,
        transition: Transition::CUT,
        show_copyright: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lampstand_core::deck::{DeckSource, Version};
    use lampstand_core::id::Timestamp;
    use lampstand_core::scripture::Licence;
    use lampstand_core::song::Section;
    use lampstand_core::theme::REFERENCE_HEIGHT;
    use lampstand_sync::team::DeviceKeypair;

    fn open(dir: &Path) -> Library {
        Library::open(dir, DeviceKeypair::generate()).unwrap()
    }

    fn song(title: &str, line: &str) -> Song {
        let mut song = Song::new(title);
        song.sections.push(Section {
            label: "Verse 1".into(),
            lines: vec![line.into()],
        });
        song
    }

    fn bible() -> BibleText {
        BibleText {
            books: vec![
                vec![vec!["In the beginning".into(), "And the earth".into()]],
                vec![],
            ],
        }
    }

    #[test]
    fn songs_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let lib = open(dir.path());
        let s = song("Amazing Grace", "Amazing grace how sweet");
        lib.put_song(&s).unwrap();
        assert_eq!(lib.song(s.id).unwrap(), Some(s.clone()));
        assert_eq!(lib.songs().unwrap(), vec![s.clone()]);
        lib.delete_song(s.id).unwrap();
        assert_eq!(lib.song(s.id).unwrap(), None);
        assert!(lib.songs().unwrap().is_empty());
    }

    #[test]
    fn services_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let lib = open(dir.path());
        let mut s = Service::new("Sunday");
        s.theme = Some(lib.themes().unwrap()[0].id);
        lib.put_service(&s).unwrap();
        assert_eq!(lib.service(s.id).unwrap(), Some(s.clone()));
        assert_eq!(lib.services().unwrap(), vec![s]);
    }

    #[test]
    fn themes_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let lib = open(dir.path());
        let mut t = starter_theme();
        t.id = ThemeId::generate();
        t.name = "Lower third".into();
        t.max_lines = 2;
        lib.put_theme(&t).unwrap();
        assert_eq!(lib.theme(t.id).unwrap(), Some(t.clone()));
        assert_eq!(lib.themes().unwrap().len(), 2);
        lib.delete_theme(t.id).unwrap();
        assert_eq!(lib.theme(t.id).unwrap(), None);
    }

    #[test]
    fn decks_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let lib = open(dir.path());
        let version = |n: u8| Version {
            blob: BlobHash::of(&[n]),
            fetched_at: Timestamp(1_700_000_000 + i64::from(n)),
            source_version: Some(format!("v{n}")),
        };
        let deck = Deck {
            id: DeckId::generate(),
            title: "Announcements".into(),
            source: DeckSource::File {
                name: "ann.pptx".into(),
            },
            current: version(1),
            pending: Some(version(2)),
        };
        lib.put_deck(&deck).unwrap();
        assert_eq!(lib.deck(deck.id).unwrap(), Some(deck.clone()));
        assert_eq!(lib.decks().unwrap(), vec![deck]);
    }

    #[test]
    fn translations_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let lib = open(dir.path());
        let t = Translation {
            id: TranslationId::generate(),
            name: "King James".into(),
            abbreviation: "KJV".into(),
            language: "en".into(),
            licence: Licence::Licensed {
                notice: Some("notice".into()),
            },
            text: BlobHash::of(b"x"),
        };
        lib.put_translation(&t).unwrap();
        assert_eq!(lib.translation(t.id).unwrap(), Some(t.clone()));
        assert_eq!(lib.translations().unwrap(), vec![t]);
    }

    #[test]
    fn bible_encode_store_decode() {
        let text = bible();
        let bytes = encode_bible_text(&text).unwrap();
        assert_eq!(decode_bible_text(&bytes).unwrap(), text);
        assert!(matches!(
            decode_bible_text(b"not a bible"),
            Err(SyncError::Corrupt(_))
        ));

        let dir = tempfile::tempdir().unwrap();
        let lib = open(dir.path());
        let hash = lib.repo().blobs().put(&bytes).unwrap();
        let t = Translation {
            id: TranslationId::generate(),
            name: "Test".into(),
            abbreviation: "TST".into(),
            language: "en".into(),
            licence: Licence::PublicDomain,
            text: hash,
        };
        lib.put_translation(&t).unwrap();
        let first = lib.bible_text(t.id).unwrap().unwrap();
        assert_eq!(*first, text);
        // The second call is served from memory: the same allocation.
        let second = lib.bible_text(t.id).unwrap().unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(lib.bible_text(TranslationId::generate()).unwrap(), None);
    }

    #[test]
    fn bible_blob_not_yet_synced_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let lib = open(dir.path());
        let t = Translation {
            id: TranslationId::generate(),
            name: "Absent".into(),
            abbreviation: "ABS".into(),
            language: "en".into(),
            licence: Licence::PublicDomain,
            text: BlobHash::of(b"never stored"),
        };
        lib.put_translation(&t).unwrap();
        assert_eq!(lib.bible_text(t.id).unwrap(), None);
    }

    #[test]
    fn search_follows_puts_and_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let lib = open(dir.path());
        let mut s = song("Amazing Grace", "how sweet the sound");
        lib.put_song(&s).unwrap();
        assert_eq!(lib.search_songs("amazing", 5).unwrap()[0].song, s.id);

        s.title = "Great Is Thy Faithfulness".into();
        lib.put_song(&s).unwrap();
        assert!(lib.search_songs("amazing", 5).unwrap().is_empty());
        assert_eq!(lib.search_songs("faithfulness", 5).unwrap()[0].song, s.id);

        lib.delete_song(s.id).unwrap();
        assert!(lib.search_songs("faithfulness", 5).unwrap().is_empty());

        // A later put of the same song brings it back.
        lib.put_song(&s).unwrap();
        assert_eq!(lib.search_songs("faithfulness", 5).unwrap().len(), 1);
    }

    #[test]
    fn search_is_built_from_stored_songs_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let s = song("Be Thou My Vision", "high king of heaven");
        {
            let lib = open(dir.path());
            lib.put_song(&s).unwrap();
        }
        let lib = open(dir.path());
        assert_eq!(lib.search_songs("vision", 5).unwrap()[0].song, s.id);
    }

    #[test]
    fn starter_theme_exists_exactly_once_across_reopens() {
        let dir = tempfile::tempdir().unwrap();
        for _ in 0..3 {
            let lib = open(dir.path());
            let themes = lib.themes().unwrap();
            assert_eq!(themes.len(), 1);
            let t = &themes[0];
            assert_eq!(t.text.font_family, "Noto Sans");
            assert_eq!(t.text.color, Color::WHITE);
            assert_eq!(t.background, Background::Color(Color::BLACK));
            assert_eq!(t.max_lines, 4);
            assert!(t.show_copyright);
            assert_eq!(
                t.text_area,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0
                }
            );
            assert!(t.text.size <= REFERENCE_HEIGHT);
        }
    }

    #[test]
    fn starter_theme_is_not_recreated_beside_other_themes() {
        let dir = tempfile::tempdir().unwrap();
        let lib = open(dir.path());
        lib.delete_theme(ThemeId::from_uuid(STARTER_THEME_ID))
            .unwrap();
        let mut t = starter_theme();
        t.id = ThemeId::generate();
        lib.put_theme(&t).unwrap();
        drop(lib);
        assert_eq!(open(dir.path()).themes().unwrap().len(), 1);
    }
}
