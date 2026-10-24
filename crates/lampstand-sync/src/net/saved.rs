//! What survives a restart of the roles: `roles.json`, beside `paired.json`.
//! Device-local, never synced.
//!
//! Version 0.7.0 kept the same facts in two files: `presenter.json` (the
//! presenter lease and the takeover time) and `driver.json` (the devices
//! always allowed to drive). A device that finds neither `roles.json` nor
//! anything else but those moves them over once ([`load`]):
//!
//! - the lease becomes the Projection record, keeping its epoch and its
//!   takeover time;
//! - the driver seat becomes Operator: held by whoever held the lease, as it
//!   was before anyone asked to drive (a driver was never kept across a
//!   restart), with the always-allowed list carried over;
//! - Audio goes where Projection is, so the sound keeps playing where it
//!   does today; Stream is unheld, so Projection keeps the OBS duty;
//! - Operator and Audio start at the lease's epoch, so every device that
//!   moves over agrees on the same records without talking.
//!
//! The old files are left where they are. A device with nobody paired holds
//! all four roles whatever the records say ([`Roles`]).

use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::roles::{Record, Roles, DEFAULT_TAKEOVER};
use crate::team::DeviceKey;

pub(super) const FILE: &str = "roles.json";

/// `roles.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct RolesFile {
    pub projection: Record,
    pub operator: Record,
    pub stream: Record,
    pub audio: Record,
    #[serde(default = "default_takeover")]
    pub takeover_secs: u64,
    /// Devices answered at once, by role that offers it.
    #[serde(default)]
    pub always: Always,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Always {
    #[serde(default)]
    pub operator: Vec<(String, String)>,
    #[serde(default)]
    pub stream: Vec<(String, String)>,
}

fn default_takeover() -> u64 {
    DEFAULT_TAKEOVER.as_secs()
}

/// 0.7.0's `presenter.json`.
#[derive(Deserialize)]
struct PresenterFile {
    lease: Record,
    takeover_secs: u64,
}

/// 0.7.0's `driver.json`.
#[derive(Deserialize, Default)]
struct DriverFile {
    always: Vec<(String, String)>,
}

impl RolesFile {
    /// The roles of a device that has never had any.
    fn unheld() -> Self {
        Self {
            projection: Record::default(),
            operator: Record::default(),
            stream: Record::default(),
            audio: Record::default(),
            takeover_secs: default_takeover(),
            always: Always::default(),
        }
    }

    /// 0.7.0's lease and driver list, as roles.
    fn migrate(presenter: Option<PresenterFile>, driver: Option<DriverFile>) -> Self {
        let mut file = Self::unheld();
        if let Some(p) = presenter {
            file.projection = p.lease.clone();
            file.operator = p.lease.clone();
            file.audio = p.lease;
            file.takeover_secs = p.takeover_secs;
        }
        if let Some(d) = driver {
            file.always.operator = d.always;
        }
        file
    }

    pub fn into_roles(self, me: DeviceKey, name: String) -> Roles {
        let always = |list: &[(String, String)]| -> BTreeMap<DeviceKey, String> {
            list.iter()
                .filter_map(|(hex, name)| Some((DeviceKey::from_hex(hex)?, name.clone())))
                .collect()
        };
        Roles::new(
            me,
            name,
            [self.projection, self.operator, self.stream, self.audio],
            Duration::from_secs(self.takeover_secs),
            [
                BTreeMap::new(),
                always(&self.always.operator),
                always(&self.always.stream),
                BTreeMap::new(),
            ],
        )
    }

    pub fn from_roles(roles: &Roles) -> Self {
        use crate::roles::Role;
        let list = |role: Role| -> Vec<(String, String)> {
            roles
                .always(role)
                .into_iter()
                .map(|n| (n.device.to_hex(), n.name))
                .collect()
        };
        let [projection, operator, stream, audio] = roles.records().clone();
        Self {
            projection,
            operator,
            stream,
            audio,
            takeover_secs: roles.takeover_after().as_secs().max(1),
            always: Always {
                operator: list(Role::Operator),
                stream: list(Role::Stream),
            },
        }
    }
}

