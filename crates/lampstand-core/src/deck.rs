//! Slide decks: a PowerPoint or Google Slides file in the library, and
//! where it came from.
//!
//! PRD: PS-1, PS-2, PS-5, PS-6, CD-9.
//!
//! The `Deck` document records the source and which version is current.
//! The file itself is a blob. Parsing it (`lampstand-slides`) is derived
//! data, cached per device and never synced.

use serde::{Deserialize, Serialize};

use crate::id::{BlobHash, Id, Timestamp};

pub type DeckId = Id<Deck>;

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deck {
    pub id: DeckId,
    pub title: String,
    pub source: DeckSource,
    /// The PPTX that plays. Google Slides are stored as their PPTX export.
    pub current: Version,
    /// A newer version fetched from the source and waiting for an operator
    /// to accept it. A deck never changes while it is live (PS-5).
    pub pending: Option<Version>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub blob: BlobHash,
    pub fetched_at: Timestamp,
    /// The source's own version marker (Drive `version`, an ETag), used to
    /// check for updates without downloading.
    pub source_version: Option<String>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeckSource {
    /// Dropped in or picked from disk. Never updates.
    File { name: String },
    /// Picked with the Google Drive picker (PS-2). Google Slides files are
    /// exported to PPTX; other files download as they are.
    GoogleDrive {
        file_id: String,
        google_slides: bool,
    },
    /// A pasted "anyone with the link" Google Slides URL, no account needed.
    GoogleSlidesLink { url: String },
    /// A pasted OneDrive or SharePoint share link.
    OneDriveLink { url: String },
    /// Uploaded by a guest speaker through a service's upload link (CD-9).
    GuestUpload { uploader: String },
}

impl DeckSource {
    /// Whether the source can be polled for newer versions.
    pub fn is_linked(&self) -> bool {
        !matches!(
            self,
            DeckSource::File { .. } | DeckSource::GuestUpload { .. }
        )
    }
}
