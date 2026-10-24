//! The library: songs, services, themes, decks and translations as the app
//! sees them, on top of the sync repo. Adds search, service packages, the
//! readiness check and CCLI usage logging.
//!
//! PRD: SL-1, SL-3, BI-2, CD-8, CD-10, PS-6, RP-1.

pub mod brand_package;
pub mod bundled;
pub mod package;
pub mod readiness;
pub mod search;
pub mod usage;

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard};

use lampstand_core::brand::Brand;
use lampstand_core::church::{ChurchSettings, CHURCH_SETTINGS_ID};
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
/// The starter theme every library has from first run. Songs and
/// scripture use it when neither the service nor the output names a theme.
pub fn starter_theme_id() -> ThemeId {
    ThemeId::from_uuid(STARTER_THEME_ID)
}

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
    repo: Arc<Repo>,
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
            repo: Arc::new(repo),
            search: Mutex::new(Search { changes, index }),
            bibles: Mutex::new(HashMap::new()),
        };
        library.ensure_starter_theme()?;
        Ok(library)
    }

    pub fn repo(&self) -> &Repo {
        &self.repo
    }

    /// The repo, for the sync node, which outlives any one borrow.
    pub fn shared_repo(&self) -> Arc<Repo> {
        self.repo.clone()
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

    /// Deletes a theme the way the Themes page does: never the last one,
    /// and never the Starter theme, which is what everything falls back to
    /// (use [`Library::delete_theme`] for a delete that arrives by sync).
    pub fn remove_theme(&self, id: ThemeId) -> Result<(), ThemeDeleteError> {
        if id == starter_theme_id() {
            return Err(ThemeDeleteError::Starter);
        }
        if self.repo.list(DocKind::Theme).len() <= 1 {
            return Err(ThemeDeleteError::Last);
        }
        self.repo.delete(DocKind::Theme, id.uuid())?;
        Ok(())
    }

    /// Puts the Starter theme back as it was made, even if it was edited
    /// or a synced delete removed it.
    pub fn restore_starter_theme(&self) -> Result<Theme, SyncError> {
        let theme = starter_theme();
        self.repo.put(&theme)?;
        Ok(theme)
    }

    /// The theme to draw with: the first of `candidates` (most specific
    /// first: the output's, the service's, the device default) that still
    /// exists, else the Starter theme, else any theme, else the built-in
    /// starter, so that a delete on another device never takes an output
    /// down.
    pub fn resolve_theme(&self, candidates: &[Option<ThemeId>]) -> Theme {
        candidates
            .iter()
            .flatten()
            .find_map(|id| self.theme(*id).ok().flatten())
            .or_else(|| self.theme(starter_theme_id()).ok().flatten())
            .or_else(|| self.themes().ok()?.into_iter().next())
            .unwrap_or_else(starter_theme)
    }

    /// The church's settings (look, default theme, idle screen) that sync
    /// to its other devices; all unset before anything is saved.
    pub fn church_settings(&self) -> Result<ChurchSettings, SyncError> {
        Ok(self
            .repo
            .get::<ChurchSettings>(CHURCH_SETTINGS_ID)?
            .unwrap_or_default())
    }

    /// Saves the church's settings. Only the fields that changed become
    /// edits, so another device's edit to a different field is kept.
    pub fn put_church_settings(&self, settings: &ChurchSettings) -> Result<(), SyncError> {
        self.repo.put(settings)
    }

    /// Takes a deleted theme out of the church settings that name it, so
    /// the default falls back to the Starter theme on every device.
    pub fn forget_theme_in_church_settings(&self, id: ThemeId) -> Result<(), SyncError> {
        let mut settings = self.church_settings()?;
        if settings.default_theme == Some(id) || settings.brand_theme == Some(id) {
            if settings.default_theme == Some(id) {
                settings.default_theme = None;
            }
            if settings.brand_theme == Some(id) {
                settings.brand_theme = None;
            }
            self.put_church_settings(&settings)?;
        }
        Ok(())
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

/// Why a theme could not be deleted from the Themes page.
#[derive(Debug)]
pub enum ThemeDeleteError {
    /// The Starter theme is what everything falls back to.
    Starter,
    /// There must always be a theme.
    Last,
    Sync(SyncError),
}

impl From<SyncError> for ThemeDeleteError {
    fn from(e: SyncError) -> Self {
        Self::Sync(e)
    }
}

impl std::fmt::Display for ThemeDeleteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Starter => write!(
                f,
                "The Starter theme can't be deleted: it is what every screen falls back to. Duplicate it to make your own."
            ),
            Self::Last => write!(f, "This is the only theme, so it can't be deleted."),
            Self::Sync(e) => e.fmt(f),
        }
    }
}

