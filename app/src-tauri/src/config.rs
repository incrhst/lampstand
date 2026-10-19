//! Device-local settings: output configuration and the device key. They
//! belong to this machine's screens and identity, so they live as plain
//! files in the app-data folder and never sync (ARCHITECTURE.md, "What
//! syncs and what stays on the device").

use std::io;
use std::path::{Path, PathBuf};

use lampstand_core::output::{OutputConfig, OutputId};
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
    use lampstand_core::output::{BackgroundMode, OutputRole, OutputTarget};
    use lampstand_core::safe_area::SafeArea;

    fn window() -> OutputConfig {
        OutputConfig {
            id: OutputId::generate(),
            name: "Rehearsal".into(),
            role: OutputRole::Audience { primary: true },
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
    fn the_device_key_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let first = device_key(dir.path()).unwrap();
        let second = device_key(dir.path()).unwrap();
        assert_eq!(first.public(), second.public());
    }
}
