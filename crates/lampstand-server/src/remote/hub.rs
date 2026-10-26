//! The app's side of Lampstand Remote (phones and tablets): the PIN shown
//! while one pairs, the paired remotes (`remotes.json`), the remote pages connected now, and
//! what they are told.
//!
//! A PIN lasts five minutes, five wrong tries void it, and only one runs at
//! a time, as with device pairing (`lampstand_sync::net`). Each address may
//! try at most [`TRIES_PER_ADDRESS`] PINs a minute, whatever PIN is shown,
//! so one phone cannot use up the tries meant for another in a burst. A
//! right PIN makes a random 256-bit session token; only its hash is kept.
//! `remotes.json` lives beside `paired.json` and never syncs.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, watch};

/// How long a PIN stays valid.
pub const PIN_TTL: Duration = Duration::from_secs(5 * 60);
/// Wrong PINs allowed before the PIN is void.
pub const MAX_WRONG: u8 = 5;
/// PINs one address may try in [`TRY_WINDOW`].
pub const TRIES_PER_ADDRESS: usize = 5;
pub const TRY_WINDOW: Duration = Duration::from_secs(60);
/// The longest name a phone is kept under.
const NAME_CHARS: usize = 40;

/// Answers one request from a remote page: what the page's session is
/// paired for, the request as the page sent it (JSON), and what to send
/// back. The app's gate decides what is allowed.
pub type RemoteHandler = Arc<
    dyn Fn(RemoteKind, serde_json::Value) -> BoxFuture<'static, serde_json::Value> + Send + Sync,
>;

/// What a phone is paired for (decision 0006, amended for 0.9.1). Chosen in
/// the Connect dialog before the PIN is shown, and kept with the session.
/// A session saved before 0.9.1 has none, and is an operator's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind")]
pub enum RemoteKind {
    /// Runs the service: the wall, Next, Back, Blank, Idle and the sound.
    #[default]
    Operator,
    /// A speaker's phone: Next and Back through one item's slides, only
    /// while that item is on air. Ends when its service is closed.
    Speaker {
        /// The service's ID, as the app writes it.
        service: String,
        /// The item's ID.
        item: String,
        /// The item's title when it paired, for the devices screen.
        title: String,
    },
}

/// The PIN shown now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePin {
    /// Six digits.
    pub pin: String,
    /// Unix milliseconds.
    pub expires_ms: i64,
    /// What the phone that types it is paired for.
    pub kind: RemoteKind,
}

/// A paired phone, as the devices screen lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSession {
    /// Not the token: a short public ID, for revoking.
    pub id: String,
    pub name: String,
    pub paired_ms: i64,
    pub last_seen_ms: Option<i64>,
    /// Remote pages open with this session now.
    pub connected: usize,
    pub kind: RemoteKind,
}

/// Why a PIN did not pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairError {
    /// No PIN is shown, or it ran out or was voided.
    NoPin,
    /// Wrong; `left` more tries before the PIN is void.
    Wrong { left: u8 },
    /// This address tried too often; try again after a minute.
    TooMany,
}

impl PairError {
    /// What the phone shows.
    pub fn words(&self) -> String {
        match self {
            PairError::NoPin => {
                "That PIN has run out. On the laptop, in Present, choose More, then Connect a \
                 phone or tablet for a new one."
                    .into()
            }
            PairError::Wrong { left: 0 } => {
                "That PIN is not right, and it has now stopped working. Ask for a new one on the \
                 laptop."
                    .into()
            }
            PairError::Wrong { left: 1 } => "That PIN is not right. One more try.".into(),
            PairError::Wrong { left } => format!("That PIN is not right. {left} more tries."),
            PairError::TooMany => "Too many tries from here. Wait a minute.".into(),
        }
    }
}

