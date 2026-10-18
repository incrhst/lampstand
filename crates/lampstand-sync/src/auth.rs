//! Per-church shared secret.
//!
//! Every device of a church, and the church's entry on a relay, holds the
//! same church id and token. A peer or relay only syncs with a connection
//! that presents both, as `GET /sync/<church>` with
//! `Authorization: Bearer <token>`. The token is the whole of the access
//! control for now: anyone holding it can read and write everything.
//! Per-person invites and roles are CD-6.

use std::fmt;

use rand::RngCore;

use crate::SyncError;

#[derive(Clone, PartialEq, Eq)]
pub struct ChurchCredentials {
    church: String,
    token: String,
}

impl ChurchCredentials {
    pub fn new(church: impl Into<String>, token: impl Into<String>) -> Result<Self, SyncError> {
        let church = church.into();
        validate_church_id(&church)?;
        Ok(Self {
            church,
            token: token.into(),
        })
    }

    /// A fresh church: random token for the given id.
    pub fn generate(church: impl Into<String>) -> Result<Self, SyncError> {
        Self::new(church, generate_token())
    }

    pub fn church(&self) -> &str {
        &self.church
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// True if `church` and `token` are this church's, compared in constant
    /// time so response timing does not leak the token.
    pub fn matches(&self, church: &str, token: &str) -> bool {
        church == self.church && tokens_equal(token, &self.token)
    }
}

impl fmt::Debug for ChurchCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChurchCredentials")
            .field("church", &self.church)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// 256 random bits as 64 hex characters.
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Church ids appear in URLs, mDNS records and relay data paths, so they are
/// kept to 1 to 64 lowercase ASCII letters, digits and `-`.
pub fn validate_church_id(church: &str) -> Result<(), SyncError> {
    let valid = (1..=64).contains(&church.len())
        && church
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if valid {
        Ok(())
    } else {
        Err(SyncError::InvalidChurchId(church.to_owned()))
    }
}

/// Constant-time comparison for the token bytes. Only the length can leak.
pub fn tokens_equal(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_only_the_right_church_and_token() {
        let creds = ChurchCredentials::new("grace", "secret").unwrap();
        assert!(creds.matches("grace", "secret"));
        assert!(!creds.matches("grace", "secreT"));
        assert!(!creds.matches("grace", "secret!"));
        assert!(!creds.matches("hope", "secret"));
    }

    #[test]
    fn church_ids_are_url_safe() {
        assert!(validate_church_id("grace-chapel-2").is_ok());
        for bad in ["", "Grace", "a/b", "a b", "..", &"a".repeat(65)] {
            assert!(validate_church_id(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn generated_tokens_are_long_and_distinct() {
        let (a, b) = (generate_token(), generate_token());
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert!(!format!("{:?}", ChurchCredentials::new("x", a.clone()).unwrap()).contains(&a));
    }
}
