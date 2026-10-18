//! Connectors to outside services: Google Drive and Slides, OneDrive share
//! links, and (Phase 2) Planning Center and OBS WebSocket.
//!
//! PRD: PS-1, PS-2, PS-5, CD-9.

pub mod google;
pub mod links;

/// A file fetched from a source, with the source's own version marker so
/// later checks can skip the download when nothing changed (PS-5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub name: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
    pub version: Option<String>,
}

#[derive(Debug)]
pub enum FetchError {
    Network(String),
    /// The link is private or the account has no access.
    Forbidden,
    NotFound,
    /// The link points at something that is not a presentation.
    NotADeck(String),
}
