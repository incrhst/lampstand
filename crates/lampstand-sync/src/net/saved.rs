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

use crate::roles::{Record, Roles, Signer, DEFAULT_TAKEOVER};
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

    pub fn into_roles(self, me: DeviceKey, name: String, signer: Signer) -> Roles {
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
            signer,
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
    /// What is on disk may be replaced. False when `roles.json` could not be
    /// read or set aside: it is left exactly as it is.
    pub writable: bool,
    /// What the roles were is lost (`roles.json` was unreadable): they wait
    /// to be taught again by the devices this one is paired with.
    pub damaged: bool,
}

fn read<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

impl RolesFile {
    /// Every device key in it is well formed.
    fn sound(&self) -> bool {
        self.always
            .operator
            .iter()
            .chain(&self.always.stream)
            .all(|(hex, _)| DeviceKey::from_hex(hex).is_some())
    }
}

/// `roles.json` if there is one; else what 0.7.0's files say. A
/// `roles.json` that cannot be read is never rebuilt from the older files
/// (they are staler) and never overwritten: it is set aside as
/// `roles.json.bad`, or left where it is if that fails, and the roles start
/// out unheld, to be taught by the devices this one meets.
pub(super) fn load(dir: &Path) -> Loaded {
    let path = dir.join(FILE);
    match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<RolesFile>(&bytes) {
            Ok(file) if file.sound() => {
                return Loaded {
                    file,
                    migrated: false,
                    writable: true,
                    damaged: false,
                }
            }
            bad => {
                let why = match bad {
                    Err(e) => e.to_string(),
                    Ok(_) => "a device key in it is not valid".to_string(),
                };
                let aside = path.with_extension("json.bad");
                let moved = std::fs::rename(&path, &aside);
                eprintln!(
                    "lampstand: {} is unreadable ({why}); {}. The roles start unheld.",
                    path.display(),
                    match &moved {
                        Ok(()) => format!("set aside as {}", aside.display()),
                        Err(e) => format!("it could not be set aside ({e}) and is left alone"),
                    }
                );
                return Loaded {
                    file: RolesFile::unheld(),
                    migrated: false,
                    writable: moved.is_ok(),
                    damaged: true,
                };
            }
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => {
            eprintln!(
                "lampstand: cannot read {}: {e}. The roles start unheld and it is left alone.",
                path.display()
            );
            return Loaded {
                file: RolesFile::unheld(),
                migrated: false,
                writable: false,
                damaged: true,
            };
        }
    }
    let presenter = read(&dir.join("presenter.json"));
    let driver = read(&dir.join("driver.json"));
    // A device that has never had any role has nothing to move over, and
    // nothing is written until a role changes.
    let migrated = presenter.is_some() || driver.is_some();
    Loaded {
        file: RolesFile::migrate(presenter, driver),
        migrated,
        writable: true,
        damaged: false,
    }
}

/// Writes `name` in `dir` through a temporary file of its own, readable
/// only by this user on Unix, flushed before it replaces the old one.
fn write_private(dir: &Path, name: &str, json: &[u8]) -> io::Result<()> {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SAVES: AtomicU64 = AtomicU64::new(0);
    let tmp = dir.join(format!(
        "{name}.{}.{}.tmp",
        std::process::id(),
        SAVES.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let written = options.open(&tmp).and_then(|mut f| {
        f.write_all(json)?;
        f.sync_all()
    });
    match written.and_then(|()| std::fs::rename(&tmp, dir.join(name))) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

pub(super) fn save(dir: &Path, file: &RolesFile) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(file).map_err(io::Error::other)?;
    write_private(dir, FILE, &json)
}

const CONTACTS: &str = "contact.json";

/// When each paired device was last heard from (Unix ms), kept so that a
/// restart does not forget how long a device has been away.
pub(super) fn load_contacts(dir: &Path) -> std::collections::HashMap<DeviceKey, i64> {
    let list: Vec<(String, i64)> = read(&dir.join(CONTACTS)).unwrap_or_default();
    list.into_iter()
        .filter_map(|(hex, ms)| Some((DeviceKey::from_hex(&hex)?, ms)))
        .collect()
}

pub(super) fn save_contacts(
    dir: &Path,
    contacts: &std::collections::HashMap<DeviceKey, i64>,
) -> io::Result<()> {
    let list: Vec<(String, i64)> = contacts.iter().map(|(k, ms)| (k.to_hex(), *ms)).collect();
    let json = serde_json::to_vec(&list).map_err(io::Error::other)?;
    write_private(dir, CONTACTS, &json)
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
    fn a_roles_json_that_does_not_parse_is_set_aside_not_rebuilt_from_older_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), "{not json").unwrap();
        std::fs::write(dir.path().join("presenter.json"), PRESENTER).unwrap();
        let loaded = load(dir.path());
        assert!(!loaded.migrated && loaded.writable);
        assert_eq!(
            loaded.file,
            RolesFile::unheld(),
            "staler files are not used"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("roles.json.bad")).unwrap(),
            "{not json"
        );
        assert!(!dir.path().join(FILE).exists());
    }

    #[test]
    fn a_device_key_that_is_not_hex_makes_the_file_unreadable_not_nobody() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = RolesFile::unheld();
        save(dir.path(), &file).unwrap();
        let good = std::fs::read_to_string(dir.path().join(FILE)).unwrap();
        let bad = good.replacen("\"holder\": null", "\"holder\": \"not a key\"", 1);
        assert_ne!(good, bad);
        std::fs::write(dir.path().join(FILE), &bad).unwrap();
        let loaded = load(dir.path());
        assert_eq!(loaded.file, RolesFile::unheld());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("roles.json.bad")).unwrap(),
            bad,
            "kept, not read as nobody"
        );
        file.always.operator.push(("zz".into(), "x".into()));
        save(dir.path(), &file).unwrap();
        assert!(load(dir.path()).file.always.operator.is_empty());
    }

    #[test]
    fn an_unreadable_file_that_cannot_be_set_aside_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where the file should be: reading it fails, not "missing".
        std::fs::create_dir(dir.path().join(FILE)).unwrap();
        std::fs::write(dir.path().join("presenter.json"), PRESENTER).unwrap();
        let loaded = load(dir.path());
        assert!(!loaded.writable && !loaded.migrated);
        assert_eq!(loaded.file, RolesFile::unheld());
    }

    #[test]
    fn contact_times_survive_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = std::collections::HashMap::new();
        c.insert(DeviceKey::from_hex(OFFICE).unwrap(), 1_700_000_000_000);
        save_contacts(dir.path(), &c).unwrap();
        assert_eq!(load_contacts(dir.path()), c);
        assert!(load_contacts(tempfile::tempdir().unwrap().path()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn roles_json_is_private_and_leaves_no_temporary_file() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &RolesFile::unheld()).unwrap();
        save(dir.path(), &RolesFile::unheld()).unwrap();
        let mode = std::fs::metadata(dir.path().join(FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "{mode:o}");
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, vec![FILE.to_string()]);
    }
}
