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

/// A device's signing key, kept in the OS keychain (T18).
pub struct DeviceKeypair {
    _private: (),
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
    fn roles_narrow_from_admin_to_viewer() {
        use DocKind::*;
        let all = [Team, Song, Service, Theme, Deck, Translation, UsageLog];
        let count = |r: Role| all.iter().filter(|k| r.can_write(**k)).count();
        assert_eq!(count(Role::Admin), 7);
        assert_eq!(count(Role::Planner), 6);
        assert_eq!(count(Role::Operator), 3);
        assert_eq!(count(Role::Viewer), 1);
        assert!(!Role::Operator.can_write(Song));
        assert!(Role::Operator.can_write(Service));
    }
}
