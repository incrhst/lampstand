//! Training services (#190): services a volunteer learns Lampstand by
//! running in Present, bundled with the app (the app builds them from
//! `training/` at start, see `app/src-tauri/src/training.rs`).
//!
//! They are not documents. They live beside the repo, in memory, so they
//! never sync: nothing is written to the team index, no peer is offered
//! them, and a device on an older Lampstand never hears of them. No new
//! document kind, field or protocol version is needed.
//!
//! Lookups by ID ([`Library::service`], [`Library::deck`],
//! [`Library::song`], and [`Library::translation`] when the library has
//! none of its own) find them, so opening one for live, its slides,
//! thumbnails and notes work as for any service. Listings
//! ([`Library::services`], [`Library::decks`], [`Library::songs`]) leave
//! them out: the app lists training on its own, and the media list, the
//! song search and the planner's library hold only the church's own things.
//!
//! The songs are the library's own copies of the seeded public-domain
//! hymns under fixed IDs, so a training service still plays after the
//! church deletes or edits its own copy, and a CCLI report never counts
//! one ([`Library::counts_for_ccli`]).
//!
//! Practice changes (a verse added with Quick add while one is open for
//! live) are kept in memory until [`Library::reset_training`] or the next
//! start, when each service is as it was bundled again.
//!
//! The decks are bundled; the training videos are not. The website
//! publishes them with a [`VideoManifest`] (`site/training/videos.json`),
//! and the app downloads them into the blob store the first time a
//! training service is opened, so a new training video needs no release. A
//! video item names its video by the manifest's BLAKE3 hash, which is its
//! blob hash.

use std::collections::HashMap;

use lampstand_core::deck::{Deck, DeckId};
use lampstand_core::id::BlobHash;
use lampstand_core::scripture::{Translation, TranslationId};
use lampstand_core::service::{ItemKind, Service, ServiceId};
use lampstand_core::song::{Song, SongId};
use serde::{Deserialize, Serialize};

use crate::Library;

/// Everything the training services use that the library may not have.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrainingSet {
    /// In the order they are taken: Training Session 0 first.
    pub services: Vec<Service>,
    pub decks: Vec<Deck>,
    pub songs: Vec<Song>,
    /// Used only when the library has no translation with the same ID (a
    /// bundled Bible the church deleted).
    pub translations: Vec<Translation>,
}

/// The training videos the website offers: `site/training/videos.json`,
/// written by `cargo xtask training-videos` and read by the app.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoManifest {
    /// [`VideoManifest::VERSION`]. Fields this Lampstand does not know are
    /// ignored, so adding one needs no new version.
    pub version: u32,
    pub videos: Vec<TrainingVideo>,
}

/// One training video, at `<file>` beside the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingVideo {
    /// The file name without `.mp4`: "t1-welcome".
    pub id: String,
    /// What `training/services/*.json` names: "t1-welcome.mp4".
    pub file: String,
    /// In bytes.
    pub size: u64,
    /// The BLAKE3 hash of the file, which is also its blob hash.
    pub blake3: BlobHash,
}

impl VideoManifest {
    pub const VERSION: u32 = 1;

    /// The video a training service names by file.
    pub fn video(&self, file: &str) -> Option<&TrainingVideo> {
        self.videos.iter().find(|v| v.file == file)
    }
}

#[derive(Debug, Default)]
pub(crate) struct Training {
    bundled: TrainingSet,
    /// The services as they stand, with practice changes.
    services: HashMap<ServiceId, Service>,
}

impl Training {
    fn reset(&mut self) {
        self.services = self
            .bundled
            .services
            .iter()
            .map(|s| (s.id, s.clone()))
            .collect();
    }

    pub(crate) fn service(&self, id: ServiceId) -> Option<Service> {
        self.services.get(&id).cloned()
    }

