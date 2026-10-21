//! Device-local settings: output configuration, browser-source keys, the
//! device key and the other settings (`settings.json`, clicker mode). They
//! belong to this machine's screens and identity, so they live as plain
//! files in the app-data folder and never sync (ARCHITECTURE.md, "What
//! syncs and what stays on the device").

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use lampstand_api::DeviceSettings;
use lampstand_core::output::{BackgroundMode, OutputConfig, OutputId, OutputRole, OutputTarget};
use lampstand_core::safe_area::SafeArea;
use lampstand_sync::team::DeviceKeypair;

/// `outputs.json`: every output configured on this machine.
pub struct OutputStore {
    path: PathBuf,
    outputs: Vec<OutputConfig>,
}

impl OutputStore {
    /// Reads `path`, or starts empty if it does not exist. A file that does
    /// not parse is set aside as `outputs.json.bad` rather than lost.
    pub fn load(path: PathBuf) -> io::Result<Self> {
        let outputs = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(outputs) => outputs,
                Err(e) => {
                    eprintln!(
                        "lampstand: {} is unreadable ({e}); starting over",
                        path.display()
                    );
                    std::fs::rename(&path, path.with_extension("json.bad"))?;
                    Vec::new()
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        Ok(Self { path, outputs })
    }

    /// Like `load`, but on a machine with no `outputs.json` yet it starts
    /// with a rehearsal window and saves it, so a first launch has a screen
    /// to show. Once the file exists the outputs are the user's: deleting
    /// them all leaves none.
    pub fn load_or_seed(path: PathBuf) -> io::Result<Self> {
        let first_run = !path.exists();
        let mut store = Self::load(path)?;
        if first_run && store.outputs.is_empty() {
            if let Some(dir) = store.path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            store.put(rehearsal_window())?;
        }
        Ok(store)
    }

    pub fn all(&self) -> &[OutputConfig] {
        &self.outputs
    }

    pub fn get(&self, id: OutputId) -> Option<&OutputConfig> {
        self.outputs.iter().find(|o| o.id == id)
    }

    /// Adds or replaces an output, and saves.
    pub fn put(&mut self, output: OutputConfig) -> io::Result<()> {
        match self.outputs.iter_mut().find(|o| o.id == output.id) {
            Some(existing) => *existing = output,
            None => self.outputs.push(output),
        }
        self.save()
    }

    pub fn remove(&mut self, id: OutputId) -> io::Result<Option<OutputConfig>> {
        let Some(index) = self.outputs.iter().position(|o| o.id == id) else {
            return Ok(None);
        };
        let removed = self.outputs.remove(index);
        self.save()?;
        Ok(Some(removed))
    }

    fn save(&self) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(&self.outputs).map_err(io::Error::other)?;
        write_atomically(&self.path, &json)
    }
}

/// The window a fresh install starts with: small, a normal window in the
/// corner of the operator's screen. It is not the main audience screen, so
/// a display added later takes that place (a lone audience output is used
/// when none is main).
fn rehearsal_window() -> OutputConfig {
    OutputConfig {
        id: OutputId::generate(),
        name: "Rehearsal".into(),
        role: OutputRole::Audience { primary: false },
        target: OutputTarget::Window {
            width: 960,
            height: 540,
        },
        safe_area: SafeArea::default(),
        background: BackgroundMode::Bleed,
        theme: None,
        enabled: true,
    }
}

/// `browser-sources.json`: the key in each browser-source output's OBS
/// URL, kept beside `outputs.json` so the URL pasted into OBS survives
/// restarts. A key is a secret of this machine, so it is not in
/// `OutputConfig` (which the UI lists) and never syncs.
pub struct KeyStore {
    path: PathBuf,
    keys: BTreeMap<OutputId, String>,
}

impl KeyStore {
    /// Reads `path`, or starts empty if it does not exist or does not
    /// parse (a lost key only means pasting a new URL into OBS).
    pub fn load(path: PathBuf) -> io::Result<Self> {
        let keys = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                eprintln!(
                    "lampstand: {} is unreadable ({e}); browser sources get new URLs",
                    path.display()
                );
                BTreeMap::new()
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(e),
        };
        Ok(Self { path, keys })
    }

    pub fn get(&self, output: OutputId) -> Option<&str> {
        self.keys.get(&output).map(String::as_str)
    }

    /// Sets an output's key, and saves if it changed.
    pub fn put(&mut self, output: OutputId, key: &str) -> io::Result<()> {
        if self.get(output) == Some(key) {
            return Ok(());
        }
        self.keys.insert(output, key.to_string());
        self.save()
    }

    pub fn remove(&mut self, output: OutputId) -> io::Result<()> {
        if self.keys.remove(&output).is_some() {
            self.save()?;
        }
        Ok(())
    }

    fn save(&self) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(&self.keys).map_err(io::Error::other)?;
        write_atomically(&self.path, &json)
    }
}

