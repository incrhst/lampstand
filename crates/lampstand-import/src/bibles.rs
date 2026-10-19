//! Bible importers, one module per format.

pub mod openlp;
pub mod opensong;
pub mod osis;
pub mod usfm;
pub mod zefania;

/// The highest chapter or verse number an importer accepts. No book has
/// 1000 chapters or a chapter 1000 verses, and a malformed file must not
/// make an importer allocate without bound (OpenSong and OpenLP use the
/// same limit).
pub(crate) const MAX_NUMBER: usize = 999;
