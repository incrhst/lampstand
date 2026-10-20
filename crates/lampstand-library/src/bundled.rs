//! The public-domain Bibles every install starts with (BI-2): KJV and WEB,
//! embedded from `assets/bibles/` (made by `cargo xtask bibles`; sources
//! and licences are in that folder's README).
//!
//! Each translation has a fixed ID, so two devices that install them on
//! first run end up with one document each, not two.

use lampstand_core::id::BlobHash;
use lampstand_core::scripture::{Licence, Translation, TranslationId};
use lampstand_sync::SyncError;
use uuid::Uuid;

use crate::Library;

pub struct BundledBible {
    pub id: TranslationId,
    pub name: &'static str,
    pub abbreviation: &'static str,
    pub language: &'static str,
    /// The translation blob: postcard then zstd, as `encode_bible_text`.
    pub blob: &'static [u8],
}

impl BundledBible {
    pub fn translation(&self) -> Translation {
        Translation {
            id: self.id,
            name: self.name.into(),
            abbreviation: self.abbreviation.into(),
            language: self.language.into(),
            licence: Licence::PublicDomain,
            text: BlobHash::of(self.blob),
        }
    }
}

pub const BUNDLED: [BundledBible; 2] = [
    BundledBible {
        id: TranslationId::from_uuid(Uuid::from_u128(0x0192_0000_0000_7000_8000_6b6a_7600_0001)),
        name: "King James Version",
        abbreviation: "KJV",
        language: "en",
        blob: include_bytes!("../../../assets/bibles/kjv.postcard.zst"),
    },
    BundledBible {
        id: TranslationId::from_uuid(Uuid::from_u128(0x0192_0000_0000_7000_8000_7765_6200_0001)),
        name: "World English Bible",
        abbreviation: "WEB",
        language: "en",
        blob: include_bytes!("../../../assets/bibles/web.postcard.zst"),
    },
];

impl Library {
    /// Stores each bundled Bible the library does not already have, and
    /// returns the abbreviations added. Callers decide when to run it: the
    /// app does so once per data folder, so a translation the operator
    /// deletes stays deleted.
    pub fn install_bundled_bibles(&self) -> Result<Vec<&'static str>, SyncError> {
        let mut added = Vec::new();
        for bible in &BUNDLED {
            if self.translation(bible.id)?.is_some() {
                continue;
            }
            let translation = bible.translation();
            let hash = self.repo().blobs().put(bible.blob)?;
            debug_assert_eq!(hash, translation.text);
            self.put_translation(&translation)?;
            added.push(bible.abbreviation);
        }
        Ok(added)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode_bible_text;
    use lampstand_core::scripture::parse_reference;
    use lampstand_sync::team::DeviceKeypair;

    fn john_3_16(abbreviation: &str) -> String {
        let bible = BUNDLED
            .iter()
            .find(|b| b.abbreviation == abbreviation)
            .unwrap();
        let text = decode_bible_text(bible.blob).unwrap();
        let verses = text.verses(&parse_reference("John 3:16").unwrap()).unwrap();
        assert_eq!(verses.len(), 1);
        verses[0].1.to_string()
    }

    /// Checked against the published text at ebible.org/eng-kjv and
    /// ebible.org/engwebp (2026-10).
    #[test]
    fn john_3_16_matches_the_published_text() {
        assert_eq!(
            john_3_16("KJV"),
            "For God so loved the world, that he gave his only begotten Son, that \
             whosoever believeth in him should not perish, but have everlasting life."
        );
        assert_eq!(
            john_3_16("WEB"),
            "For God so loved the world, that he gave his only born Son, that whoever \
             believes in him should not perish, but have eternal life."
        );
    }

    #[test]
    fn every_bundled_bible_has_the_whole_canon() {
        for bible in &BUNDLED {
            let text = decode_bible_text(bible.blob).unwrap();
            assert_eq!(text.books.len(), 66, "{}", bible.abbreviation);
            assert!(
                text.books.iter().all(|b| !b.is_empty()),
                "{}",
                bible.abbreviation
            );
            // Last verse of the Bible, so a truncated file fails here.
            let rev = text.verses(&parse_reference("Rev 22:21").unwrap());
            assert!(rev.is_ok(), "{}", bible.abbreviation);
            // No pilcrows or USFM markers left in the text.
            for verse in text.books.iter().flatten().flatten() {
                assert!(
                    !verse.contains(['¶', '\\', '|']),
                    "{}: {verse}",
                    bible.abbreviation
                );
            }
        }
    }

    #[test]
    fn install_adds_each_once_and_text_loads() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(dir.path(), DeviceKeypair::generate()).unwrap();
        assert_eq!(lib.install_bundled_bibles().unwrap(), vec!["KJV", "WEB"]);
        assert!(lib.install_bundled_bibles().unwrap().is_empty());
        assert_eq!(lib.translations().unwrap().len(), 2);
        for bible in &BUNDLED {
            let translation = lib.translation(bible.id).unwrap().unwrap();
            assert_eq!(translation.licence, Licence::PublicDomain);
            assert!(lib.bible_text(bible.id).unwrap().is_some());
        }
    }
}