    pub(crate) fn deck(&self, id: DeckId) -> Option<Deck> {
        self.bundled.decks.iter().find(|d| d.id == id).cloned()
    }

    pub(crate) fn song(&self, id: SongId) -> Option<Song> {
        self.bundled.songs.iter().find(|s| s.id == id).cloned()
    }

    pub(crate) fn translation(&self, id: TranslationId) -> Option<Translation> {
        self.bundled
            .translations
            .iter()
            .find(|t| t.id == id)
            .cloned()
    }

    /// Replaces a training service with a changed copy. False when `service`
    /// is not one.
    pub(crate) fn put(&mut self, service: &Service) -> bool {
        match self.services.get_mut(&service.id) {
            Some(held) => {
                *held = service.clone();
                true
            }
            None => false,
        }
    }

    pub(crate) fn has_service(&self, id: ServiceId) -> bool {
        self.services.contains_key(&id)
    }

    fn replace_media(&mut self, from: BlobHash, to: BlobHash) {
        let services = self
            .bundled
            .services
            .iter_mut()
            .chain(self.services.values_mut());
        for service in services {
            for item in service.items.values_mut() {
                if let ItemKind::Media { blob, .. } = &mut item.kind {
                    if *blob == from {
                        *blob = to;
                    }
                }
            }
        }
    }
}

impl Library {
    /// Makes `set` the training services, as bundled, replacing any before.
    pub fn install_training(&self, set: TrainingSet) {
        let mut training = self.training();
        training.bundled = set;
        training.reset();
    }

    /// The training services as they stand, Training Session 0 first.
    pub fn training_services(&self) -> Vec<Service> {
        let training = self.training();
        training
            .bundled
            .services
            .iter()
            .filter_map(|s| training.service(s.id))
            .collect()
    }

    /// The decks the training services use.
    pub fn training_decks(&self) -> Vec<Deck> {
        self.training().bundled.decks.clone()
    }

    /// Points every training item that plays `from` at `to`: a training
    /// video the website changed. Practice changes stay.
    pub fn replace_training_media(&self, from: BlobHash, to: BlobHash) {
        if from != to {
            self.training().replace_media(from, to);
        }
    }

    /// Whether `id` is a training service.
    pub fn is_training(&self, id: ServiceId) -> bool {
        self.training().has_service(id)
    }

    /// Puts every training service back as it was bundled ("Reset
    /// training").
    pub fn reset_training(&self) {
        self.training().reset();
    }

    /// Whether a song shown in `service` goes in the CCLI usage log (RP-1,
    /// T38): never for a training service, which is practice, not worship.
    pub fn counts_for_ccli(&self, service: ServiceId) -> bool {
        !self.is_training(service)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lampstand_core::service::ItemKind;
    use lampstand_sync::team::DeviceKeypair;
    use lampstand_sync::DocKind;

    fn set() -> TrainingSet {
        let mut song = Song::new("Amazing Grace");
        song.id = SongId::generate();
        let mut service = Service::new("Training Session 1: Run a service");
        service.insert(
            0,
            ItemKind::Song {
                song: song.id,
                arrangement: None,
            },
        );
        TrainingSet {
            services: vec![service],
            songs: vec![song],
            ..TrainingSet::default()
        }
    }

    #[test]
    fn training_is_found_by_id_but_never_listed_or_stored() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::open(dir.path(), DeviceKeypair::generate()).unwrap();
        let set = set();
        let id = set.services[0].id;
        let song = set.songs[0].id;
        library.install_training(set.clone());

        assert_eq!(library.service(id).unwrap(), Some(set.services[0].clone()));
        assert_eq!(library.song(song).unwrap(), Some(set.songs[0].clone()));
        assert!(library.is_training(id));
        assert!(!library.counts_for_ccli(id));
        assert!(library.services().unwrap().is_empty());
        assert!(library.songs().unwrap().is_empty());
        assert_eq!(library.training_services(), set.services);
        // Nothing reached the repo, so there is nothing to sync.
        assert!(library.repo().list(DocKind::Service).is_empty());
        assert!(library.repo().list(DocKind::Song).is_empty());
        assert!(library
            .repo()
            .documents()
            .iter()
            .all(|(k, _)| *k == DocKind::Team || *k == DocKind::Theme));
    }

