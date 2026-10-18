use std::fmt;
use std::path::PathBuf;

use crate::SyncError;

/// The name of one synced document, such as `library` or
/// `services/2026-10-18-am`.
///
/// Segments are separated by `/` and use only ASCII letters, digits, `-` and
/// `_`, so an id maps safely to a path under the store directory and arrives
/// from the network without any path tricks.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocId(String);

impl DocId {
    pub const MAX_LEN: usize = 200;

    pub fn new(id: impl Into<String>) -> Result<Self, SyncError> {
        let id = id.into();
        let valid = !id.is_empty()
            && id.len() <= Self::MAX_LEN
            && id.split('/').all(|segment| {
                !segment.is_empty()
                    && segment
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            });
        if valid {
            Ok(Self(id))
        } else {
            Err(SyncError::InvalidDocId(id))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Path of this document relative to the store's `docs` directory.
    pub(crate) fn relative_path(&self) -> PathBuf {
        let mut path: PathBuf = self.0.split('/').collect();
        path.set_extension("automerge");
        path
    }
}

impl fmt::Display for DocId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_and_nested_ids() {
        assert!(DocId::new("library").is_ok());
        assert!(DocId::new("services/2026-10-18_am").is_ok());
        assert_eq!(
            DocId::new("services/a").unwrap().relative_path(),
            PathBuf::from("services/a.automerge")
        );
    }

    #[test]
    fn rejects_paths_that_escape_the_store() {
        for bad in [
            "", "..", "../etc", "a/../b", "/abs", "a//b", "a/", "a.b", "a b",
        ] {
            assert!(DocId::new(bad).is_err(), "{bad:?} should be rejected");
        }
    }
}