/// `settings.json`: this machine's settings that are not about a screen.
pub struct SettingsStore {
    path: PathBuf,
    settings: DeviceSettings,
}

impl SettingsStore {
    /// Reads `path`, or the defaults if it does not exist or does not
    /// parse (it holds nothing that cannot be set again).
    pub fn load(path: PathBuf) -> io::Result<Self> {
        let settings = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                eprintln!("lampstand: {} is unreadable ({e})", path.display());
                DeviceSettings::default()
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => DeviceSettings::default(),
            Err(e) => return Err(e),
        };
        Ok(Self { path, settings })
    }

    pub fn get(&self) -> &DeviceSettings {
        &self.settings
    }

    pub fn put(&mut self, settings: DeviceSettings) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(&settings).map_err(io::Error::other)?;
        write_atomically(&self.path, &json)?;
        self.settings = settings;
        Ok(())
    }
}

/// The device's signing key, created on first run (`device.key`, 32 bytes).
pub fn device_key(dir: &Path) -> io::Result<DeviceKeypair> {
    let path = dir.join("device.key");
    match std::fs::read(&path) {
        Ok(bytes) => {
            let secret: [u8; 32] = bytes
                .try_into()
                .map_err(|_| io::Error::other(format!("{} is not 32 bytes", path.display())))?;
            Ok(DeviceKeypair::from_secret_bytes(secret))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let key = DeviceKeypair::generate();
            write_atomically(&path, &key.secret_bytes())?;
            Ok(key)
        }
        Err(e) => Err(e),
    }
}

fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> OutputConfig {
        rehearsal_window()
    }

    #[test]
    fn outputs_survive_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("outputs.json");
        let mut store = OutputStore::load(path.clone()).unwrap();
        let output = window();
        store.put(output.clone()).unwrap();
        let again = OutputStore::load(path.clone()).unwrap();
        assert_eq!(again.all(), std::slice::from_ref(&output));

        let mut again = again;
        again.remove(output.id).unwrap();
        assert!(OutputStore::load(path).unwrap().all().is_empty());
    }

    #[test]
    fn an_unreadable_file_is_set_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("outputs.json");
        std::fs::write(&path, b"{not json").unwrap();
        assert!(OutputStore::load(path.clone()).unwrap().all().is_empty());
        assert!(dir.path().join("outputs.json.bad").exists());
    }

    #[test]
    fn browser_source_keys_survive_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("browser-sources.json");
        let (a, b) = (OutputId::generate(), OutputId::generate());
        let mut keys = KeyStore::load(path.clone()).unwrap();
        keys.put(a, "0123456789abcdef").unwrap();
        keys.put(b, "fedcba9876543210").unwrap();
        keys.remove(b).unwrap();
        let again = KeyStore::load(path.clone()).unwrap();
        assert_eq!(again.get(a), Some("0123456789abcdef"));
        assert_eq!(again.get(b), None);
        std::fs::write(&path, b"[").unwrap();
        assert_eq!(KeyStore::load(path).unwrap().get(a), None);
    }

    #[test]
    fn settings_survive_a_reload_and_default_to_off() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut store = SettingsStore::load(path.clone()).unwrap();
        assert!(!store.get().clicker_mode);
        store.put(DeviceSettings { clicker_mode: true }).unwrap();
        assert!(SettingsStore::load(path).unwrap().get().clicker_mode);
    }

    #[test]
    fn the_device_key_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let first = device_key(dir.path()).unwrap();
        let second = device_key(dir.path()).unwrap();
        assert_eq!(first.public(), second.public());
    }

    #[test]
    fn a_fresh_install_starts_with_one_rehearsal_window() {
        let dir = tempfile::tempdir().unwrap();
        // The app's data folder does not exist yet on a first launch.
        let path = dir.path().join("not-made-yet").join("outputs.json");
        let store = OutputStore::load_or_seed(path.clone()).unwrap();
        let [seeded] = store.all() else {
            panic!("expected one output, found {}", store.all().len());
        };
        assert_eq!(seeded.name, "Rehearsal");
        assert!(seeded.enabled);
        assert_eq!(seeded.role, OutputRole::Audience { primary: false });
        assert!(matches!(seeded.target, OutputTarget::Window { .. }));
        // It is saved, so a second launch finds the same output.
        let again = OutputStore::load_or_seed(path).unwrap();
        assert_eq!(again.all(), store.all());
    }

    #[test]
    fn outputs_the_user_removed_are_not_seeded_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("outputs.json");
        let mut store = OutputStore::load_or_seed(path.clone()).unwrap();
        let id = store.all()[0].id;
        store.remove(id).unwrap();
        assert!(OutputStore::load_or_seed(path).unwrap().all().is_empty());
    }
}