/// A theme made from a church's brand: the Starter theme with the brand's
/// colours and font. Lyrics are readable on the brand colour (black or
/// white unless the church chose a text colour).
pub fn theme_from_brand(brand: &Brand, id: ThemeId) -> Theme {
    let mut theme = starter_theme();
    theme.id = id;
    theme.name = if brand.name.trim().is_empty() {
        "Our church".into()
    } else {
        brand.name.trim().into()
    };
    theme.background = Background::Color(brand.primary);
    theme.text.color = brand.text_color();
    if !brand.font_family.trim().is_empty() {
        theme.text.font_family = brand.font_family.trim().into();
    }
    theme
}

/// White Noto Sans on black, full screen, four lines a slide. A fresh
/// library gets it as a document; the app also falls back to it when every
/// theme has been deleted (deletes sync, so that can happen mid-service).
pub fn starter_theme() -> Theme {
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

    fn another_theme(lib: &Library, name: &str) -> Theme {
        let mut t = starter_theme();
        t.id = ThemeId::generate();
        t.name = name.into();
        lib.put_theme(&t).unwrap();
        t
    }

    #[test]
    fn the_starter_and_the_last_theme_cannot_be_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let lib = open(dir.path());
        assert!(matches!(
            lib.remove_theme(starter_theme_id()),
            Err(ThemeDeleteError::Starter)
        ));
        let mine = another_theme(&lib, "Mine");
        lib.remove_theme(mine.id).unwrap();
        assert_eq!(lib.themes().unwrap().len(), 1);
        // Only the starter is left, and a synced delete had removed it
        // from beside another theme: the one that is left stays.
        let other = another_theme(&lib, "Other");
        lib.delete_theme(starter_theme_id()).unwrap();
        assert!(matches!(
            lib.remove_theme(other.id),
            Err(ThemeDeleteError::Last)
        ));
        assert_eq!(lib.themes().unwrap().len(), 1);
    }

    #[test]
    fn the_starter_theme_can_be_restored() {
        let dir = tempfile::tempdir().unwrap();
        let lib = open(dir.path());
        let mut edited = lib.theme(starter_theme_id()).unwrap().unwrap();
        edited.text.size = 20.0;
        lib.put_theme(&edited).unwrap();
        lib.restore_starter_theme().unwrap();
        assert_eq!(
            lib.theme(starter_theme_id()).unwrap(),
            Some(starter_theme())
        );
        another_theme(&lib, "Other");
        lib.delete_theme(starter_theme_id()).unwrap();
        lib.restore_starter_theme().unwrap();
        assert!(lib.theme(starter_theme_id()).unwrap().is_some());
    }

    #[test]
    fn the_most_specific_theme_that_exists_wins() {
        let dir = tempfile::tempdir().unwrap();
        let lib = open(dir.path());
        let output = another_theme(&lib, "Output");
        let service = another_theme(&lib, "Service");
        let default = another_theme(&lib, "Default");
        let pick = |c: &[Option<ThemeId>]| lib.resolve_theme(c).name;
        let all = [Some(output.id), Some(service.id), Some(default.id)];
        assert_eq!(pick(&all), "Output");
        assert_eq!(pick(&[None, Some(service.id), Some(default.id)]), "Service");
        assert_eq!(pick(&[None, None, Some(default.id)]), "Default");
        assert_eq!(pick(&[None, None, None]), "Starter");
        // A theme deleted on another device falls through to the next.
        lib.delete_theme(output.id).unwrap();
        assert_eq!(pick(&all), "Service");
    }

    #[test]
    fn with_every_theme_gone_the_built_in_starter_is_used() {
        let dir = tempfile::tempdir().unwrap();
        let lib = open(dir.path());
        lib.delete_theme(starter_theme_id()).unwrap();
        assert_eq!(
            lib.resolve_theme(&[Some(ThemeId::generate())]),
            starter_theme()
        );
    }

    #[test]
    fn a_brand_makes_a_theme_in_its_colours_and_font() {
        use lampstand_core::brand::contrast;
        let brand = Brand {
            name: " Grace Chapel ".into(),
            logo: None,
            primary: Color::rgb(20, 30, 90),
            text: None,
            accent: Color::rgb(240, 190, 60),
            font_family: "Noto Serif".into(),
            bug: Default::default(),
        };
        let id = ThemeId::generate();
        let t = theme_from_brand(&brand, id);
        assert_eq!((t.id, t.name.as_str()), (id, "Grace Chapel"));
        assert_eq!(t.background, Background::Color(brand.primary));
        assert_eq!(t.text.font_family, "Noto Serif");
        assert!(contrast(t.text.color, brand.primary) > 7.0);
        let plain = Brand {
            font_family: "".into(),
            name: "".into(),
            text: Some(Color::rgb(255, 230, 0)),
            ..brand
        };
        let t = theme_from_brand(&plain, id);
        assert_eq!(t.text.font_family, starter_theme().text.font_family);
        assert_eq!(t.text.color, Color::rgb(255, 230, 0));
        assert_eq!(t.name, "Our church");
    }

    /// Passes sync messages both ways until neither library has more to say.
    fn sync(a: &Library, b: &Library) {
        for _ in 0..20 {
            let mut ids: Vec<Uuid> = a.repo().documents().into_iter().map(|(_, id)| id).collect();
            ids.extend(b.repo().documents().into_iter().map(|(_, id)| id));
            ids.sort();
            ids.dedup();
            let mut quiet = true;
            for id in ids {
                if let Some(m) = a.repo().generate_sync_message(b.repo().device(), id) {
                    b.repo().receive_sync_message(a.repo().device(), m).unwrap();
                    quiet = false;
                }
                if let Some(m) = b.repo().generate_sync_message(a.repo().device(), id) {
                    a.repo().receive_sync_message(b.repo().device(), m).unwrap();
                    quiet = false;
                }
            }
            if quiet {
                return;
            }
        }
        panic!("libraries did not converge");
    }

    fn brand(name: &str) -> Brand {
        Brand {
            name: name.into(),
            logo: Some(BlobHash::of(b"logo")),
            primary: Color::rgb(20, 30, 90),
            text: None,
            accent: Color::rgb(240, 190, 60),
            font_family: "Noto Serif".into(),
            bug: Default::default(),
        }
    }

    #[test]
    fn a_brand_and_logo_set_on_one_device_arrive_on_the_other() {
        let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (a, b) = (open(da.path()), open(db.path()));
        let theme = ThemeId::generate();
        let mut s = a.church_settings().unwrap();
        s.set_brand(Some(&brand("Grace Chapel")));
        s.brand_theme = Some(theme);
        s.default_theme = Some(theme);
        s.set_idle(Some(&lampstand_core::output::IdleScreen::Video {
            blob: BlobHash::of(b"loop"),
            name: "Loop.mp4".into(),
        }));
        s.updated_by = Some("Mary's laptop".into());
        a.put_church_settings(&s).unwrap();
        assert!(b.church_settings().unwrap().is_empty());
        sync(&a, &b);
        let got = b.church_settings().unwrap();
        assert_eq!(got, s);
        assert_eq!(got.brand().unwrap().logo, Some(BlobHash::of(b"logo")));
        assert_eq!(got.files().len(), 2);
    }

    #[test]
    fn concurrent_edits_to_different_fields_merge_and_the_same_field_keeps_the_later() {
        let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (a, b) = (open(da.path()), open(db.path()));
        // Both devices create the document before they meet.
        let mut base = ChurchSettings::default();
        base.set_brand(Some(&brand("Grace Chapel")));
        a.put_church_settings(&base).unwrap();
        sync(&a, &b);
        let mut on_a = a.church_settings().unwrap();
        let mut on_b = b.church_settings().unwrap();
        let mut brand_a = on_a.brand().unwrap();
        brand_a.name = "Grace Church".into();
        on_a.set_brand(Some(&brand_a));
        let mut brand_b = on_b.brand().unwrap();
        brand_b.font_family = "Lato".into();
        on_b.set_brand(Some(&brand_b));
        a.put_church_settings(&on_a).unwrap();
        b.put_church_settings(&on_b).unwrap();
        sync(&a, &b);
        let (ma, mb) = (a.church_settings().unwrap(), b.church_settings().unwrap());
        assert_eq!(ma, mb);
        let merged = ma.brand().unwrap();
        assert_eq!(merged.name, "Grace Church");
        assert_eq!(merged.font_family, "Lato");

        // The same field edited on both: one wins, on both devices.
        let mut x = a.church_settings().unwrap();
        x.name = Some("From A".into());
        a.put_church_settings(&x).unwrap();
        let mut y = b.church_settings().unwrap();
        y.name = Some("From B".into());
        b.put_church_settings(&y).unwrap();
        sync(&a, &b);
        let (ma, mb) = (a.church_settings().unwrap(), b.church_settings().unwrap());
        assert_eq!(ma.name, mb.name);
        assert!(matches!(ma.name.as_deref(), Some("From A" | "From B")));
    }

    #[test]
    fn two_devices_that_each_start_the_document_still_merge() {
        let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (a, b) = (open(da.path()), open(db.path()));
        let mut on_a = ChurchSettings::default();
        on_a.set_brand(Some(&brand("Grace Chapel")));
        a.put_church_settings(&on_a).unwrap();
        let theme = ThemeId::generate();
        let on_b = ChurchSettings {
            default_theme: Some(theme),
            ..ChurchSettings::default()
        };
        b.put_church_settings(&on_b).unwrap();
        sync(&a, &b);
        for lib in [&a, &b] {
            let got = lib.church_settings().unwrap();
            assert_eq!(got.brand().unwrap().name, "Grace Chapel");
            assert_eq!(got.default_theme, Some(theme));
        }
    }

    #[test]
    fn a_deleted_theme_falls_back_to_the_starter_on_every_device() {
        let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (a, b) = (open(da.path()), open(db.path()));
        let mut t = starter_theme();
        t.id = ThemeId::generate();
        t.name = "Church".into();
        a.put_theme(&t).unwrap();
        a.put_church_settings(&ChurchSettings {
            default_theme: Some(t.id),
            brand_theme: Some(t.id),
            ..ChurchSettings::default()
        })
        .unwrap();
        sync(&a, &b);
        let on_b = b.church_settings().unwrap();
        assert_eq!(b.resolve_theme(&[on_b.default_theme]).id, t.id);

        a.remove_theme(t.id).unwrap();
        a.forget_theme_in_church_settings(t.id).unwrap();
        sync(&a, &b);
        let on_b = b.church_settings().unwrap();
        assert_eq!((on_b.default_theme, on_b.brand_theme), (None, None));
        assert_eq!(
            b.resolve_theme(&[on_b.default_theme]).id,
            starter_theme_id()
        );
        // Even a setting that still names the theme resolves to the starter.
        assert_eq!(b.resolve_theme(&[Some(t.id)]).id, starter_theme_id());
    }
}
