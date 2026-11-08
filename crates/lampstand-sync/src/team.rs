//! Teams, devices, roles and invites.
//!
//! PRD: CD-6; NFR Security.
//!
//! A team is rooted in its founder's device key. Every membership entry in
//! the team document is signed by an admin whose own entry chains back to
//! the founder, so a peer or the relay can work out each device's role from
//! the document alone, with no server in charge. Peers refuse connections
//! from devices without a role and drop changes to kinds of document the
//! author's role may not write.
//!
//! Threat model: this keeps out everyone outside the team and stops
//! members' mistakes. It does not stop a team member who runs a modified
//! client, because Automerge changes are not signed one by one; removing
//! that member's device does.

use serde::{Deserialize, Serialize};

use crate::DocKind;

/// A device's ed25519 public key.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(type = "Array<number>"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DeviceKey(pub [u8; 32]);

impl DeviceKey {
    /// 64 lowercase hex digits.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn from_hex(s: &str) -> Option<DeviceKey> {
        let s = s.trim();
        if s.len() != 64 || !s.is_ascii() {
            return None;
        }
        let mut key = [0u8; 32];
        for (i, byte) in key.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
        }
        Some(DeviceKey(key))
    }

    /// The first 8 hex digits in two groups, for people to compare:
    /// "3fa9 c201".
    pub fn fingerprint(&self) -> String {
        let hex = self.to_hex();
        format!("{} {}", &hex[..4], &hex[4..8])
    }

    /// Whether `signature` is this device's ed25519 signature of `message`.
    pub fn verify(&self, message: &[u8], signature: &[u8; 64]) -> bool {
        use ed25519_dalek::Verifier;
        ed25519_dalek::VerifyingKey::from_bytes(&self.0).is_ok_and(|key| {
            key.verify(message, &ed25519_dalek::Signature::from_bytes(signature))
                .is_ok()
        })
    }
}

/// A device's signing key, kept in the OS keychain (T18).
pub struct DeviceKeypair {
    signing: ed25519_dalek::SigningKey,
}

impl DeviceKeypair {
    /// A new key from the operating system's random source.
    pub fn generate() -> Self {
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).expect("operating system random source");
        Self::from_secret_bytes(secret)
    }

    /// Restores a key saved with [`DeviceKeypair::secret_bytes`].
    pub fn from_secret_bytes(secret: [u8; 32]) -> Self {
        Self {
            signing: ed25519_dalek::SigningKey::from_bytes(&secret),
        }
    }

    /// The secret key, for the keychain. Never sync or log it.
    pub fn secret_bytes(&self) -> [u8; 32] {
        self.signing.to_bytes()
    }

    pub fn public(&self) -> DeviceKey {
        DeviceKey(self.signing.verifying_key().to_bytes())
    }

    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        use ed25519_dalek::Signer;
        self.signing.sign(message).to_bytes()
    }
}

impl std::fmt::Debug for DeviceKeypair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceKeypair")
            .field("public", &self.public())
            .finish_non_exhaustive()
    }
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Role {
    Admin,
    Planner,
    Operator,
    Viewer,
}

impl Role {
    /// Who may change what. Every role can read everything.
    ///
    /// Operators may edit services (a last-minute song swap on Sunday) and
    /// accept deck updates, but not change the song library or themes.
    pub fn can_write(self, kind: DocKind) -> bool {
        use DocKind::*;
        match self {
            Role::Admin => true,
            Role::Planner => kind != Team,
            Role::Operator => matches!(kind, Service | Deck | UsageLog),
            Role::Viewer => kind == UsageLog,
        }
    }
}

/// A link or QR code that lets a new device join with a role. Single use,
/// with an expiry. Encoded as `lampstand://join?team=…&relay=…&token=…`.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invite {
    pub team: DeviceKey,
    pub team_name: String,
    pub relay: Option<String>,
    pub role: Role,
    /// 32 random bytes; the team document stores only its hash.
    #[cfg_attr(feature = "ts", ts(type = "Array<number>"))]
    pub token: [u8; 32],
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub expires_ms: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_keys_sign_and_verify() {
        let device = DeviceKeypair::generate();
        let again = DeviceKeypair::from_secret_bytes(device.secret_bytes());
        assert_eq!(device.public(), again.public());
        let signature = device.sign(b"join team");
        assert!(device.public().verify(b"join team", &signature));
        assert!(!device.public().verify(b"leave team", &signature));
        assert!(!DeviceKeypair::generate()
            .public()
            .verify(b"join team", &signature));
    }

    #[test]
    fn device_keys_print_as_hex() {
        let key = DeviceKeypair::generate().public();
        assert_eq!(DeviceKey::from_hex(&key.to_hex()), Some(key));
        assert_eq!(DeviceKey::from_hex("zz"), None);
        assert_eq!(key.fingerprint().len(), 9);
    }

    #[test]
    fn roles_narrow_from_admin_to_viewer() {
        use DocKind::*;
        let all = [
            Team,
            Song,
            Service,
            Theme,
            Deck,
            Translation,
            ChurchSettings,
            UsageLog,
        ];
        let count = |r: Role| all.iter().filter(|k| r.can_write(**k)).count();
        assert_eq!(count(Role::Admin), 8);
        assert_eq!(count(Role::Planner), 7);
        assert_eq!(count(Role::Operator), 3);
        assert_eq!(count(Role::Viewer), 1);
        assert!(!Role::Operator.can_write(Song));
        assert!(Role::Operator.can_write(Service));
    }
}