struct Pin {
    pin: RemotePin,
    until: Instant,
    wrong: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Saved {
    id: String,
    /// The token's BLAKE3 hash in hex.
    hash: String,
    name: String,
    paired_ms: i64,
    #[serde(default)]
    last_seen_ms: Option<i64>,
    /// Absent in a file written before 0.9.1: an operator's.
    #[serde(default)]
    kind: RemoteKind,
}

#[derive(Serialize, Deserialize, Default)]
struct File {
    sessions: Vec<Saved>,
}

#[derive(Default)]
struct Inner {
    pin: Option<Pin>,
    sessions: Vec<Saved>,
    tries: HashMap<IpAddr, VecDeque<Instant>>,
    connected: HashMap<String, usize>,
}

/// Lampstand Remote's pairing, sessions and feed. The app makes one, gives
/// it to the server through `Backend::remotes`, sets its handler, and
/// [`publishes`](RemoteHub::publish) what the remote shows.
pub struct RemoteHub {
    path: PathBuf,
    inner: Mutex<Inner>,
    /// Every remote page's feed: messages ready to send.
    feed: broadcast::Sender<Arc<str>>,
    /// The last message of each kind, sent to a page as it connects.
    latest: Mutex<Vec<(String, Arc<str>)>>,
    /// Bumped by every revoke, so open sockets check their session.
    revoked: watch::Sender<u64>,
    handler: OnceLock<RemoteHandler>,
    changed: OnceLock<Arc<dyn Fn() + Send + Sync>>,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn random<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).expect("operating system random source");
    buf
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hash(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

/// Six digits, every one as likely as any other.
fn fresh_pin() -> String {
    loop {
        let n = u32::from_le_bytes(random::<4>());
        // The largest multiple of a million below 2^32, so `%` is fair.
        if n < 4_294_000_000 {
            return format!("{:06}", n % 1_000_000);
        }
    }
}

/// Compares without stopping at the first difference.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A name as a person would read it: no control characters, trimmed, short.
fn clean_name(name: &str) -> String {
    let name: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(NAME_CHARS)
        .collect();
    let name = name.trim();
    if name.is_empty() {
        "Remote".into()
    } else {
        name.into()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn write_atomic(target: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = target.with_extension(format!("json.{}.tmp", hex(&random::<4>())));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, target).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

impl RemoteHub {
    /// Reads the paired phones from `path` (`remotes.json`), or starts with
    /// none. A file that does not parse is set aside as `.bad`: its phones
    /// pair again, nothing else is lost.
    pub fn open(path: PathBuf) -> io::Result<Self> {
        let sessions = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<File>(&bytes) {
                Ok(file) => file.sessions,
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
            inner: Mutex::new(Inner {
                sessions,
                ..Inner::default()
            }),
            feed: broadcast::channel(64).0,
            latest: Mutex::default(),
            revoked: watch::channel(0).0,
            handler: OnceLock::new(),
            changed: OnceLock::new(),
        })
    }

    /// What answers the remote pages' requests. Set once; until then every
    /// request is refused.
    pub fn set_handler(&self, handler: RemoteHandler) {
        let _ = self.handler.set(handler);
    }

    pub(crate) fn handler(&self) -> Option<RemoteHandler> {
        self.handler.get().cloned()
    }

    /// Called after the PIN, the paired phones or their connections change,
    /// so the app can tell its screens. Set once.
    pub fn on_change(&self, changed: Arc<dyn Fn() + Send + Sync>) {
        let _ = self.changed.set(changed);
    }

    fn changed(&self) {
        if let Some(f) = self.changed.get() {
            f();
        }
    }

    /// A fresh PIN for a phone that runs the service, replacing any shown
    /// before.
    pub fn start_pairing(&self) -> RemotePin {
        self.start_pairing_as(RemoteKind::Operator)
    }

    /// A fresh PIN for a phone paired as `kind`, replacing any shown before.
    pub fn start_pairing_as(&self, kind: RemoteKind) -> RemotePin {
        let pin = RemotePin {
            pin: fresh_pin(),
            expires_ms: now_ms() + PIN_TTL.as_millis() as i64,
            kind,
        };
        lock(&self.inner).pin = Some(Pin {
            pin: pin.clone(),
            until: Instant::now() + PIN_TTL,
            wrong: 0,
        });
        self.changed();
        pin
    }

    pub fn stop_pairing(&self) {
        let had = lock(&self.inner).pin.take().is_some();
        if had {
            self.changed();
        }
    }

    /// The PIN shown now, if it is still valid.
    pub fn pairing(&self) -> Option<RemotePin> {
        let mut inner = lock(&self.inner);
        Self::current_pin(&mut inner).map(|p| p.pin.clone())
    }

    fn current_pin(inner: &mut Inner) -> Option<&mut Pin> {
        if inner
            .pin
            .as_ref()
            .is_some_and(|p| Instant::now() >= p.until)
        {
            inner.pin = None;
        }
        inner.pin.as_mut()
    }

    /// A phone at `from` typed `pin`. Right, and the PIN is used up: the
    /// new session's token (for the cookie) and the name it was kept under.
    pub fn pair(&self, from: IpAddr, pin: &str, name: &str) -> Result<(String, String), PairError> {
        let result = self.try_pair(from, pin, name);
        if !matches!(result, Err(PairError::TooMany)) {
            self.changed();
        }
        result
    }

    fn try_pair(&self, from: IpAddr, pin: &str, name: &str) -> Result<(String, String), PairError> {
        let typed: String = pin.chars().filter(char::is_ascii_digit).collect();
        let mut inner = lock(&self.inner);
        let now = Instant::now();
        let tries = inner.tries.entry(from).or_default();
        while tries
            .front()
            .is_some_and(|t| now.duration_since(*t) >= TRY_WINDOW)
        {
            tries.pop_front();
        }
        if tries.len() >= TRIES_PER_ADDRESS {
            return Err(PairError::TooMany);
        }
        tries.push_back(now);
        // Forget addresses that have not tried for a while.
        inner.tries.retain(|_, t| {
            t.back()
                .is_some_and(|last| now.duration_since(*last) < TRY_WINDOW)
        });
        let Some(shown) = Self::current_pin(&mut inner) else {
            return Err(PairError::NoPin);
        };
        if !same(shown.pin.pin.as_bytes(), typed.as_bytes()) {
            shown.wrong += 1;
            let left = MAX_WRONG.saturating_sub(shown.wrong);
            if left == 0 {
                inner.pin = None;
            }
            return Err(PairError::Wrong { left });
        }
        let kind = shown.pin.kind.clone();
        inner.pin = None;
        let token = hex(&random::<32>());
        let name = clean_name(name);
        let now = now_ms();
        inner.sessions.push(Saved {
            id: hex(&random::<8>()),
            hash: hash(&token),
            name: name.clone(),
            paired_ms: now,
            last_seen_ms: Some(now),
            kind,
        });
        if let Err(e) = self.save(&inner) {
            eprintln!("lampstand: cannot save the paired remotes: {e}");
        }
        Ok((token, name))
    }

    /// The session a cookie's token belongs to: its ID and name.
    pub fn session(&self, token: &str) -> Option<(String, String)> {
        self.session_kind(token).map(|(id, name, _)| (id, name))
    }

    /// The session a cookie's token belongs to: its ID, name and what it is
    /// paired for.
    pub fn session_kind(&self, token: &str) -> Option<(String, String, RemoteKind)> {
        if token.is_empty() {
            return None;
        }
        let wanted = hash(token);
        lock(&self.inner)
            .sessions
            .iter()
            .find(|s| same(s.hash.as_bytes(), wanted.as_bytes()))
            .map(|s| (s.id.clone(), s.name.clone(), s.kind.clone()))
    }

    pub(crate) fn alive(&self, id: &str) -> bool {
        lock(&self.inner).sessions.iter().any(|s| s.id == id)
    }

    /// The paired phones, oldest first.
    pub fn sessions(&self) -> Vec<RemoteSession> {
        let inner = lock(&self.inner);
        inner
            .sessions
            .iter()
            .map(|s| RemoteSession {
                id: s.id.clone(),
                name: s.name.clone(),
                paired_ms: s.paired_ms,
                last_seen_ms: s.last_seen_ms,
                connected: inner.connected.get(&s.id).copied().unwrap_or(0),
                kind: s.kind.clone(),
            })
            .collect()
    }

    /// Forgets a phone: its cookie stops working and its open pages are
    /// disconnected at once. False if there is no such phone.
    pub fn revoke(&self, id: &str) -> bool {
        {
            let mut inner = lock(&self.inner);
            let before = inner.sessions.len();
            inner.sessions.retain(|s| s.id != id);
            if inner.sessions.len() == before {
                return false;
            }
            if let Err(e) = self.save(&inner) {
                eprintln!("lampstand: cannot save the paired remotes: {e}");
            }
        }
        self.revoked.send_modify(|n| *n += 1);
        self.changed();
        true
    }

    /// Forgets every speaker's phone whose service `ended` says is over
    /// (closed, ended, or another opened in its place): their pages are
    /// disconnected at once, as by a revoke. How many went.
    pub fn end_speakers(&self, ended: impl Fn(&str) -> bool) -> usize {
        let gone = {
            let mut inner = lock(&self.inner);
            let before = inner.sessions.len();
            inner.sessions.retain(|s| match &s.kind {
                RemoteKind::Speaker { service, .. } => !ended(service),
                RemoteKind::Operator => true,
            });
            let gone = before - inner.sessions.len();
            if gone > 0 {
                if let Err(e) = self.save(&inner) {
                    eprintln!("lampstand: cannot save the paired remotes: {e}");
                }
            }
            gone
        };
        if gone > 0 {
            self.revoked.send_modify(|n| *n += 1);
            self.changed();
        }
        gone
    }

    pub(crate) fn revocations(&self) -> watch::Receiver<u64> {
        self.revoked.subscribe()
    }

    /// A page of session `id` connected (`true`) or went away. Both count
    /// as seeing the phone.
    pub(crate) fn connection(&self, id: &str, open: bool) {
        {
            let mut inner = lock(&self.inner);
            let n = inner.connected.entry(id.to_string()).or_default();
            if open {
                *n += 1;
            } else {
                *n = n.saturating_sub(1);
            }
            if let Some(s) = inner.sessions.iter_mut().find(|s| s.id == id) {
                s.last_seen_ms = Some(now_ms());
            }
            if let Err(e) = self.save(&inner) {
                eprintln!("lampstand: cannot save the paired remotes: {e}");
            }
        }
        self.changed();
    }

    /// Sends `event` (JSON) to every remote page, and to each page that
    /// connects later, until another of the same `kind` replaces it.
    pub fn publish(&self, kind: &str, event: &str) {
        let message: Arc<str> = format!("{{\"event\":{event}}}").into();
        {
            let mut latest = lock(&self.latest);
            match latest.iter_mut().find(|(k, _)| k == kind) {
                Some((_, m)) => *m = message.clone(),
                None => latest.push((kind.to_string(), message.clone())),
            }
        }
        let _ = self.feed.send(message);
    }

    /// Stops sending the last `kind` to pages that connect from now on.
    pub fn forget(&self, kind: &str) {
        lock(&self.latest).retain(|(k, _)| k != kind);
    }

    pub(crate) fn latest(&self) -> Vec<Arc<str>> {
        lock(&self.latest).iter().map(|(_, m)| m.clone()).collect()
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<Arc<str>> {
        self.feed.subscribe()
    }

    fn save(&self, inner: &Inner) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(&File {
            sessions: inner.sessions.clone(),
        })
        .map_err(io::Error::other)?;
        write_atomic(&self.path, &json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 30));

    fn hub() -> (RemoteHub, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let hub = RemoteHub::open(dir.path().join("remotes.json")).unwrap();
        (hub, dir)
    }

    fn other(n: u8) -> IpAddr {
        IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, n))
    }

    fn wrong_pin(right: &str) -> String {
        format!("{:06}", (right.parse::<u32>().unwrap() + 1) % 1_000_000)
    }

    #[test]
    fn the_right_pin_pairs_once_and_keeps_only_a_hash() {
        let (hub, dir) = hub();
        let pin = hub.start_pairing();
        assert_eq!(pin.pin.len(), 6);
        let spaced = format!("{} {}", &pin.pin[..3], &pin.pin[3..]);
        let (token, name) = hub.pair(HOME, &spaced, " Sam's phone\n").unwrap();
        assert_eq!(name, "Sam's phone");
        assert_eq!(token.len(), 64, "256 bits in hex");
        assert_eq!(hub.session(&token).unwrap().1, "Sam's phone");
        assert!(hub.session("").is_none() && hub.session("nope").is_none());
        // The PIN is used up.
        assert!(hub.pairing().is_none());
        assert_eq!(hub.pair(other(2), &pin.pin, "x"), Err(PairError::NoPin));
        // The file holds the hash, never the token.
        let file = std::fs::read_to_string(dir.path().join("remotes.json")).unwrap();
        assert!(!file.contains(&token));
        assert!(file.contains(&hash(&token)));
    }

    #[test]
    fn a_wrong_pin_does_not_pair_and_says_how_many_tries_are_left() {
        let (hub, _dir) = hub();
        let pin = hub.start_pairing();
        let wrong = wrong_pin(&pin.pin);
        assert_eq!(
            hub.pair(HOME, &wrong, "x"),
            Err(PairError::Wrong { left: 4 })
        );
        assert_eq!(hub.pair(HOME, "", "x"), Err(PairError::Wrong { left: 3 }));
        assert!(hub.sessions().is_empty());
        assert!(hub.pair(other(2), &pin.pin, "x").is_ok());
    }

    #[test]
    fn the_fifth_wrong_try_voids_the_pin() {
        let (hub, _dir) = hub();
        let pin = hub.start_pairing();
        let wrong = wrong_pin(&pin.pin);
        // From five addresses, so the per-address limit is not what stops it.
        for (n, left) in (1..=5).zip([4, 3, 2, 1, 0]) {
            assert_eq!(
                hub.pair(other(n), &wrong, "x"),
                Err(PairError::Wrong { left })
            );
        }
        assert!(hub.pairing().is_none());
        assert_eq!(hub.pair(other(9), &pin.pin, "x"), Err(PairError::NoPin));
        assert!(hub.sessions().is_empty());
    }

    #[test]
    fn a_pin_runs_out_after_five_minutes() {
        let (hub, _dir) = hub();
        let pin = hub.start_pairing();
        assert!(pin.expires_ms > now_ms() + PIN_TTL.as_millis() as i64 - 5_000);
        lock(&hub.inner).pin.as_mut().unwrap().until = Instant::now();
        assert!(hub.pairing().is_none());
        assert_eq!(hub.pair(HOME, &pin.pin, "x"), Err(PairError::NoPin));
    }

    #[test]
    fn one_address_is_limited_whatever_pin_is_shown() {
        let (hub, _dir) = hub();
        for _ in 0..TRIES_PER_ADDRESS {
            assert_eq!(hub.pair(HOME, "000000", "x"), Err(PairError::NoPin));
        }
        let pin = hub.start_pairing();
        assert_eq!(hub.pair(HOME, &pin.pin, "x"), Err(PairError::TooMany));
        // Refused tries do not use up the PIN's tries.
        assert_eq!(hub.pairing(), Some(pin.clone()));
        // Another address is not held up.
        assert!(hub.pair(other(2), &pin.pin, "x").is_ok());
        // A minute later the address may try again.
        for t in lock(&hub.inner).tries.get_mut(&HOME).unwrap() {
            *t -= TRY_WINDOW;
        }
        assert_eq!(hub.pair(HOME, "000000", "x"), Err(PairError::NoPin));
    }

    #[test]
    fn a_new_pin_replaces_the_old_one() {
        let (hub, _dir) = hub();
        let first = hub.start_pairing();
        let second = hub.start_pairing();
        if first.pin != second.pin {
            assert!(matches!(
                hub.pair(HOME, &first.pin, "x"),
                Err(PairError::Wrong { .. })
            ));
        }
        assert!(hub.pair(other(2), &second.pin, "x").is_ok());
    }

    #[test]
    fn a_session_survives_a_restart_and_a_revoked_one_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("remotes.json");
        let (kept, gone) = {
            let hub = RemoteHub::open(path.clone()).unwrap();
            let pin = hub.start_pairing();
            let kept = hub.pair(HOME, &pin.pin, "Kept").unwrap().0;
            let pin = hub.start_pairing();
            let gone = hub.pair(HOME, &pin.pin, "Gone").unwrap().0;
            let id = hub.session(&gone).unwrap().0;
            let revocations = hub.revocations();
            assert!(hub.revoke(&id));
            assert!(revocations.has_changed().unwrap());
            assert!(!hub.revoke(&id));
            (kept, gone)
        };
        let hub = RemoteHub::open(path).unwrap();
        assert_eq!(hub.session(&kept).unwrap().1, "Kept");
        assert!(hub.session(&gone).is_none());
        assert_eq!(hub.sessions().len(), 1);
    }