/// What was found in a device's `sync` folder.
pub(super) struct Loaded {
    pub file: RolesFile,
    /// It came from 0.7.0's files and is not on disk yet.
    pub migrated: bool,
}

fn read<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// `roles.json` if there is one; else what 0.7.0's files say.
pub(super) fn load(dir: &Path) -> Loaded {
    if let Some(file) = read::<RolesFile>(&dir.join(FILE)) {
        return Loaded {
            file,
            migrated: false,
        };
    }
    let presenter = read(&dir.join("presenter.json"));
    let driver = read(&dir.join("driver.json"));
    // A device that has never had any role has nothing to move over, and
    // nothing is written until a role changes.
    let migrated = presenter.is_some() || driver.is_some();
    Loaded {
        file: RolesFile::migrate(presenter, driver),
        migrated,
    }
}

pub(super) fn save(dir: &Path, file: &RolesFile) -> io::Result<()> {
    let tmp = dir.join(format!("{FILE}.tmp"));
    let json = serde_json::to_vec_pretty(file).map_err(io::Error::other)?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, dir.join(FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The files a real 0.7.0 pair wrote (see tests/fixtures/v0.7.0): the
    /// sanctuary handed the lease to the office PC at epoch 2, allowed it to
    /// drive "always", and set the takeover time to five minutes.
    const PRESENTER: &str = include_str!("../../tests/fixtures/v0.7.0/sanctuary/presenter.json");
    const DRIVER: &str = include_str!("../../tests/fixtures/v0.7.0/sanctuary/driver.json");
    const OFFICE: &str = "2ac160d95ab8b26c2789f247317a67f3d4d5efe707b578d952bd9a1f8af72389";

    #[test]
    fn the_lease_becomes_projection_and_the_always_list_carries_over() {
        let file = RolesFile::migrate(
            serde_json::from_str(PRESENTER).ok(),
            serde_json::from_str(DRIVER).ok(),
        );
        assert_eq!(file.projection.holder, DeviceKey::from_hex(OFFICE));
        assert_eq!(file.projection.name, "Office PC");
        assert_eq!(file.projection.epoch, 2, "the epoch is kept");
        assert_eq!(file.takeover_secs, 300);
        assert_eq!(
            file.always.operator,
            vec![(OFFICE.to_string(), "Office PC".to_string())]
        );
        assert!(file.always.stream.is_empty());
        // The sound stays with the screens; Stream is unheld.
        assert_eq!(file.operator, file.projection);
        assert_eq!(file.audio, file.projection);
        assert_eq!(file.stream, Record::default());
    }

    #[test]
    fn files_that_are_missing_or_unreadable_leave_every_role_unheld() {
        let file = RolesFile::migrate(None, None);
        assert_eq!(file, RolesFile::unheld());
        assert_eq!(file.takeover_secs, 120);
        // Only the driver list: the lease was never kept.
        let file = RolesFile::migrate(None, serde_json::from_str(DRIVER).ok());
        assert_eq!(file.always.operator.len(), 1);
        assert_eq!(file.projection, Record::default());
    }

    #[test]
    fn roles_json_round_trips_and_wins_over_the_old_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("presenter.json"), PRESENTER).unwrap();
        std::fs::write(dir.path().join("driver.json"), DRIVER).unwrap();
        let first = load(dir.path());
        assert!(first.migrated);
        save(dir.path(), &first.file).unwrap();
        // The old files are not touched.
        assert_eq!(
            std::fs::read_to_string(dir.path().join("presenter.json")).unwrap(),
            PRESENTER
        );
        // Changing the old ones now makes no difference.
        std::fs::write(dir.path().join("presenter.json"), "{}").unwrap();
        let again = load(dir.path());
        assert!(!again.migrated);
        assert_eq!(again.file, first.file);
    }

    #[test]
    fn a_roles_json_that_does_not_parse_falls_back_to_the_old_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), "{not json").unwrap();
        std::fs::write(dir.path().join("presenter.json"), PRESENTER).unwrap();
        let loaded = load(dir.path());
        assert!(loaded.migrated);
        assert_eq!(loaded.file.projection.epoch, 2);
    }
}
