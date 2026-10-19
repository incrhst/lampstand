//! The network transport (T17): devices on the church network find each
//! other by mDNS, connect over WebSocket, prove their device keys and keep
//! every document in sync as it changes.
//!
//! - **One connection per pair of devices.** Either side may dial; the
//!   server side is `WS /sync` on the local server (`lampstand-server`),
//!   which hands each upgraded socket to [`SyncNode::accept`]. If both
//!   dial at once, both keep the connection dialled by the smaller device
//!   key and close the other.
//! - **Handshake.** A device-key challenge, and refusal of devices that are
//!   not paired; see [`handshake`](self) and [`trust`](self). Until T18
//!   trust is a device-local paired list set up with a pairing code.
//! - **Push, then sweep.** On connect, each side offers every document
//!   ([`Repo::documents`]). After that, a change here (`DocChanged`) is
//!   sent to every connected peer the moment it happens, so a lyric fix on
//!   a planner's laptop reaches the presenting machine in milliseconds.
//!   Incoming sync messages that arrive together are applied as one batch
//!   ([`Repo::receive_sync_messages`]).
//! - **Reconnect.** Each paired device has a dialler that reconnects with
//!   backoff (0.5 s doubling to 10 s) to its mDNS address or the address
//!   it was last reached at, and wakes at once when mDNS sees it again.
//!   On disconnect the peer's sync state is dropped ([`Repo::forget_peer`]),
//!   so the next connection resumes with a fresh sync handshake.
//! - **Blobs** are fetched by hash on request ([`SyncNode::fetch_blob`]),
//!   in 256 KiB chunks, and verified on arrival ([`BlobStore::put_verified`]).
//!
//! [`BlobStore::put_verified`]: crate::BlobStore::put_verified

mod discovery;
mod handshake;
mod session;
mod trust;
mod wire;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use lampstand_core::id::BlobHash;
use tokio::sync::{mpsc, oneshot, watch};
use uuid::Uuid;

pub use trust::{PairedDevice, PairingCode, PAIRING_TTL};
pub use wire::Link;

use crate::team::{DeviceKey, DeviceKeypair};
use crate::Repo;
use discovery::Discovery;
use handshake::{Me, Peer};
use trust::Trust;

/// The mDNS service type peers advertise.
pub const SERVICE_TYPE: &str = "_lampstand._tcp.local.";

