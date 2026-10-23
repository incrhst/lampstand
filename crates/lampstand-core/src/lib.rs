//! Core data model shared by every Lampstand crate. Pure data and pure
//! logic: no I/O, no async, no GPU.
//!
//! See docs/ARCHITECTURE.md for how the pieces fit together.

pub mod brand;
pub mod deck;
pub mod geom;
pub mod id;
pub mod output;
pub mod program;
pub mod safe_area;
pub mod scene;
pub mod scripture;
pub mod service;
pub mod song;
pub mod theme;

/// Whether a font family is available, and what stands in for it if not.
/// Implemented by `lampstand-layout`'s font set; used by the slide engine
/// to build its fidelity report without depending on the text stack.
pub trait FontLookup {
    fn lookup(&self, family: &str) -> FontMatch;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontMatch {
    Exact,
    /// Not installed or bundled; this metric-compatible family is used.
    Substitute(String),
    /// Nothing close; the default family is used.
    Missing,
}
