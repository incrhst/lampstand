//! The library: songs, services, themes, decks and translations as the app
//! sees them, on top of the sync repo. Adds search, service packages, the
//! readiness check and CCLI usage logging.
//!
//! PRD: SL-1, SL-3, BI-2, CD-8, CD-10, PS-6, RP-1.

pub mod package;
pub mod readiness;
pub mod search;
pub mod usage;

use std::path::Path;

use lampstand_core::deck::{Deck, DeckId};
use lampstand_core::scripture::{BibleText, Translation, TranslationId};
use lampstand_core::service::{Service, ServiceId};
use lampstand_core::song::{Song, SongId};
use lampstand_core::theme::{Theme, ThemeId};
use lampstand_sync::{DocChanged, Repo, SyncError};

pub struct Library {
    repo: Repo,
}

impl Library {
    pub fn open(
        dir: &Path,
        device: lampstand_sync::team::DeviceKeypair,
    ) -> Result<Self, SyncError> {
        Ok(Self {
            repo: Repo::open(dir.join("repo"), device)?,
        })
    }

    pub fn repo(&self) -> &Repo {
        &self.repo
    }

    pub fn song(&self, id: SongId) -> Result<Option<Song>, SyncError> {
        let _ = id;
        todo!("T35")
    }

    pub fn songs(&self) -> Result<Vec<Song>, SyncError> {
        todo!("T35")
    }

    pub fn put_song(&self, song: &Song) -> Result<(), SyncError> {
        let _ = song;
        todo!("T35")
    }

    pub fn service(&self, id: ServiceId) -> Result<Option<Service>, SyncError> {
        let _ = id;
        todo!("T35")
    }

    pub fn services(&self) -> Result<Vec<Service>, SyncError> {
        todo!("T35")
    }

    pub fn put_service(&self, service: &Service) -> Result<(), SyncError> {
        let _ = service;
        todo!("T35")
    }

    pub fn theme(&self, id: ThemeId) -> Result<Option<Theme>, SyncError> {
        let _ = id;
        todo!("T35")
    }

    pub fn deck(&self, id: DeckId) -> Result<Option<Deck>, SyncError> {
        let _ = id;
        todo!("T35")
    }

    pub fn translations(&self) -> Result<Vec<Translation>, SyncError> {
        todo!("T35")
    }

    /// Decoded Bible text, cached in memory after the first call.
    pub fn bible_text(
        &self,
        id: TranslationId,
    ) -> Result<Option<std::sync::Arc<BibleText>>, SyncError> {
        let _ = id;
        todo!("T35")
    }

    pub fn subscribe(&self) -> std::sync::mpsc::Receiver<DocChanged> {
        self.repo.subscribe()
    }
}
