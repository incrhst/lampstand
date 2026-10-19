//! Service packages (CD-8): one file holding a service and everything it
//! needs, to send by email, USB stick or chat.
//!
//! Format: a zip with the `.lampstand` extension.
//!
//! ```text
//! manifest.json        format version, app version, service ID, created at
//! service.json         the Service, as serde JSON
//! songs/<id>.json      each Song the service uses
//! themes/<id>.json     each Theme it uses
//! decks/<id>.json      each Deck (current version only)
//! blobs/<hash>         deck files, images, video
//! ```
//!
//! Licensed Bible text is never included: scripture items carry only their
//! passage and translation abbreviation, and the receiver's own library
//! supplies the text (or the readiness check reports it missing). Public
//! domain translations are included as blobs.
//!
//! Opening a package merges by ID: songs already in the library are kept
//! (and reported if they differ), new ones are added.

use std::io::{Read, Seek, Write};

use lampstand_core::service::ServiceId;

use crate::Library;

#[derive(Debug)]
pub enum PackageError {
    Io(std::io::Error),
    NotAPackage,
    /// Made by a newer version of Lampstand.
    TooNew {
        format: u32,
    },
    Sync(lampstand_sync::SyncError),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ImportSummary {
    pub songs_added: usize,
    /// In the library already with different content; the library's copy
    /// was kept.
    pub songs_differing: Vec<String>,
    pub decks_added: usize,
    pub missing_translations: Vec<String>,
}

pub const FORMAT_VERSION: u32 = 1;

pub fn export(
    library: &Library,
    service: ServiceId,
    out: impl Write + Seek,
) -> Result<(), PackageError> {
    let _ = (library, service, out);
    todo!("T37")
}

pub fn import(
    library: &Library,
    input: impl Read + Seek,
) -> Result<(ServiceId, ImportSummary), PackageError> {
    let _ = (library, input);
    todo!("T37")
}
