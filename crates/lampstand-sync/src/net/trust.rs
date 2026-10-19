//! Interim trust until teams (T18): a device-local list of paired devices,
//! set up with a short pairing code.
//!
//! The presenting machine shows a six-digit code; the service device types
//! it. The code is never sent: both sides run SPAKE2 with it and confirm
//! the derived key over the handshake transcript, so a listener on the
//! network learns nothing and an impostor gets one guess per attempt.
//! After five wrong guesses, or five minutes, the code is void. A guess
//! counts from the moment its handshake starts, so attempts made in
//! parallel get no more than five between them, and a guess that turns out
//! right still fails if the code was voided or replaced meanwhile.
//!
//! The list lives in `paired.json` beside the repo and never syncs. T18
//! replaces it with signed team membership.

use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::team::DeviceKey;

/// How long a pairing code stays valid.
pub const PAIRING_TTL: Duration = Duration::from_secs(5 * 60);

/// Wrong codes allowed before the code is void.
const MAX_FAILURES: u8 = 5;

/// A device this one trusts to sync.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairedDevice {
    #[serde(with = "hex_key")]
    pub device: DeviceKey,
    pub name: String,
    /// Where it was last reached (`host:port`), for networks without mDNS.
    pub address: Option<String>,
    pub paired_ms: i64,
}

/// The code a presenting machine shows while it waits to pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingCode {
    /// Six digits.
    pub code: String,
    /// When it stops working, in Unix milliseconds.
    pub expires_ms: i64,
}

struct Active {
    code: PairingCode,
    until: Instant,
    /// Tells this code from one started later.
    generation: u64,
    /// Handshakes that have started with this code.
    guesses: u8,
    failures: u8,
}

/// One handshake's claim on the code being shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Guess {
    pub code: PairingCode,
    generation: u64,
}

pub(crate) struct Trust {
    path: PathBuf,
    devices: Vec<PairedDevice>,
    active: Option<Active>,
    generations: u64,
}

#[derive(Serialize, Deserialize, Default)]
struct File {
    devices: Vec<PairedDevice>,
}

