//! The show engine: library, service order, cues, live state and commands,
//! plus the protocol types its clients speak.
//!
//! PRD: LV-1, LV-2, PS-7, PS-8, PS-9. Design: `docs/v0.1.md`.

pub mod library;
pub mod protocol;
pub mod service;
pub mod show;

pub use library::{ImportSummary, Library, LibraryError};
pub use service::{Item, Service, Source};
pub use show::Show;

#[cfg(test)]
mod tests;
