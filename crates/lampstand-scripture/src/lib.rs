//! Scripture: typed references, passages split across slides, and importers
//! for the translations a church holds a licence for.
//!
//! PRD: BI-1, BI-2.
//!
//! ```
//! use lampstand_scripture::{import, split, Budget, Reference};
//!
//! let usfm = "\\id JHN\n\\c 3\n\\v 16 For God so loved the world.";
//! let imported = import::usfm::import([usfm], Default::default()).unwrap();
//! let reference: Reference = "Jn 3:16".parse().unwrap();
//! let verses = imported.translation.passage(&reference).unwrap();
//! let slides = split(&verses, Budget { chars_per_line: 40, lines: 6 });
//! assert_eq!(slides[0].text(), "16 For God so loved the world.");
//! ```

pub mod book;
pub mod import;
pub mod reference;
pub mod split;
pub mod translation;

pub use book::{Book, BookError};
pub use reference::{Point, Range, Reference, ReferenceError};
pub use split::{split, Budget, Slide, SlideVerse};
pub use translation::{PassageError, Translation, TranslationMeta, Verse};