impl Trust {
    /// Reads `path`, or starts with no paired devices. A file that does
    /// not parse is set aside as `.bad` rather than lost.
    pub fn load(path: PathBuf) -> io::Result<Self> {
        let devices = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<File>(&bytes) {
                Ok(file) => file.devices,
                Err(e) => {
                    eprintln!("lampstand: {} is unreadable ({e})", path.display());
                    std::fs::rename(&path, path.with_extension("json.bad"))?;
                    Vec::new()
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        Ok(Self {
            path,
            devices,
            active: None,
            generations: 0,
        })
    }

    pub fn devices(&self) -> &[PairedDevice] {
        &self.devices
    }

    pub fn is_paired(&self, device: DeviceKey) -> bool {
        self.devices.iter().any(|d| d.device == device)
    }

    pub fn get(&self, device: DeviceKey) -> Option<&PairedDevice> {
        self.devices.iter().find(|d| d.device == device)
    }

    /// Adds or refreshes a paired device, and saves.
    pub fn add(
        &mut self,
        device: DeviceKey,
        name: &str,
        address: Option<String>,
    ) -> io::Result<()> {
        match self.devices.iter_mut().find(|d| d.device == device) {
            Some(d) => {
                d.name = name.to_string();
                if address.is_some() {
                    d.address = address;
                }
            }
            None => self.devices.push(PairedDevice {
                device,
                name: name.to_string(),
                address,
                paired_ms: now_ms(),
            }),
        }
        self.save()
    }

    /// Records where a paired device was reached. Saves only on change.
    pub fn seen_at(
        &mut self,
        device: DeviceKey,
        name: &str,
        address: Option<String>,
    ) -> io::Result<()> {
        let Some(d) = self.devices.iter_mut().find(|d| d.device == device) else {
            return Ok(());
        };
        let changed = d.name != name || (address.is_some() && d.address != address);
        if !changed {
            return Ok(());
        }
        d.name = name.to_string();
        if address.is_some() {
            d.address = address;
        }
        self.save()
    }

    pub fn remove(&mut self, device: DeviceKey) -> io::Result<bool> {
        let before = self.devices.len();
        self.devices.retain(|d| d.device != device);
        if self.devices.len() == before {
            return Ok(false);
        }
        self.save().map(|_| true)
    }

    /// A fresh code, replacing any shown before.
    pub fn start_pairing(&mut self) -> PairingCode {
        let n = u32::from_le_bytes(crate::repo::random_bytes::<4>()) % 1_000_000;
        let code = PairingCode {
            code: format!("{n:06}"),
            expires_ms: now_ms() + PAIRING_TTL.as_millis() as i64,
        };
        self.generations += 1;
        self.active = Some(Active {
            code: code.clone(),
            until: Instant::now() + PAIRING_TTL,
            generation: self.generations,
            guesses: 0,
            failures: 0,
        });
        code
    }

    pub fn stop_pairing(&mut self) {
        self.active = None;
    }

    /// The code being shown, if it is still valid.
    pub fn pairing(&mut self) -> Option<PairingCode> {
        if self
            .active
            .as_ref()
            .is_some_and(|a| Instant::now() >= a.until)
        {
            self.active = None;
        }
        self.active.as_ref().map(|a| a.code.clone())
    }

    /// Starts a guess at the code being shown, or `None` if no code is
    /// valid or its five guesses are all taken.
    pub fn begin_guess(&mut self) -> Option<Guess> {
        self.pairing()?;
        let a = self.active.as_mut()?;
        if a.guesses >= MAX_FAILURES {
            return None;
        }
        a.guesses += 1;
        Some(Guess {
            code: a.code.clone(),
            generation: a.generation,
        })
    }

    /// A peer confirmed a different code. Voids the code after too many.
    pub fn pairing_failed(&mut self, guess: &Guess) {
        if let Some(a) = &mut self.active {
            if a.generation != guess.generation {
                return;
            }
            a.failures += 1;
            if a.failures >= MAX_FAILURES {
                self.active = None;
            }
        }
    }

    /// A peer confirmed the code. True, and the code is used up, if it is
    /// still the one being shown and still valid; false if it was voided,
    /// expired or replaced while the handshake ran.
    pub fn pairing_succeeded(&mut self, guess: &Guess) -> bool {
        let current = self.pairing().is_some()
            && self
                .active
                .as_ref()
                .is_some_and(|a| a.generation == guess.generation);
        if current {
            self.active = None;
        }
        current
    }

    fn save(&self) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(&File {
            devices: self.devices.clone(),
        })
        .map_err(io::Error::other)?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        crate::repo::write_atomic(&self.path, &json)
    }
}

/// The digits of a code as typed: "482 913" and "482-913" are "482913".
pub(crate) fn normalize_code(typed: &str) -> String {
    typed.chars().filter(char::is_ascii_digit).collect()
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Device keys as 64 hex digits in `paired.json`.
mod hex_key {
    use serde::{Deserialize, Deserializer, Serializer};

    use crate::team::DeviceKey;

    pub fn serialize<S: Serializer>(key: &DeviceKey, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&key.to_hex())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DeviceKey, D::Error> {
        let s = String::deserialize(d)?;
        DeviceKey::from_hex(&s).ok_or_else(|| serde::de::Error::custom("not a device key"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_devices_survive_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("paired.json");
        let mut trust = Trust::load(path.clone()).unwrap();
        let key = DeviceKey([4; 32]);
        trust
            .add(key, "Office PC", Some("10.0.0.2:7340".into()))
            .unwrap();
        let again = Trust::load(path.clone()).unwrap();
        assert!(again.is_paired(key));
        assert_eq!(
            again.get(key).unwrap().address.as_deref(),
            Some("10.0.0.2:7340")
        );
        let mut again = again;
        assert!(again.remove(key).unwrap());
        assert!(!Trust::load(path).unwrap().is_paired(key));
    }

    #[test]
    fn a_code_is_void_after_five_wrong_guesses() {
        let dir = tempfile::tempdir().unwrap();
        let mut trust = Trust::load(dir.path().join("paired.json")).unwrap();
        let code = trust.start_pairing();
        assert_eq!(code.code.len(), 6);
        for _ in 0..4 {
            let guess = trust.begin_guess().unwrap();
            trust.pairing_failed(&guess);
        }
        assert_eq!(trust.pairing(), Some(code));
        let guess = trust.begin_guess().unwrap();
        trust.pairing_failed(&guess);
        assert_eq!(trust.pairing(), None);
        assert!(!trust.pairing_succeeded(&guess));
    }

    #[test]
    fn parallel_guesses_share_five_and_none_succeeds_after_the_code_is_void() {
        let dir = tempfile::tempdir().unwrap();
        let mut trust = Trust::load(dir.path().join("paired.json")).unwrap();
        trust.start_pairing();
        // Five handshakes start before any finishes; a sixth is refused.
        let guesses: Vec<Guess> = (0..5).map(|_| trust.begin_guess().unwrap()).collect();
        assert_eq!(trust.begin_guess(), None);
        for guess in &guesses[..4] {
            trust.pairing_failed(guess);
        }
        // The fifth guessed right, but the code is still shown: it pairs.
        assert!(trust.pairing_succeeded(&guesses[4]));
        assert_eq!(trust.pairing(), None, "a code is used once");
        assert!(!trust.pairing_succeeded(&guesses[4]));

        trust.start_pairing();
        let guesses: Vec<Guess> = (0..5).map(|_| trust.begin_guess().unwrap()).collect();
        for guess in &guesses {
            trust.pairing_failed(guess);
        }
        assert_eq!(
            trust.pairing(),
            None,
            "five parallel failures void the code"
        );
        assert_eq!(trust.begin_guess(), None);

        trust.start_pairing();
        let right = trust.begin_guess().unwrap();
        trust.stop_pairing();
        assert!(
            !trust.pairing_succeeded(&right),
            "a right guess still running when the code is withdrawn fails"
        );
    }

    #[test]
    fn a_guess_at_a_replaced_code_does_not_count_or_succeed() {
        let dir = tempfile::tempdir().unwrap();
        let mut trust = Trust::load(dir.path().join("paired.json")).unwrap();
        trust.start_pairing();
        let old = trust.begin_guess().unwrap();
        let code = trust.start_pairing();
        trust.pairing_failed(&old);
        assert!(!trust.pairing_succeeded(&old));
        assert_eq!(trust.pairing(), Some(code));
    }

    #[test]
    fn typed_codes_lose_their_spaces() {
        assert_eq!(normalize_code(" 482 913 "), "482913");
        assert_eq!(normalize_code("482-913"), "482913");
    }
}