    #[test]
    fn a_file_from_before_speakers_loads_its_phones_as_operators() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("remotes.json");
        let token = "ab".repeat(32);
        // remotes.json as 0.9.0 wrote it.
        let old = format!(
            r#"{{"sessions":[{{"id":"0011223344556677","hash":"{}","name":"Sam's phone","paired_ms":1,"last_seen_ms":2}}]}}"#,
            hash(&token)
        );
        std::fs::write(&path, old).unwrap();
        let hub = RemoteHub::open(path.clone()).unwrap();
        let (_, name, kind) = hub.session_kind(&token).unwrap();
        assert_eq!(name, "Sam's phone");
        assert_eq!(kind, RemoteKind::Operator);
        assert!(!dir.path().join("remotes.json.bad").exists());
        // Saved again, it still reads.
        hub.connection("0011223344556677", true);
        let hub = RemoteHub::open(path).unwrap();
        assert_eq!(hub.sessions()[0].kind, RemoteKind::Operator);
    }

    #[test]
    fn a_speaker_pairing_is_kept_and_ends_with_its_service() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("remotes.json");
        let speaker = RemoteKind::Speaker {
            service: "s1".into(),
            item: "i1".into(),
            title: "Sermon".into(),
        };
        let hub = RemoteHub::open(path.clone()).unwrap();
        let pin = hub.start_pairing_as(speaker.clone());
        let talk = hub.pair(HOME, &pin.pin, "Pastor").unwrap().0;
        let pin = hub.start_pairing();
        let run = hub.pair(other(2), &pin.pin, "Desk").unwrap().0;
        assert_eq!(hub.session_kind(&talk).unwrap().2, speaker);
        assert_eq!(hub.session_kind(&run).unwrap().2, RemoteKind::Operator);
        // Kept across a restart.
        let hub = RemoteHub::open(path).unwrap();
        assert_eq!(hub.session_kind(&talk).unwrap().2, speaker);
        let revocations = hub.revocations();
        assert_eq!(hub.end_speakers(|s| s == "other"), 0);
        assert!(!revocations.has_changed().unwrap());
        assert_eq!(hub.end_speakers(|s| s == "s1"), 1);
        assert!(revocations.has_changed().unwrap(), "its page is closed");
        assert!(hub.session_kind(&talk).is_none());
        assert!(
            hub.session_kind(&run).is_some(),
            "an operator's phone stays"
        );
    }

    #[test]
    fn an_unreadable_file_is_set_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("remotes.json");
        std::fs::write(&path, b"{ not json").unwrap();
        let hub = RemoteHub::open(path.clone()).unwrap();
        assert!(hub.sessions().is_empty());
        assert!(dir.path().join("remotes.json.bad").exists());
    }

    #[test]
    fn pins_are_six_digits() {
        for _ in 0..200 {
            let pin = fresh_pin();
            assert_eq!(pin.len(), 6);
            assert!(pin.bytes().all(|b| b.is_ascii_digit()));
        }
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("  "), "Remote");
        assert_eq!(clean_name("a\u{7}b"), "ab");
        assert_eq!(clean_name(&"x".repeat(100)).len(), NAME_CHARS);
    }

    #[test]
    fn a_page_that_connects_gets_the_last_of_each_kind() {
        let (hub, _dir) = hub();
        hub.publish("Live", r#"{"n":1}"#);
        hub.publish("Loaded", r#"{"n":2}"#);
        hub.publish("Live", r#"{"n":3}"#);
        let latest: Vec<String> = hub.latest().iter().map(|m| m.to_string()).collect();
        assert_eq!(latest, [r#"{"event":{"n":3}}"#, r#"{"event":{"n":2}}"#]);
        hub.forget("Loaded");
        assert_eq!(hub.latest().len(), 1);
    }
}