/// The local server's port when none is given in an address.
pub const DEFAULT_PORT: u16 = 7340;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const BLOB_TIMEOUT: Duration = Duration::from_secs(120);
const BACKOFF_START: Duration = Duration::from_millis(500);
const BACKOFF_MAX: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub enum NetError {
    Io(io::Error),
    /// The other device (or this one) refused the connection; the reason
    /// is for people: "this device is not paired with that one".
    Refused(String),
    /// The connection closed before the handshake finished.
    Closed,
    Timeout,
    /// Nothing to connect to: no such address, no device showing a code.
    Unreachable(String),
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::Io(e) => write!(f, "{e}"),
            NetError::Refused(why) => write!(f, "refused: {why}"),
            NetError::Closed => write!(f, "the connection closed"),
            NetError::Timeout => write!(f, "the other device did not answer in time"),
            NetError::Unreachable(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for NetError {}

impl From<io::Error> for NetError {
    fn from(e: io::Error) -> Self {
        NetError::Io(e)
    }
}

/// How a node is set up.
#[derive(Debug, Clone)]
pub struct NodeConfig {
    /// This device's name as other devices show it: "Office PC".
    pub name: String,
    /// Device-local state: `paired.json` and partly fetched blobs. Never
    /// synced.
    pub dir: PathBuf,
}

/// What the devices screen shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeStatus {
    pub device: DeviceKey,
    pub name: String,
    /// The port the local server listens on, once started.
    pub port: Option<u16>,
    /// The code this device is showing, while it waits to pair.
    pub pairing: Option<PairingCode>,
    pub paired: Vec<PeerStatus>,
    /// Lampstand devices seen by mDNS that are not paired with this one.
    pub nearby: Vec<Nearby>,
}

impl NodeStatus {
    pub fn connected(&self) -> usize {
        self.paired.iter().filter(|p| p.connected).count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerStatus {
    pub device: DeviceKey,
    pub name: String,
    pub address: Option<String>,
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nearby {
    pub device: DeviceKey,
    pub name: String,
    pub address: String,
    /// It is showing a pairing code now.
    pub pairing: bool,
}

/// What a session is asked to do by the rest of the node.
pub(crate) enum Control {
    /// These documents changed here: offer them.
    Changed(Arc<Vec<Uuid>>),
    /// Ask the peer for a blob, and say how it went.
    Want(BlobHash, oneshot::Sender<Result<(), NetError>>),
}

struct Session {
    id: u64,
    /// Which side dialled; on a duplicate the smaller key's dial wins.
    client: DeviceKey,
    address: Option<String>,
    control: mpsc::UnboundedSender<Control>,
    stop: Option<oneshot::Sender<()>>,
    done: Option<oneshot::Receiver<()>>,
}

/// A device seen by mDNS.
#[derive(Debug, Clone)]
pub(crate) struct Found {
    pub fullname: String,
    pub name: String,
    pub addresses: Vec<SocketAddr>,
    pub pairing: bool,
}

#[derive(Default)]
struct Peers {
    sessions: HashMap<DeviceKey, Session>,
    next_id: u64,
    found: HashMap<DeviceKey, Found>,
    dialling: HashSet<DeviceKey>,
}

/// This device on the network: its paired devices, its connections and
/// the tasks that keep them in sync. Methods that start tasks must be
/// called inside a Tokio runtime.
pub struct SyncNode {
    repo: Arc<Repo>,
    key: DeviceKeypair,
    name: String,
    dir: PathBuf,
    trust: Mutex<Trust>,
    peers: Mutex<Peers>,
    /// Bumped whenever the status changes or mDNS sees something.
    changed: watch::Sender<u64>,
    port: Mutex<Option<u16>>,
    discovery: Mutex<Option<Discovery>>,
    me: Weak<SyncNode>,
}

impl SyncNode {
    /// A node over `repo`, signing with `key` (the same device key the repo
    /// was opened with). Nothing runs until [`SyncNode::start`].
    pub fn new(repo: Arc<Repo>, key: DeviceKeypair, config: NodeConfig) -> io::Result<Arc<Self>> {
        assert_eq!(
            key.public(),
            repo.device(),
            "the node signs with the repo's device key"
        );
        std::fs::create_dir_all(&config.dir)?;
        let trust = Trust::load(config.dir.join("paired.json"))?;
        Ok(Arc::new_cyclic(|me| Self {
            repo,
            key,
            name: config.name,
            dir: config.dir,
            trust: Mutex::new(trust),
            peers: Mutex::new(Peers::default()),
            changed: watch::Sender::new(0),
            port: Mutex::new(None),
            discovery: Mutex::new(None),
            me: me.clone(),
        }))
    }

    /// Starts pushing changes, a dialler per paired device and, with
    /// `advertise`, mDNS on `port` (the local server's).
    pub fn start(self: &Arc<Self>, port: u16, advertise: bool) {
        *lock(&self.port) = Some(port);
        self.start_pusher();
        let paired: Vec<DeviceKey> = lock(&self.trust)
            .devices()
            .iter()
            .map(|d| d.device)
            .collect();
        for device in paired {
            self.ensure_dialler(device);
        }
        if advertise {
            match Discovery::start(self, port) {
                Ok(d) => *lock(&self.discovery) = Some(d),
                Err(e) => eprintln!("lampstand: mDNS is off ({e}); connect by address instead"),
            }
        }
        self.bump();
    }

    pub fn device(&self) -> DeviceKey {
        self.key.public()
    }

    pub fn repo(&self) -> &Arc<Repo> {
        &self.repo
    }

    pub fn status(&self) -> NodeStatus {
        let pairing = lock(&self.trust).pairing();
        let trust = lock(&self.trust);
        let peers = lock(&self.peers);
        let paired = trust
            .devices()
            .iter()
            .map(|d| {
                let session = peers.sessions.get(&d.device);
                PeerStatus {
                    device: d.device,
                    name: d.name.clone(),
                    address: session
                        .and_then(|s| s.address.clone())
                        .or_else(|| d.address.clone()),
                    connected: session.is_some(),
                }
            })
            .collect();
        let mut nearby: Vec<Nearby> = peers
            .found
            .iter()
            .filter(|(key, _)| !trust.is_paired(**key))
            .filter_map(|(key, f)| {
                Some(Nearby {
                    device: *key,
                    name: f.name.clone(),
                    address: f.addresses.first()?.to_string(),
                    pairing: f.pairing,
                })
            })
            .collect();
        nearby.sort_by(|a, b| a.name.cmp(&b.name).then(a.device.cmp(&b.device)));
        NodeStatus {
            device: self.device(),
            name: self.name.clone(),
            port: *lock(&self.port),
            pairing,
            paired,
            nearby,
        }
    }

    /// Changes whenever [`SyncNode::status`] might have.
    pub fn watch(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    /// Shows a pairing code: the next device that confirms it is paired.
    pub fn start_pairing(&self) -> PairingCode {
        let code = lock(&self.trust).start_pairing();
        self.readvertise();
        self.bump();
        code
    }

    pub fn stop_pairing(&self) {
        lock(&self.trust).stop_pairing();
        self.readvertise();
        self.bump();
    }

    /// Pairs with a device showing `code`: the one at `address`, or else
    /// each device nearby that is showing a code, until one accepts. The
    /// two stay paired and connected.
    pub async fn pair(&self, code: &str, address: Option<&str>) -> Result<PeerStatus, NetError> {
        let code = trust::normalize_code(code);
        if code.len() != 6 {
            return Err(NetError::Refused("a pairing code has six digits".into()));
        }
        let targets: Vec<String> = match address {
            Some(a) => vec![normalize_address(a)],
            None => {
                let found: Vec<(DeviceKey, Found)> = lock(&self.peers)
                    .found
                    .iter()
                    .map(|(k, f)| (*k, f.clone()))
                    .collect();
                let trust = lock(&self.trust);
                found
                    .into_iter()
                    .filter(|(key, f)| f.pairing && !trust.is_paired(*key))
                    .flat_map(|(_, f)| f.addresses.into_iter().map(|a| a.to_string()))
                    .collect()
            }
        };
        if targets.is_empty() {
            return Err(NetError::Unreachable(
                "no device nearby is showing a pairing code; type its address".into(),
            ));
        }
        let mut last = None;
        for target in targets {
            match self.dial(&target, Some(&code)).await {
                Ok((peer, _session)) => return Ok(self.peer_status(peer.device)),
                Err(e) => last = Some(e),
            }
        }
        Err(last.expect("at least one target"))
    }

    /// Connects to an already paired device at `address` (for networks
    /// that block mDNS). The address is remembered for reconnecting.
    pub async fn connect(&self, address: &str) -> Result<PeerStatus, NetError> {
        let (peer, _session) = self.dial(&normalize_address(address), None).await?;
        Ok(self.peer_status(peer.device))
    }

    /// Stops trusting `device` and closes its connection.
    pub fn unpair(&self, device: DeviceKey) -> io::Result<bool> {
        let removed = lock(&self.trust).remove(device)?;
        if let Some(stop) = lock(&self.peers)
            .sessions
            .get_mut(&device)
            .and_then(|s| s.stop.take())
        {
            let _ = stop.send(());
        }
        self.bump();
        Ok(removed)
    }

    /// Closes every connection, for tests of catching up after a cut.
    /// Diallers reconnect with backoff.
    pub fn drop_connections(&self) {
        for session in lock(&self.peers).sessions.values_mut() {
            if let Some(stop) = session.stop.take() {
                let _ = stop.send(());
            }
        }
    }

    /// Fetches a blob from whichever connected device has it.
    pub async fn fetch_blob(&self, hash: BlobHash) -> Result<(), NetError> {
        if self.repo.blobs().path(&hash).is_some() {
            return Ok(());
        }
        let controls: Vec<_> = lock(&self.peers)
            .sessions
            .values()
            .map(|s| s.control.clone())
            .collect();
        for control in controls {
            let (tx, rx) = oneshot::channel();
            if control.send(Control::Want(hash, tx)).is_err() {
                continue;
            }
            if let Ok(Ok(Ok(()))) = tokio::time::timeout(BLOB_TIMEOUT, rx).await {
                return Ok(());
            }
        }
        Err(NetError::Unreachable(format!(
            "no connected device has the file {hash}"
        )))
    }

    /// Runs the server side of one incoming `WS /sync` connection, until
    /// it closes. Refused peers are told why and dropped.
    pub async fn accept(&self, mut link: Link) {
        let me = Me {
            key: &self.key,
            name: &self.name,
        };
        let peer = match tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            handshake::server(&mut link, &me, &self.trust),
        )
        .await
        {
            Ok(Ok(peer)) => peer,
            Ok(Err(e)) => {
                eprintln!("lampstand: sync connection refused: {e}");
                return;
            }
            Err(_) => return,
        };
        if peer.paired_now {
            self.bump();
            self.ensure_dialler(peer.device);
        }
        let _ = lock(&self.trust).seen_at(peer.device, &peer.name, None);
        if let Some(session) = self.begin(link, &peer, peer.device, None).await {
            let _ = session.await;
        }
    }

    // ---- Internals -----------------------------------------------------

    fn arc(&self) -> Arc<SyncNode> {
        self.me.upgrade().expect("a node is used through its Arc")
    }

    fn bump(&self) {
        self.changed.send_modify(|n| *n = n.wrapping_add(1));
    }

    fn peer_status(&self, device: DeviceKey) -> PeerStatus {
        self.status()
            .paired
            .into_iter()
            .find(|p| p.device == device)
            .unwrap_or(PeerStatus {
                device,
                name: String::new(),
                address: None,
                connected: false,
            })
    }

    fn readvertise(&self) {
        let pairing = lock(&self.trust).pairing().is_some();
        if let Some(d) = lock(&self.discovery).as_ref() {
            d.advertise(pairing);
        }
    }

    /// Dials `address` (`host:port`), runs the handshake and starts the
    /// session. Returns the session's task.
    async fn dial(
        &self,
        address: &str,
        code: Option<&str>,
    ) -> Result<(Peer, tokio::task::JoinHandle<()>), NetError> {
        let url = format!("ws://{address}/sync");
        let (ws, _) = tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(url))
            .await
            .map_err(|_| NetError::Timeout)?
            .map_err(|e| NetError::Unreachable(format!("cannot reach {address}: {e}")))?;
        let remote = match ws.get_ref() {
            tokio_tungstenite::MaybeTlsStream::Plain(s) => s.peer_addr().ok(),
            _ => None,
        };
        let mut link = Link::from_tungstenite(ws, remote);
        let me = Me {
            key: &self.key,
            name: &self.name,
        };
        let peer = tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            handshake::client(&mut link, &me, &self.trust, code),
        )
        .await
        .map_err(|_| NetError::Timeout)??;
        let _ = lock(&self.trust).seen_at(peer.device, &peer.name, Some(address.to_string()));
        if peer.paired_now {
            self.bump();
            self.ensure_dialler(peer.device);
        }
        match self
            .begin(link, &peer, self.device(), Some(address.to_string()))
            .await
        {
            Some(session) => Ok((peer, session)),
            // Already connected the other way, which wins: fine.
            None => Ok((peer, tokio::spawn(async {}))),
        }
    }

    /// Registers a session for `peer` and spawns it, unless a connection
    /// that wins the tie-break is already open.
    async fn begin(
        &self,
        link: Link,
        peer: &Peer,
        client: DeviceKey,
        address: Option<String>,
    ) -> Option<tokio::task::JoinHandle<()>> {
        loop {
            let old = {
                let mut peers = lock(&self.peers);
                match peers.sessions.get(&peer.device) {
                    Some(s) if s.client <= client => return None,
                    Some(_) => peers.sessions.remove(&peer.device),
                    None => {
                        let id = peers.next_id;
                        peers.next_id += 1;
                        let (control, control_rx) = mpsc::unbounded_channel();
                        let (stop, stop_rx) = oneshot::channel();
                        let (done_tx, done) = oneshot::channel();
                        peers.sessions.insert(
                            peer.device,
                            Session {
                                id,
                                client,
                                address: address
                                    .or_else(|| link.remote().map(|a| a.ip().to_string())),
                                control,
                                stop: Some(stop),
                                done: Some(done),
                            },
                        );
                        drop(peers);
                        self.bump();
                        let node = self.arc();
                        let device = peer.device;
                        return Some(tokio::spawn(async move {
                            session::run(node, link, device, id, control_rx, stop_rx).await;
                            drop(done_tx);
                        }));
                    }
                }
            };
            // A connection that loses the tie-break: stop it, wait until it
            // no longer touches the repo, then start afresh.
            if let Some(mut old) = old {
                if let Some(stop) = old.stop.take() {
                    let _ = stop.send(());
                }
                if let Some(done) = old.done.take() {
                    let _ = done.await;
                }
                self.repo.forget_peer(peer.device);
            }
        }
    }

    /// A session ended. If it is still the registered one, the peer is
    /// gone: drop its sync state.
    pub(crate) fn ended(&self, device: DeviceKey, id: u64) {
        let mut peers = lock(&self.peers);
        if peers.sessions.get(&device).is_some_and(|s| s.id == id) {
            peers.sessions.remove(&device);
            // Under the peers lock, so a new session cannot start before
            // the old state is gone.
            self.repo.forget_peer(device);
            drop(peers);
            self.bump();
        }
    }

    pub(crate) fn found(&self, device: DeviceKey, found: Found) {
        if device == self.device() {
            return;
        }
        lock(&self.peers).found.insert(device, found);
        self.bump();
    }

    pub(crate) fn lost(&self, fullname: &str) {
        let mut peers = lock(&self.peers);
        let before = peers.found.len();
        peers.found.retain(|_, f| f.fullname != fullname);
        let changed = peers.found.len() != before;
        drop(peers);
        if changed {
            self.bump();
        }
    }

    /// Forwards every repo change to every session.
    fn start_pusher(&self) {
        let changes = self.repo.subscribe();
        let node = self.me.clone();
        let team = self.repo.team_doc();
        std::thread::Builder::new()
            .name("lampstand-sync-push".into())
            .spawn(move || {
                while let Ok(first) = changes.recv() {
                    let Some(node) = node.upgrade() else {
                        return;
                    };
                    let mut ids: Vec<Uuid> = std::iter::once(first)
                        .chain(changes.try_iter())
                        .map(|c| c.id)
                        .collect();
                    // A new document also changes the team index.
                    ids.push(team);
                    ids.sort();
                    ids.dedup();
                    let ids = Arc::new(ids);
                    for s in lock(&node.peers).sessions.values() {
                        let _ = s.control.send(Control::Changed(ids.clone()));
                    }
                }
            })
            .expect("spawning the sync push thread");
    }

    fn ensure_dialler(&self, device: DeviceKey) {
        if !lock(&self.peers).dialling.insert(device) {
            return;
        }
        let node = self.arc();
        tokio::spawn(async move {
            node.dial_loop(device).await;
            lock(&node.peers).dialling.remove(&device);
        });
    }

    /// Keeps a paired device connected: dials its mDNS address or its last
    /// known one whenever there is no connection, backing off on failure.
    async fn dial_loop(&self, device: DeviceKey) {
        let mut changed = self.watch();
        let mut backoff = BACKOFF_START;
        loop {
            if !lock(&self.trust).is_paired(device) {
                return;
            }
            if lock(&self.peers).sessions.contains_key(&device) {
                let _ = changed.changed().await;
                continue;
            }
            let mut addresses: Vec<String> = lock(&self.peers)
                .found
                .get(&device)
                .map(|f| f.addresses.iter().map(|a| a.to_string()).collect())
                .unwrap_or_default();
            if let Some(a) = lock(&self.trust)
                .get(device)
                .and_then(|d| d.address.clone())
            {
                if !addresses.contains(&a) {
                    addresses.push(a);
                }
            }
            let mut lasted = Duration::ZERO;
            for address in addresses {
                let started = tokio::time::Instant::now();
                match self.dial(&address, None).await {
                    Ok((peer, session)) if peer.device == device => {
                        let _ = session.await;
                        lasted = started.elapsed();
                        break;
                    }
                    // Another device answers there now.
                    Ok(_) => {}
                    Err(_) => {}
                }
            }
            if lasted > Duration::from_secs(10) {
                backoff = BACKOFF_START;
            }
            let jitter = Duration::from_millis(u64::from(crate::repo::random_bytes::<1>()[0]));
            tokio::select! {
                _ = tokio::time::sleep(backoff + jitter) => {}
                _ = changed.changed() => {}
            }
            backoff = (backoff * 2).min(BACKOFF_MAX);
        }
    }
}

/// `"10.0.0.5"` → `"10.0.0.5:7340"`; `"ws://host:1234/sync"` → `"host:1234"`.
pub fn normalize_address(typed: &str) -> String {
    let s = typed.trim();
    let s = s.strip_prefix("ws://").unwrap_or(s);
    let s = s.strip_suffix("/sync").unwrap_or(s).trim_end_matches('/');
    let has_port = match s.rsplit_once(':') {
        // An IPv6 address has colons of its own; its port follows "]".
        Some((host, port)) => {
            port.parse::<u16>().is_ok() && (!host.contains(':') || host.ends_with(']'))
        }
        None => false,
    };
    if has_port {
        s.to_string()
    } else {
        format!("{s}:{DEFAULT_PORT}")
    }
}

/// This machine's LAN address with `port`, for people to type on a
/// network without mDNS. Asks the OS which interface would route to the
/// internet; nothing is sent.
pub fn local_addresses(port: u16) -> Vec<String> {
    let probe = std::net::UdpSocket::bind("0.0.0.0:0").and_then(|s| {
        s.connect("192.0.2.1:9")?;
        s.local_addr()
    });
    match probe {
        Ok(a) if !a.ip().is_unspecified() => vec![SocketAddr::new(a.ip(), port).to_string()],
        _ => Vec::new(),
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_get_the_default_port() {
        assert_eq!(normalize_address("10.0.0.5"), "10.0.0.5:7340");
        assert_eq!(normalize_address(" 10.0.0.5:9000 "), "10.0.0.5:9000");
        assert_eq!(
            normalize_address("ws://office.local:7341/sync"),
            "office.local:7341"
        );
        assert_eq!(normalize_address("[::1]:7400"), "[::1]:7400");
    }
}