    #[test]
    fn practice_changes_stay_until_reset() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::open(dir.path(), DeviceKeypair::generate()).unwrap();
        let set = set();
        let id = set.services[0].id;
        library.install_training(set.clone());

        let mut changed = library.service(id).unwrap().unwrap();
        changed.insert(
            1,
            ItemKind::Header {
                title: "Added in practice".into(),
            },
        );
        library.put_service(&changed).unwrap();
        assert_eq!(library.service(id).unwrap().unwrap().items.len(), 2);
        assert!(library.repo().list(DocKind::Service).is_empty());

        library.reset_training();
        assert_eq!(library.service(id).unwrap(), Some(set.services[0].clone()));
    }

    #[test]
    fn a_changed_video_replaces_the_old_one_and_practice_stays() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::open(dir.path(), DeviceKeypair::generate()).unwrap();
        let mut set = set();
        let (old, new) = (BlobHash::of(b"old"), BlobHash::of(b"new"));
        set.services[0].insert(
            1,
            ItemKind::Media {
                blob: old,
                name: "Welcome.mp4".into(),
                looped: false,
            },
        );
        let id = set.services[0].id;
        library.install_training(set);
        let mut practised = library.service(id).unwrap().unwrap();
        practised.insert(
            0,
            ItemKind::Header {
                title: "Added in practice".into(),
            },
        );
        library.put_service(&practised).unwrap();

        library.replace_training_media(old, new);
        let media = |s: &Service| -> Vec<BlobHash> {
            s.items
                .values()
                .filter_map(|i| match i.kind {
                    ItemKind::Media { blob, .. } => Some(blob),
                    _ => None,
                })
                .collect()
        };
        let now = library.service(id).unwrap().unwrap();
        assert_eq!(media(&now), [new]);
        assert_eq!(now.items.len(), 3, "the practice change stays");
        library.reset_training();
        assert_eq!(media(&library.service(id).unwrap().unwrap()), [new]);
    }

    #[test]
    fn the_manifest_reads_as_the_website_writes_it() {
        let json = format!(
            r#"{{"version":1,"videos":[{{"id":"t1-welcome","file":"t1-welcome.mp4","size":3,"blake3":"{}","later":true}}]}}"#,
            BlobHash::of(b"abc")
        );
        let manifest: VideoManifest = serde_json::from_str(&json).unwrap();
        let video = manifest.video("t1-welcome.mp4").unwrap();
        assert_eq!(video.blake3, BlobHash::of(b"abc"));
        assert_eq!(video.size, 3);
        assert!(manifest.video("t0-welcome.mp4").is_none());
    }

    #[test]
    fn a_training_service_cannot_be_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::open(dir.path(), DeviceKeypair::generate()).unwrap();
        let set = set();
        let id = set.services[0].id;
        library.install_training(set);
        assert!(library.delete_service(id).is_err());
        assert!(library.service(id).unwrap().is_some());
        assert!(library.repo().list(DocKind::Service).is_empty());
    }

    #[test]
    fn a_training_hymn_cannot_be_saved_or_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::open(dir.path(), DeviceKeypair::generate()).unwrap();
        let set = set();
        let mut song = set.songs[0].clone();
        library.install_training(set);
        song.title = "Edited".into();
        assert!(library.put_song(&song).is_err());
        assert!(library.delete_song(song.id).is_err());
        assert_eq!(
            library.song(song.id).unwrap().unwrap().title,
            "Amazing Grace"
        );
        assert!(library.repo().list(DocKind::Song).is_empty());
    }
}
