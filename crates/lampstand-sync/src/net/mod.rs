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
//! - **Encryption (T75).** The handshake also agrees an X25519 secret, and
//!   every frame after it is sealed with ChaCha20-Poly1305 under a key per
//!   direction and a counter; a frame that fails to open ends the session
//!   ([`secure`](self)). The WebSocket itself stays plain `ws://`.
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
//! - **Roles.** Four jobs, each held by one paired device at a time
//!   ([`roles`](crate::roles)): Projection (runs the screens, the old
//!   presenter lease), Operator (runs the service, the old driver seat),
//!   Stream and Audio. Each side sends its four records when a session opens
//!   and whenever one changes; the newer one wins. `RoleAsk` / `RoleAnswer`
//!   frames carry a request for a role. Every session also sends a `Beat`
//!   every few seconds and ends if it hears nothing for half a minute, so
//!   "offline" means a connection that is really gone, and the time since a
//!   holder was last heard from is what lets another device take over.
//! - **Driving.** The Operator operates the device that holds Projection.
//!   `DriveCommand` frames carry the Operator's requests to the Projection
//!   holder, which applies them if the sender holds Operator, and
//!   `DriveLive` frames carry its live state back for the Operator's
//!   screen. The payloads are opaque here: the app decides what they say
//!   ([`DriveHooks`]).
//! - **Blobs** are fetched by hash on request ([`SyncNode::fetch_blob`]),
//!   in 256 KiB chunks, and verified on arrival ([`BlobStore::put_verified`]).
//!
//! [`BlobStore::put_verified`]: crate::BlobStore::put_verified

mod discovery;
mod handshake;
mod saved;
mod secure;
mod session;
mod trust;
mod wire;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use lampstand_core::id::BlobHash;
use tokio::sync::{mpsc, oneshot, watch};
use uuid::Uuid;

pub use trust::{PairedDevice, PairingCode, PAIRING_TTL};
pub use wire::Link;

use crate::roles::{
    Answer, AskState, Asked, Asking, Blocked, Denied, Holder, Mode, Named, Net, Notice, OnAir,
    Record, Role, Roles, Signer, Standing,
};
use crate::team::{DeviceKey, DeviceKeypair};
use crate::wording;
use crate::Repo;
use discovery::Discovery;
use handshake::{Me, Peer};
use saved::RolesFile;
use trust::Trust;
use wire::Frame;

/// The mDNS service type peers advertise.
pub const SERVICE_TYPE: &str = "_lampstand._tcp.local.";

/// The local server's port when none is given in an address.
pub const DEFAULT_PORT: u16 = 7340;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const BLOB_TIMEOUT: Duration = Duration::from_secs(120);
const BACKOFF_START: Duration = Duration::from_millis(500);
const BACKOFF_MAX: Duration = Duration::from_secs(10);
/// How long after a connection opens a claim made while apart is believed.
const FRESH_SESSION: Duration = Duration::from_secs(10);

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
    /// When sync messages last arrived from, and went out to, any paired
    /// device (unix ms): what the status bar calls "syncing" while recent.
    pub last_in_ms: Option<i64>,
    pub last_out_ms: Option<i64>,
    pub presenter: PresenterStatus,
    /// All four roles.
    pub roles: RolesStatus,
}

/// One role, as the screens show it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoleStatus {
    pub role: Role,
    /// Who holds it, as this device last heard.
    pub record: Record,
    /// This device does the job: it holds the record, or it has nobody to
    /// share with.
    pub holds: bool,
    /// How this device could take it.
    pub standing: Standing,
    /// What stops this device taking it whatever the holder says.
    pub blocked: Option<Blocked>,
    /// The device asking this one, the holder, to hand it over. At most
    /// one: a second asker is told who is asking.
    pub request: Option<Named>,
    /// This device's own ask.
    pub asking: Option<Asking>,
    pub notice: Option<Notice>,
    /// Devices this one hands the role to without asking.
    pub always: Vec<Named>,
    /// A newer record was seen whose steps do not join this device's state
    /// (it missed more than eight): it waits for one that does.
    pub unverified: bool,
}

/// All four roles.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RolesStatus {
    pub projection: RoleStatus,
    pub operator: RoleStatus,
    pub stream: RoleStatus,
    pub audio: RoleStatus,
    /// This device has screens set up, so it can hold Projection.
    pub screens: bool,
    /// How long a holder must be offline before another device may take its
    /// role without asking.
    pub takeover_after: Duration,
    /// The Operator controls work: Projection is on this device or
    /// connected. While it is not, they wait and the seat is kept.
    pub operator_ready: bool,
}

impl RolesStatus {
    /// One role's status.
    pub fn of(&self, role: Role) -> &RoleStatus {
        match role {
            Role::Projection => &self.projection,
            Role::Operator => &self.operator,
            Role::Stream => &self.stream,
            Role::Audio => &self.audio,
        }
    }
}

/// Who may present, as the devices screen and the status bar show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresenterStatus {
    pub role: Mode,
    pub standing: Standing,
    /// The Projection record.
    pub lease: Record,
    /// How long the presenter must be offline before another device may
    /// take over without asking.
    pub takeover_after: Duration,
    /// Devices asking this one (the presenter) to hand over.
    pub requests: Vec<Named>,
    /// This device's own ask.
    pub asking: Option<Asking>,
    pub notice: Option<Notice>,
    pub drive: DriveStatus,
}

/// Driving, as the screens show it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DriveStatus {
    /// This device is driving the presenter.
    pub driving: Option<Driving>,
    /// Who is driving the presenter (on the presenter: authoritative; on
    /// another device: as the presenter last said).
    pub active: Option<Named>,
    /// Devices asking to drive this one, the presenter.
    pub requests: Vec<Named>,
    /// This device's own ask to drive.
    pub asking: Option<DriveAsking>,
    /// This device could ask to drive now: it is not presenting and the
    /// presenter is connected.
    pub can_drive: bool,
    /// Devices this one allows to drive without asking.
    pub always: Vec<Named>,
    pub notice: Option<DriveNotice>,
}

/// This device's ask to drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriveAsking {
    Waiting,
    Declined,
    Busy { by: String },
}

/// Something about driving the person should be told once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriveNotice {
    /// This device was driving and the Projection holder took the seat back.
    TakenBack { by: String },
}

/// The presenter this device is driving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Driving {
    pub presenter: DeviceKey,
    pub name: String,
    /// The presenter is connected. While it is not, the controls wait.
    pub online: bool,
}

/// What the app does with driving traffic. Called on sync tasks: they must
/// not block, so pass the work on.
pub struct DriveHooks {
    /// On the presenter: the active driver sent a request (JSON).
    pub command: Box<dyn Fn(DeviceKey, Vec<u8>) + Send + Sync>,
    /// On the driver: the presenter sent live state (one JSON event).
    pub live: Box<dyn Fn(Vec<u8>) + Send + Sync>,
    /// On the presenter: a device started (`true`) or stopped driving; send
    /// it the full live state when it starts.
    pub driver: Box<dyn Fn(bool) + Send + Sync>,
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
    /// It holds Projection.
    pub presenting: bool,
    /// Why the last attempt to reach it failed, for people; cleared on
    /// connecting.
    pub problem: Option<String>,
    /// The device answered and refused this one (it no longer has it
    /// paired), so waiting will not help.
    pub refused: bool,
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
    /// Send this frame as it is (role records and asks).
    Frame(Frame),
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
    /// When each device was last heard from, once its session has run.
    last_contact: HashMap<DeviceKey, Instant>,
    /// How long each connected device had been out of reach when its
    /// present connection began.
    away_before: HashMap<DeviceKey, Duration>,
    /// When each connected device's present connection began.
    session_started: HashMap<DeviceKey, Instant>,
    /// The last failed attempt to reach each paired device, with whether
    /// it was refused.
    problems: HashMap<DeviceKey, (String, bool)>,
}

#[derive(Default)]
struct Activity {
    last_in_ms: Option<i64>,
    last_out_ms: Option<i64>,
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
    /// Held by the one pairing handshake allowed at a time.
    pairing: tokio::sync::Mutex<()>,
    peers: Mutex<Peers>,
    activity: Mutex<Activity>,
    /// Bumped whenever the status changes or mDNS sees something.
    changed: watch::Sender<u64>,
    port: Mutex<Option<u16>>,
    discovery: Mutex<Option<Discovery>>,
    roles: Mutex<Roles>,
    /// Whether another device was operating this one at the last check, so
    /// the app is told when that changes.
    last_driven: Mutex<bool>,
    /// When each paired device was last heard from (Unix ms), kept on disk
    /// so a restart does not forget how long a device has been away.
    contacts: Mutex<HashMap<DeviceKey, i64>>,
    contacts_dirty: std::sync::atomic::AtomicBool,
    /// Added to the wall clock for the times kept on disk; tests move it.
    wall_offset_ms: std::sync::atomic::AtomicI64,
    /// Contact times are saved one at a time, newest last.
    contacts_save: Mutex<()>,
    /// Roles are saved one at a time, newest last.
    save_lock: Mutex<()>,
    /// What is on disk may be replaced (see [`saved::Loaded`]).
    persist_roles: bool,
    /// How long after a connection opens a claim made while apart is
    /// believed; one that arrives later cannot have been made out of reach.
    fresh_session: Mutex<Duration>,
    drive_hooks: Mutex<Option<Arc<DriveHooks>>>,
    /// When this node was made: how long a never-seen presenter has been
    /// silent.
    started: Instant,
    me: Weak<SyncNode>,
}

/// A snapshot of who is paired, connected and for how long silent.
struct Reach {
    paired: HashSet<DeviceKey>,
    connected: HashSet<DeviceKey>,
    silent: HashMap<DeviceKey, Duration>,
    away_before: HashMap<DeviceKey, Duration>,
    /// How long each connected device's present connection has been open.
    session_age: HashMap<DeviceKey, Duration>,
    fresh_session: Duration,
}

impl Reach {
    fn has_peers(&self) -> bool {
        !self.paired.is_empty()
    }

    fn holder(&self, device: DeviceKey) -> Holder {
        if !self.paired.contains(&device) {
            Holder::Gone
        } else if self.connected.contains(&device) {
            Holder::Online
        } else {
            Holder::Offline(self.silent.get(&device).copied().unwrap_or_default())
        }
    }
}

impl Net for Reach {
    fn has_peers(&self) -> bool {
        Reach::has_peers(self)
    }

    fn holder(&self, device: DeviceKey) -> Holder {
        Reach::holder(self, device)
    }

    fn paired(&self, device: DeviceKey) -> bool {
        self.paired.contains(&device)
    }

    fn away(&self, device: DeviceKey) -> Option<Duration> {
        if !self.paired.contains(&device) {
            None
        } else if self.connected.contains(&device) {
            // Only a connection that has just opened can bring a claim made
            // while apart; one that has been up for a while has been in
            // reach all along.
            let fresh = self
                .session_age
                .get(&device)
                .is_some_and(|age| *age <= self.fresh_session);
            fresh
                .then(|| self.away_before.get(&device).copied())
                .flatten()
        } else {
            self.silent.get(&device).copied()
        }
    }
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
        let loaded = saved::load(&config.dir);
        if loaded.migrated {
            // Moved over from 0.7.0's presenter.json and driver.json (or
            // nothing yet): keep it, so the old files are read only once.
            if let Err(e) = saved::save(&config.dir, &loaded.file) {
                eprintln!("lampstand: cannot save the roles: {e}");
            }
        }
        let secret = key.secret_bytes();
        let signer =
            Signer::new(move |message| DeviceKeypair::from_secret_bytes(secret).sign(message));
        let roles = loaded
            .file
            .into_roles(key.public(), config.name.clone(), signer);
        let mut roles = roles;
        if loaded.damaged {
            roles.mark_for_reseeding();
        }
        let dir = config.dir.clone();
        Ok(Arc::new_cyclic(|me| Self {
            repo,
            key,
            name: config.name,
            dir: config.dir,
            trust: Mutex::new(trust),
            pairing: tokio::sync::Mutex::new(()),
            peers: Mutex::new(Peers::default()),
            activity: Mutex::new(Activity::default()),
            changed: watch::Sender::new(0),
            port: Mutex::new(None),
            discovery: Mutex::new(None),
            roles: Mutex::new(roles),
            last_driven: Mutex::new(false),
            contacts: Mutex::new(saved::load_contacts(&dir)),
            contacts_dirty: std::sync::atomic::AtomicBool::new(false),
            wall_offset_ms: std::sync::atomic::AtomicI64::new(0),
            contacts_save: Mutex::new(()),
            save_lock: Mutex::new(()),
            persist_roles: loaded.writable,
            fresh_session: Mutex::new(FRESH_SESSION),
            drive_hooks: Mutex::new(None),
            started: Instant::now(),
            me: me.clone(),
        }))
    }

    /// Starts pushing changes, a dialler per paired device and, with
    /// `advertise`, mDNS on `port` (the local server's).
    pub fn start(self: &Arc<Self>, port: u16, advertise: bool) {
        *lock(&self.port) = Some(port);
        self.start_pusher();
        self.start_clock();
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
        let presenter = self.presenter_status();
        let roles = self.roles_status();
        let lease = presenter.lease.clone();
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
                    presenting: lease.holder == Some(d.device),
                    problem: peers
                        .problems
                        .get(&d.device)
                        .filter(|_| session.is_none())
                        .map(|(why, _)| why.clone()),
                    refused: session.is_none()
                        && peers.problems.get(&d.device).is_some_and(|(_, r)| *r),
                }
            })
            .collect();
        let activity = lock(&self.activity);
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
            presenter,
            roles,
            last_in_ms: activity.last_in_ms,
            last_out_ms: activity.last_out_ms,
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
        if lock(&self.roles).forget(device) {
            self.save_roles();
        }
        // What was kept about when it was last heard goes with it.
        {
            let mut peers = lock(&self.peers);
            peers.last_contact.remove(&device);
            peers.away_before.remove(&device);
            peers.session_started.remove(&device);
        }
        if lock(&self.contacts).remove(&device).is_some() {
            self.save_contacts_in_background();
        }
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
        let (peer, keys) = match tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            handshake::server(&mut link, &me, &self.trust, &self.pairing),
        )
        .await
        {
            Ok(Ok(done)) => done,
            Ok(Err(e)) => {
                eprintln!("lampstand: sync connection refused: {e}");
                return;
            }
            Err(_) => return,
        };
        if peer.paired_now {
            self.bump();
            self.ensure_dialler(peer.device);
            self.start_over_if_unverified();
            lock(&self.roles).seed_from(peer.device);
            // The device that showed the code is the main machine: it takes
            // the roles nobody holds (Projection, Operator and Audio).
            let reach = self.reach();
            let taken = lock(&self.roles).claim_if_unclaimed(&reach);
            for (role, record) in taken {
                self.role_changed(role, &record);
            }
        }
        let _ = lock(&self.trust).seen_at(peer.device, &peer.name, None);
        let link = link.encrypt(keys);
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
                presenting: false,
                problem: None,
                refused: false,
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
        let (peer, keys) = tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            handshake::client(&mut link, &me, &self.trust, code),
        )
        .await
        .map_err(|_| NetError::Timeout)??;
        let link = link.encrypt(keys);
        let _ = lock(&self.trust).seen_at(peer.device, &peer.name, Some(address.to_string()));
        if peer.paired_now {
            self.bump();
            self.ensure_dialler(peer.device);
            self.start_over_if_unverified();
            lock(&self.roles).seed_from(peer.device);
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
                        peers.problems.remove(&peer.device);
                        // How long it had been out of reach, from what was
                        // heard before this connection (kept across a
                        // restart). Nothing known: no longer than this run
                        // has lasted, so a claim made on no evidence is
                        // refused rather than believed.
                        let away = self.silence_or_run(&peers, peer.device);
                        peers.away_before.insert(peer.device, away);
                        peers.session_started.insert(peer.device, Instant::now());
                        drop(peers);
                        self.bump();
                        self.driving_changed();
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
            peers.last_contact.insert(device, Instant::now());
            peers.session_started.remove(&device);
            // Under the peers lock, so a new session cannot start before
            // the old state is gone.
            self.repo.forget_peer(device);
            drop(peers);
            lock(&self.contacts).insert(device, self.wall_now());
            self.save_contacts_in_background();
            // An ask from a device that went away is withdrawn, and so is a
            // hand-back that was on its way to it.
            {
                let mut roles = lock(&self.roles);
                for role in Role::ALL {
                    roles.receive_cancel(role, device);
                }
                roles.forget_yields();
            }
            // An Operator that went away is no longer driving anything.
            self.driving_changed();
            self.bump();
        }
    }

    // ---- Roles ----------------------------------------------------------

    fn reach(&self) -> Reach {
        let paired: HashSet<DeviceKey> = lock(&self.trust)
            .devices()
            .iter()
            .map(|d| d.device)
            .collect();
        let peers = lock(&self.peers);
        let connected: HashSet<DeviceKey> = peers.sessions.keys().copied().collect();
        let silent = paired
            .iter()
            .filter(|d| !connected.contains(d))
            .map(|d| (*d, self.silence_or_run(&peers, *d)))
            .collect();
        let session_age = peers
            .session_started
            .iter()
            .map(|(d, at)| (*d, at.elapsed()))
            .collect();
        Reach {
            session_age,
            fresh_session: *lock(&self.fresh_session),
            away_before: peers.away_before.clone(),
            paired,
            connected,
            silent,
        }
    }

    /// Every role, as the screens show it: who holds it, how this device
    /// could take it, and what it is waiting on.
    pub fn roles_status(&self) -> RolesStatus {
        let reach = self.reach();
        let net: &dyn Net = &reach;
        let r = lock(&self.roles);
        let one = |role: Role| RoleStatus {
            role,
            record: r.record(role).clone(),
            holds: r.holds_now(role, net),
            standing: r.standing(role, net),
            blocked: r.blocked(role, net),
            request: r.request(role).cloned(),
            asking: r.asking(role).cloned(),
            notice: r.notice(role).cloned(),
            always: r.always(role),
            unverified: r.unverified(role),
        };
        RolesStatus {
            projection: one(Role::Projection),
            operator: one(Role::Operator),
            stream: one(Role::Stream),
            audio: one(Role::Audio),
            screens: r.screens(),
            takeover_after: r.takeover_after(),
            operator_ready: r.operator_ready(net),
        }
    }

    /// Who may present, and who drives it: the Projection and Operator
    /// roles as the status bar and the devices screen have always shown them.
    pub fn presenter_status(&self) -> PresenterStatus {
        let reach = self.reach();
        let roles = self.roles_status();
        let drive = self.drive_status(&reach, &roles);
        let p = roles.projection;
        PresenterStatus {
            drive,
            role: if p.holds {
                Mode::Presenter
            } else {
                Mode::Planning
            },
            standing: p.standing,
            lease: p.record,
            takeover_after: roles.takeover_after,
            requests: p.request.into_iter().collect(),
            asking: p.asking,
            notice: p.notice,
        }
    }

    fn drive_status(&self, reach: &Reach, roles: &RolesStatus) -> DriveStatus {
        let me = self.device();
        let has_peers = reach.has_peers();
        let (p, o) = (&roles.projection, &roles.operator);
        let presenter = p.record.holder.filter(|h| *h != me);
        let holds_projection = p.record.holder == Some(me);
        DriveStatus {
            driving: presenter
                .filter(|_| has_peers && o.record.holder == Some(me))
                .map(|presenter| Driving {
                    presenter,
                    name: p.record.name.clone(),
                    online: reach.holder(presenter) == Holder::Online,
                }),
            // Whoever holds Operator while another device holds Projection
            // is driving it. On the Projection device only a connected
            // Operator counts: one that is away is not driving anything.
            active: o
                .record
                .holder
                .filter(|h| has_peers && Some(*h) != p.record.holder)
                .filter(|h| !holds_projection || reach.connected.contains(h))
                .map(|device| Named {
                    device,
                    name: o.record.name.clone(),
                }),
            requests: o.request.iter().cloned().collect(),
            asking: o.asking.as_ref().map(|a| match &a.state {
                AskState::Waiting => DriveAsking::Waiting,
                AskState::Declined => DriveAsking::Declined,
                AskState::Busy { by } => DriveAsking::Busy { by: by.clone() },
            }),
            // Not the Projection holder (it takes the seat back instead)
            // and not the Operator already.
            can_drive: has_peers
                && !holds_projection
                && o.record.holder != Some(me)
                && o.blocked.is_none()
                && matches!(
                    o.standing,
                    Standing::Ask | Standing::Open | Standing::TakeOver { .. }
                ),
            always: o.always.clone(),
            notice: o.notice.as_ref().map(|n| match n {
                Notice::Replaced { by } => DriveNotice::TakenBack { by: by.clone() },
            }),
        }
    }

    /// Whether this device may drive the screens now.
    pub fn role(&self) -> Mode {
        let reach = self.reach();
        lock(&self.roles).mode(&reach)
    }

    /// Tells the node whether this device has screens set up: Projection
    /// can be held only by a device that does.
    pub fn set_screens(&self, set_up: bool) {
        if lock(&self.roles).set_screens(set_up) {
            self.bump();
        }
    }

    /// Takes a role when nobody needs to allow it: nobody holds it, or the
    /// holder has been offline past the timeout.
    pub fn claim_role(&self, role: Role) -> Result<(), Denied> {
        let reach = self.reach();
        let record = lock(&self.roles).claim(role, &reach)?;
        self.role_changed(role, &record);
        Ok(())
    }

    /// Asks the holder of a role to hand it over; it sees a prompt.
    pub fn ask_role(&self, role: Role) -> Result<(), Denied> {
        let reach = self.reach();
        let holder = {
            let mut r = lock(&self.roles);
            r.ask(role, &reach)?;
            r.record(role).holder
        };
        if let Some(holder) = holder {
            self.send_to(holder, Frame::RoleAsk { role });
        }
        self.bump();
        Ok(())
    }

    /// Withdraws this device's ask for a role, or clears the "not now" it
    /// was shown.
    pub fn cancel_role_ask(&self, role: Role) {
        let (open, holder) = {
            let mut r = lock(&self.roles);
            (r.cancel_ask(role), r.record(role).holder)
        };
        if let (true, Some(holder)) = (open, holder) {
            self.send_to(holder, Frame::RoleCancel { role });
        }
        self.bump();
    }

    /// Hands a role to a device that asked; with `always`, remembers the
    /// device if the role offers that. Returns what this takes off air, for
    /// the app to do.
    pub fn allow_role(&self, role: Role, device: DeviceKey, always: bool) -> Result<OnAir, String> {
        if !lock(&self.peers).sessions.contains_key(&device) {
            return Err("That device is no longer connected.".into());
        }
        let reach = self.reach();
        let allowed = lock(&self.roles)
            .allow(role, device, always, &reach)
            .map_err(|e| wording::not_allowed(role, &e))?;
        self.role_changed(role, &allowed.record);
        Ok(allowed.on_air)
    }

    /// Says not now to a device that asked.
    pub fn decline_role(&self, role: Role, device: DeviceKey) {
        let (was_asking, epoch) = {
            let mut r = lock(&self.roles);
            (r.decline(role, device), r.record(role).epoch)
        };
        if was_asking {
            self.send_to(
                device,
                Frame::RoleAnswer {
                    role,
                    epoch,
                    answer: wire::RoleAnswer::Declined,
                },
            );
        }
        self.bump();
    }

    /// Remembers a paired device as always allowed to take a role from this
    /// one, or stops. Only roles that offer it remember anyone.
    pub fn set_role_always(&self, role: Role, device: DeviceKey, on: bool) -> Result<(), String> {
        let name = lock(&self.trust)
            .get(device)
            .map(|d| d.name.clone())
            .ok_or("That device is not paired.")?;
        if lock(&self.roles).set_always(role, device, name, on) {
            self.save_roles();
            self.bump();
        }
        Ok(())
    }

    pub fn dismiss_role_notice(&self, role: Role) {
        lock(&self.roles).dismiss_notice(role);
        self.bump();
    }

    /// Takes the presenter role when nobody needs to allow it: nobody
    /// holds it, or the presenter has been offline past the timeout.
    pub fn claim_presenter(&self) -> Result<(), Denied> {
        self.claim_role(Role::Projection)
    }

    /// Asks the presenter to hand over; it sees a prompt.
    pub fn ask_to_present(&self) -> Result<(), Denied> {
        self.ask_role(Role::Projection)
    }

    /// Withdraws this device's ask to present, or clears the "not now" it
    /// was shown.
    pub fn cancel_ask(&self) {
        self.cancel_role_ask(Role::Projection);
    }

    /// Hands the presenter role to a device that asked. This device
    /// becomes Planning; the app clears its screens when it sees that.
    pub fn allow_presenter(&self, device: DeviceKey) -> Result<(), String> {
        self.allow_role(Role::Projection, device, false).map(|_| ())
    }

    /// Says not now to a device that asked to present.
    pub fn decline_presenter(&self, device: DeviceKey) {
        self.decline_role(Role::Projection, device);
    }

    pub fn dismiss_presenter_notice(&self) {
        self.dismiss_role_notice(Role::Projection);
    }

    /// How long a holder may be offline before another device may take its
    /// role without asking. Kept on this device.
    pub fn set_takeover_after(&self, after: Duration) {
        lock(&self.roles).set_takeover_after(after);
        self.save_roles();
        self.bump();
    }

    /// A role's record changed here (a claim, a hand-over, a newer record
    /// from another device): keep it, tell every device, show it.
    fn role_changed(&self, role: Role, record: &Record) {
        self.save_roles();
        let frame = record_frame(role, record);
        for s in lock(&self.peers).sessions.values() {
            let _ = s.control.send(Control::Frame(frame.clone()));
        }
        self.driving_changed();
        self.bump();
    }

    /// Pairing with a device is the way out of a role state that could not
    /// be verified: start those roles over, to be seeded by what the new
    /// device sends.
    fn start_over_if_unverified(&self) {
        if lock(&self.roles).forget_unverified() {
            self.save_roles();
            self.bump();
        }
    }

    fn save_roles(&self) {
        if !self.persist_roles {
            return;
        }
        // The snapshot is taken under the same lock the write is made
        // under, so an older one cannot land after a newer one.
        let _one_at_a_time = lock(&self.save_lock);
        let file = RolesFile::from_roles(&lock(&self.roles));
        if let Err(e) = saved::save(&self.dir, &file) {
            eprintln!("lampstand: cannot save the roles: {e}");
        }
    }

    // ---- Driving --------------------------------------------------------
    //
    // The Operator drives the Projection holder: its commands cross to the
    // device that holds Projection, and that device's live state comes back.

    /// Where the app wants driving traffic to go. Set once at start.
    pub fn set_drive_hooks(&self, hooks: DriveHooks) {
        *lock(&self.drive_hooks) = Some(Arc::new(hooks));
    }

    fn hooks(&self) -> Option<Arc<DriveHooks>> {
        lock(&self.drive_hooks).clone()
    }

    /// Connected now, and still paired: a session that is closing after an
    /// unpair is already gone.
    fn connected(&self, device: DeviceKey) -> bool {
        lock(&self.trust).is_paired(device) && lock(&self.peers).sessions.contains_key(&device)
    }

    fn paired_name(&self, device: DeviceKey) -> String {
        lock(&self.trust)
            .get(device)
            .map(|d| d.name.clone())
            .unwrap_or_default()
    }

    /// The Projection device this device operates, when it holds Operator
    /// and another device holds Projection.
    fn driven_device(&self) -> Option<(DeviceKey, String)> {
        let me = self.device();
        let has_peers = !lock(&self.trust).devices().is_empty();
        let r = lock(&self.roles);
        let p = r.record(Role::Projection);
        let holder = p.holder.filter(|h| *h != me)?;
        (has_peers && r.holds(Role::Operator)).then(|| (holder, p.name.clone()))
    }

    /// The device operating this one, when this one holds Projection and
    /// another, connected, device holds Operator.
    fn operating_device(&self) -> Option<DeviceKey> {
        let me = self.device();
        let operator = {
            let r = lock(&self.roles);
            if !r.holds(Role::Projection) {
                return None;
            }
            r.record(Role::Operator).holder.filter(|h| *h != me)?
        };
        self.connected(operator).then_some(operator)
    }

    /// This device is driving the presenter, and whether it is connected.
    pub fn driving(&self) -> Option<Driving> {
        let (presenter, name) = self.driven_device()?;
        Some(Driving {
            presenter,
            online: self.connected(presenter),
            name,
        })
    }

    /// Whether another device is driving this one, the presenter.
    pub fn driven(&self) -> bool {
        self.operating_device().is_some()
    }

    /// Asks the presenter to let this device drive it (to hold Operator).
    /// The holder sees a prompt, unless it remembers this device.
    pub fn drive_ask(&self) -> Result<(), String> {
        let holder_name = {
            let r = lock(&self.roles);
            r.record(Role::Operator).name.clone()
        };
        match self.ask_role(Role::Operator) {
            Ok(()) => {
                eprintln!("lampstand: asking {holder_name} to let this device drive");
                Ok(())
            }
            // Already the operator of another device: nothing to do.
            Err(Denied::Already) if self.driven_device().is_some() => Ok(()),
            // Nobody holds the seat, or its holder has long been gone: no
            // one to ask.
            Err(Denied::NotNeeded) => self
                .claim_role(Role::Operator)
                .map_err(|e| wording::denied(Role::Operator, &e, &holder_name)),
            Err(e) => Err(wording::denied(Role::Operator, &e, &holder_name)),
        }
    }

    /// Stops driving, or withdraws the ask to drive. Stopping gives the
    /// Operator seat back to the Projection holder, which takes it; if that
    /// device is not connected, the seat is simply let go here, so stopping
    /// works at once as it always did.
    pub fn drive_cancel(&self) {
        if let Some((presenter, _)) = self.driven_device() {
            if self.connected(presenter) {
                if lock(&self.roles).yield_role(Role::Operator) {
                    self.send_to(
                        presenter,
                        Frame::RoleYield {
                            role: Role::Operator,
                        },
                    );
                }
            } else {
                let released = lock(&self.roles).release(Role::Operator);
                if let Some(record) = released {
                    self.role_changed(Role::Operator, &record);
                }
            }
        } else {
            self.cancel_role_ask(Role::Operator);
        }
        self.bump();
    }

    /// Clears a "not now", a "busy" or a "taken back" the person has seen.
    pub fn drive_dismiss(&self) {
        {
            let mut r = lock(&self.roles);
            r.clear_answer(Role::Operator);
            r.dismiss_notice(Role::Operator);
        }
        self.bump();
    }

    /// On the holder of Operator: lets a device that asked drive, and with
    /// `always` remembers it.
    pub fn drive_allow(&self, device: DeviceKey, always: bool) -> Result<(), String> {
        self.allow_role(Role::Operator, device, always)?;
        eprintln!(
            "lampstand: {} may drive this device",
            self.paired_name(device)
        );
        Ok(())
    }

    /// On the holder of Operator: says not now to a device that asked.
    pub fn drive_decline(&self, device: DeviceKey) {
        self.decline_role(Role::Operator, device);
    }

    /// On the presenter: takes control back from the driver at once.
    pub fn drive_take_back(&self) {
        let _ = self.claim_role(Role::Operator);
    }

    /// Remembers a paired device as always allowed to drive, or stops.
    pub fn drive_set_always(&self, device: DeviceKey, on: bool) -> Result<(), String> {
        self.set_role_always(Role::Operator, device, on)
    }

    /// On the driver: sends a request (JSON) to the presenter.
    pub fn drive_send(&self, payload: Vec<u8>) -> Result<(), String> {
        let (presenter, name) = self.driven_device().ok_or("this device is not driving")?;
        if !self.connected(presenter) {
            return Err(format!("{name} is not connected"));
        }
        self.send_to(presenter, Frame::DriveCommand { payload });
        Ok(())
    }

    /// On the presenter: sends live state (one JSON event) to the driver,
    /// if there is one.
    pub fn drive_live(&self, payload: Vec<u8>) {
        if let Some(driver) = self.operating_device() {
            self.send_to(driver, Frame::DriveLive { payload });
        }
    }

    /// Whether someone drives this device may have changed: tell the app,
    /// which sends the driver everything on air when one starts.
    fn driving_changed(&self) {
        let driven = self.driven();
        if std::mem::replace(&mut *lock(&self.last_driven), driven) != driven {
            if let Some(hooks) = self.hooks() {
                (hooks.driver)(driven);
            }
            self.bump();
        }
    }

    pub(crate) fn drive_command_received(&self, from: DeviceKey, payload: Vec<u8>) {
        let accepted = {
            let r = lock(&self.roles);
            r.holds(Role::Projection) && r.record(Role::Operator).holder == Some(from)
        };
        if !accepted {
            eprintln!(
                "lampstand: ignored a command from {}: it is not the driver here",
                self.paired_name(from)
            );
            return;
        }
        if let Some(hooks) = self.hooks() {
            (hooks.command)(from, payload);
        }
    }

    pub(crate) fn drive_live_received(&self, from: DeviceKey, payload: Vec<u8>) {
        // Only the Projection holder says what is on air, and only to the
        // device that operates it.
        let accepted = self.driven_device().is_some_and(|(p, _)| p == from);
        if !accepted {
            return;
        }
        if let Some(hooks) = self.hooks() {
            (hooks.live)(payload);
        }
    }

    fn send_to(&self, device: DeviceKey, frame: Frame) {
        if let Some(s) = lock(&self.peers).sessions.get(&device) {
            let _ = s.control.send(Control::Frame(frame));
        }
    }

    /// This device's four records, as frames, for a session that opens.
    pub(crate) fn record_frames(&self) -> Vec<Frame> {
        let r = lock(&self.roles);
        Role::ALL
            .into_iter()
            .map(|role| record_frame(role, r.record(role)))
            .collect()
    }

    /// A connection to `peer` has opened and its records have been sent:
    /// whatever this device asked of it and is still waiting on is asked
    /// again, in case the ask was lost when a connection was replaced.
    pub(crate) fn session_opened(&self, peer: DeviceKey) {
        let waiting = lock(&self.roles).waiting_asks(peer);
        for role in waiting {
            self.send_to(peer, Frame::RoleAsk { role });
        }
    }

    /// A device sent its record for a role: the newer of the two stands.
    pub(crate) fn record_received(&self, from: DeviceKey, role: Role, mut record: Record) {
        let known = record
            .holder
            .is_some_and(|h| lock(&self.trust).is_paired(h) || h == self.device());
        // A name travelling in a record is the sender's word, not a fact:
        // the name shown is the one this device has for the holder from
        // pairing (as for an ask), or what it already showed for that holder.
        record.name = match record.holder {
            None => String::new(),
            Some(h) if h == self.device() => self.name.clone(),
            Some(h) if known => self.paired_name(h),
            Some(h) => {
                let r = lock(&self.roles);
                let old = r.record(role);
                if old.holder == Some(h) {
                    old.name.clone()
                } else {
                    String::new()
                }
            }
        };
        let reach = self.reach();
        let merged = lock(&self.roles).merge(role, &record, from, &reach);
        if merged.changed {
            self.role_changed(role, &record);
        } else {
            // A newer record that does not join this device's state is
            // shown as such, not taken in. One that this device's own beats
            // is answered with it, so the other device learns where things
            // stand.
            let ours = lock(&self.roles).record(role).clone();
            if ours.beats(&record) {
                self.send_to(from, record_frame(role, &ours));
            }
            self.bump();
        }
    }

    pub(crate) fn role_ask_received(&self, from: DeviceKey, role: Role) {
        let name = self.paired_name(from);
        let asked = lock(&self.roles).receive_ask(role, from, name.clone());
        match asked {
            Asked::Prompt => self.bump(),
            Asked::Remembered => {
                eprintln!("lampstand: {name} may take {role:?}: remembered");
                if let Err(e) = self.allow_role(role, from, false) {
                    eprintln!("lampstand: could not hand {role:?} to {name}: {e}");
                    // Nobody is shown a prompt for it, so do not keep it, and
                    // do not leave the asker waiting.
                    let epoch = {
                        let mut r = lock(&self.roles);
                        r.decline(role, from);
                        r.record(role).epoch
                    };
                    self.send_to(
                        from,
                        Frame::RoleAnswer {
                            role,
                            epoch,
                            answer: wire::RoleAnswer::Declined,
                        },
                    );
                }
                self.bump();
            }
            Asked::Refuse(answer) => {
                let epoch = lock(&self.roles).record(role).epoch;
                let answer = match answer {
                    Answer::Declined => wire::RoleAnswer::Declined,
                    Answer::Busy { by } => wire::RoleAnswer::Busy { by },
                };
                self.send_to(
                    from,
                    Frame::RoleAnswer {
                        role,
                        epoch,
                        answer,
                    },
                );
                self.bump();
            }
            Asked::Ignored => {}
        }
    }

    pub(crate) fn role_cancel_received(&self, from: DeviceKey, role: Role) {
        if lock(&self.roles).receive_cancel(role, from) {
            self.bump();
        }
    }

    pub(crate) fn role_answer_received(
        &self,
        from: DeviceKey,
        role: Role,
        epoch: u64,
        answer: wire::RoleAnswer,
    ) {
        lock(&self.roles).receive_answer(
            role,
            from,
            epoch,
            match answer {
                wire::RoleAnswer::Declined => Answer::Declined,
                wire::RoleAnswer::Busy { by } => Answer::Busy { by },
            },
        );
        self.bump();
    }

    /// The holder of a role gave it up on purpose and asks this device to
    /// take it. Only done if the rules let this device take it unasked.
    pub(crate) fn role_yield_received(&self, from: DeviceKey, role: Role) {
        if lock(&self.roles).record(role).holder != Some(from) {
            return;
        }
        if let Err(e) = self.claim_role(role) {
            eprintln!(
                "lampstand: {} gave up {role:?} but this device cannot take it: {e}",
                self.paired_name(from)
            );
        }
    }

    /// Notes that `device` was just heard from.
    pub(crate) fn heard(&self, device: DeviceKey) {
        lock(&self.peers)
            .last_contact
            .insert(device, Instant::now());
        lock(&self.contacts).insert(device, self.wall_now());
        self.contacts_dirty
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// How long ago `device` was last heard from: by the clock of this run
    /// if it was, else by what was kept on disk. `None`: never.
    ///
    /// The larger of the two clocks: the one that counts time asleep (the
    /// wall clock) and the one that does not (`Instant`, which stands still
    /// while a laptop sleeps on macOS and Linux).
    fn silence_in(&self, peers: &Peers, device: DeviceKey) -> Option<Duration> {
        let by_run = peers.last_contact.get(&device).map(Instant::elapsed);
        let by_wall = lock(&self.contacts)
            .get(&device)
            .map(|ms| Duration::from_millis(u64::try_from(self.wall_now() - ms).unwrap_or(0)));
        match (by_run, by_wall) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        }
    }

    /// [`SyncNode::silence_in`], or, when nothing is known (the file that
    /// kept it is gone or unreadable), no longer than this run has lasted:
    /// the answer errs towards "not away", so a claim made on no evidence is
    /// refused rather than believed. Every place that needs it uses this one.
    fn silence_or_run(&self, peers: &Peers, device: DeviceKey) -> Duration {
        self.silence_in(peers, device)
            .unwrap_or_else(|| self.started.elapsed())
    }

    fn wall_now(&self) -> i64 {
        now_ms()
            + self
                .wall_offset_ms
                .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// For tests: the wall clock jumps forward, as it does over a sleep that
    /// `Instant` does not see.
    #[doc(hidden)]
    pub fn skip_wall_clock(&self, by: Duration) {
        self.wall_offset_ms.fetch_add(
            i64::try_from(by.as_millis()).unwrap_or(i64::MAX),
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    /// Saves when devices were last heard, off the async runtime when there
    /// is one: the write is synced.
    fn save_contacts_in_background(&self) {
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                let node = self.arc();
                runtime.spawn_blocking(move || node.save_contacts());
            }
            Err(_) => self.save_contacts(),
        }
    }

    fn save_contacts(&self) {
        let _one_at_a_time = lock(&self.contacts_save);
        let contacts = lock(&self.contacts).clone();
        if let Err(e) = saved::save_contacts(&self.dir, &contacts) {
            eprintln!("lampstand: cannot save when devices were last heard: {e}");
        }
        self.contacts_dirty
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    /// How long after a connection opens a claim made while apart is
    /// believed (ten seconds unless set).
    pub fn set_fresh_session(&self, window: Duration) {
        *lock(&self.fresh_session) = window;
    }

    /// For tests: forgets what this run has heard, as a restart does, and
    /// reads when devices were last heard from back from disk.
    #[doc(hidden)]
    pub fn forget_this_runs_contacts(&self) {
        lock(&self.peers).last_contact.clear();
        *lock(&self.contacts) = saved::load_contacts(&self.dir);
    }

    /// While the presenter is offline, the time since it was heard from
    /// changes the answer; wake the status every few seconds.
    fn start_clock(&self) {
        let node = self.me.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(5));
            loop {
                tick.tick().await;
                let Some(node) = node.upgrade() else {
                    return;
                };
                if node
                    .contacts_dirty
                    .load(std::sync::atomic::Ordering::Relaxed)
                {
                    node.save_contacts_in_background();
                }
                if matches!(
                    node.presenter_status().standing,
                    Standing::Wait { .. } | Standing::TakeOver { .. }
                ) || {
                    let s = node.roles_status();
                    [&s.operator, &s.stream, &s.audio].iter().any(|r| {
                        matches!(
                            r.standing,
                            Standing::Wait { .. } | Standing::TakeOver { .. }
                        )
                    })
                } {
                    node.bump();
                }
            }
        });
    }

    /// Sync messages came in from a peer.
    pub(crate) fn note_in(&self) {
        lock(&self.activity).last_in_ms = Some(now_ms());
        self.bump();
    }

    /// Sync messages went out to a peer.
    pub(crate) fn note_out(&self) {
        lock(&self.activity).last_out_ms = Some(now_ms());
        self.bump();
    }

    fn set_problem(&self, device: DeviceKey, problem: Option<(String, bool)>) {
        let changed = {
            let mut peers = lock(&self.peers);
            match problem {
                Some(p) => peers.problems.insert(device, p.clone()) != Some(p),
                None => peers.problems.remove(&device).is_some(),
            }
        };
        if changed {
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
            let mut failure = None;
            let tried = !addresses.is_empty();
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
                    Err(e) => {
                        let refused = matches!(e, NetError::Refused(_));
                        failure = Some((e.to_string(), refused));
                    }
                }
            }
            if lasted == Duration::ZERO {
                // Not found anywhere is not a failure: it is just away.
                self.set_problem(device, failure.filter(|_| tried));
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

fn record_frame(role: Role, record: &Record) -> Frame {
    Frame::Role {
        role,
        holder: record.holder.map(|h| h.0),
        epoch: record.epoch,
        steps: record.chain.clone(),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
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

    type Hooked = (
        Arc<SyncNode>,
        Arc<Mutex<Vec<DeviceKey>>>,
        Arc<Mutex<Vec<Vec<u8>>>>,
        tempfile::TempDir,
    );

    /// A node over the folder `dir`.
    fn node_at(dir: &std::path::Path) -> Arc<SyncNode> {
        let key = DeviceKeypair::generate();
        let repo = Arc::new(
            Repo::open(
                dir.join("repo"),
                DeviceKeypair::from_secret_bytes(key.secret_bytes()),
            )
            .unwrap(),
        );
        SyncNode::new(
            repo,
            key,
            NodeConfig {
                name: "Sanctuary".into(),
                dir: dir.join("sync"),
            },
        )
        .unwrap()
    }

    /// A node on its own, with a hook that records what it is asked to do.
    fn node_with_hooks() -> Hooked {
        let dir = tempfile::tempdir().unwrap();
        let node = node_at(dir.path());
        let (commands, live) = (
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(Vec::new())),
        );
        let (c, l) = (commands.clone(), live.clone());
        node.set_drive_hooks(DriveHooks {
            command: Box::new(move |from, _| lock(&c).push(from)),
            live: Box::new(move |payload| lock(&l).push(payload)),
            driver: Box::new(|_| {}),
        });
        (node, commands, live, dir)
    }

    /// A device's keys, for signing steps the way it would.
    fn kp(n: u8) -> DeviceKeypair {
        DeviceKeypair::from_secret_bytes([n; 32])
    }

    fn id(n: u8) -> DeviceKey {
        kp(n).public()
    }

    /// Nobody is paired with the device that signs: every holder is "gone".
    struct Strangers;

    impl Net for Strangers {
        fn has_peers(&self) -> bool {
            true
        }
        fn holder(&self, _: DeviceKey) -> Holder {
            Holder::Gone
        }
        fn paired(&self, _: DeviceKey) -> bool {
            false
        }
        fn away(&self, _: DeviceKey) -> Option<Duration> {
            None
        }
    }

    /// Pairs the node with device `n`, which then seeds it.
    fn paired(node: &SyncNode, n: u8) {
        lock(&node.trust).add(id(n), "Paired", None).unwrap();
        lock(&node.roles).seed_from(id(n));
    }

    /// Everyone is paired and online.
    struct Friends;

    impl Net for Friends {
        fn has_peers(&self) -> bool {
            true
        }
        fn holder(&self, _: DeviceKey) -> Holder {
            Holder::Online
        }
        fn paired(&self, _: DeviceKey) -> bool {
            true
        }
        fn away(&self, _: DeviceKey) -> Option<Duration> {
            // Everyone has been connected for a while.
            None
        }
    }

    /// A device `n` that has screens, as it would sign for itself.
    fn device(n: u8) -> Roles {
        let mut r = Roles::new(
            id(n),
            format!("Device {n}"),
            Default::default(),
            Duration::from_secs(120),
            Default::default(),
            Signer::new(move |m| kp(n).sign(m)),
        );
        r.set_screens(true);
        r
    }

    /// Device `n` takes `role` and shows the record it would send.
    fn claimed(n: u8, role: Role) -> (Roles, Record) {
        let mut d = device(n);
        if role == Role::Operator {
            // The Operator seat needs Projection held by somebody.
            d.claim(Role::Projection, &Strangers).unwrap();
        }
        let record = d.claim(role, &Strangers).unwrap();
        (d, record)
    }

    /// `holder` hands `role` to `to`, as it would when allowing an ask.
    fn handed(holder: &mut Roles, role: Role, to: u8) -> Record {
        holder.receive_ask(role, id(to), format!("Device {to}"));
        holder
            .allow(role, id(to), false, &Strangers)
            .unwrap()
            .record
    }

    #[test]
    fn a_command_counts_only_from_the_operator_and_only_where_projection_is_held() {
        let (node, commands, _, _dir) = node_with_hooks();
        let me = node.device();
        node.claim_role(Role::Projection).unwrap();
        paired(&node, 8);
        paired(&node, 7);
        let (mut seven, op) = claimed(7, Role::Operator);
        node.record_received(id(7), Role::Operator, op);
        node.drive_command_received(id(7), b"a".to_vec());
        node.drive_command_received(id(8), b"b".to_vec());
        node.drive_command_received(me, b"c".to_vec());
        assert_eq!(*lock(&commands), vec![id(7)], "only the Operator counts");
        // An Operator that is replaced, by the hand-over it signed, no
        // longer counts.
        let next = handed(&mut seven, Role::Operator, 8);
        node.record_received(id(7), Role::Operator, next);
        node.drive_command_received(id(7), b"d".to_vec());
        assert_eq!(lock(&commands).len(), 1);
        node.drive_command_received(id(8), b"e".to_vec());
        assert_eq!(*lock(&commands), vec![id(7), id(8)]);
    }

    #[test]
    fn a_command_is_carried_out_only_where_projection_is_held() {
        let (node, commands, _, _dir) = node_with_hooks();
        paired(&node, 7);
        paired(&node, 9);
        let (_, op) = claimed(7, Role::Operator);
        node.record_received(id(7), Role::Operator, op);
        // Projection is somebody else's.
        let (_, proj) = claimed(9, Role::Projection);
        node.record_received(id(9), Role::Projection, proj);
        node.drive_command_received(id(7), b"a".to_vec());
        assert!(lock(&commands).is_empty());
    }

    #[test]
    fn live_state_counts_only_from_the_projection_holder_this_device_operates() {
        let (node, _, live, _dir) = node_with_hooks();
        // The roles must have a paired device to mean anything.
        paired(&node, 9);
        let (_, proj) = claimed(9, Role::Projection);
        node.record_received(id(9), Role::Projection, proj);
        node.claim_role(Role::Operator).unwrap();
        node.drive_live_received(id(9), b"a".to_vec());
        node.drive_live_received(id(8), b"b".to_vec());
        assert_eq!(*lock(&live), vec![b"a".to_vec()]);
    }

    #[test]
    fn a_name_in_a_record_is_never_taken_from_the_sender() {
        let (node, _, _, _dir) = node_with_hooks();
        lock(&node.trust).add(id(7), "Mary's laptop", None).unwrap();
        lock(&node.roles).seed_from(id(7));
        let (mut seven, mut forged) = claimed(7, Role::Stream);
        forged.name = "The pastor's phone".into();
        node.record_received(id(7), Role::Stream, forged);
        assert_eq!(node.roles_status().stream.record.name, "Mary's laptop");
        // A holder it has no name for gets none.
        lock(&node.roles).seed_from(id(9));
        let (_, unknown) = claimed(9, Role::Audio);
        node.record_received(id(9), Role::Audio, unknown);
        assert_eq!(node.roles_status().audio.record.name, "");
        // And a release has no name.
        let released = seven.release(Role::Stream).unwrap();
        node.record_received(id(7), Role::Stream, released);
        assert_eq!(node.roles_status().stream.record.holder, None);
        assert_eq!(node.roles_status().stream.record.name, "");
    }

    #[test]
    fn a_record_with_an_epoch_nothing_could_beat_is_ignored() {
        let (node, _, _, _dir) = node_with_hooks();
        let (_, mut record) = claimed(7, Role::Projection);
        record.epoch = u64::MAX;
        node.record_received(id(7), Role::Projection, record);
        assert_eq!(node.roles_status().projection.record.epoch, 0);
    }

    #[test]
    fn a_record_without_steps_changes_nothing_here() {
        let (node, _, _, _dir) = node_with_hooks();
        node.claim_role(Role::Stream).unwrap();
        let before = node.roles_status().stream.record;
        node.record_received(
            id(7),
            Role::Stream,
            Record {
                holder: Some(id(7)),
                epoch: 9,
                ..Default::default()
            },
        );
        assert_eq!(node.roles_status().stream.record, before);
    }

    #[test]
    fn forged_records_from_a_paired_device_change_nothing() {
        let (node, _, _, _dir) = node_with_hooks();
        paired(&node, 7);
        // This device holds Stream, handed to it by a first claim.
        node.claim_role(Role::Stream).unwrap();
        let held = node.roles_status().stream.record;
        assert_eq!(held.epoch, 1);
        let forged_into = |role: Role, record: Record| {
            node.record_received(id(7), role, record);
            assert_eq!(node.roles_status().stream.record, held);
        };
        // A paired device says it holds it now, with nothing to show.
        forged_into(
            Role::Stream,
            Record {
                holder: Some(id(7)),
                epoch: 2,
                ..Default::default()
            },
        );
        // A paired device clears it, with nothing to show.
        forged_into(
            Role::Stream,
            Record {
                holder: None,
                epoch: 2,
                ..Default::default()
            },
        );
        // A genuine step of its own for another role, replayed here: the
        // signature covers the role.
        let (_, audio) = claimed(7, Role::Audio);
        forged_into(Role::Stream, audio.clone());
        // A claim at a later epoch, signed by a stranger, against the holder
        // this device can see: refused.
        let mut stranger = device(9);
        stranger.seed_from(node.device());
        stranger.merge(Role::Stream, &held, node.device(), &Friends);
        let take = stranger.claim(Role::Stream, &Strangers);
        // (the stranger sees the holder as gone and takes it)
        let take = take.unwrap();
        assert_eq!(take.epoch, 2);
        forged_into(Role::Stream, take);
        // A hand-over this device signed, with the holder swapped for the
        // paired device: the signature no longer fits.
        node.record_received(id(7), Role::Audio, audio);
        let mut swapped = node.roles_status().audio.record;
        swapped.holder = Some(id(7));
        swapped.chain[0].holder = Some(id(8));
        node.record_received(id(7), Role::Audio, swapped);
        assert_eq!(node.roles_status().audio.record.holder, Some(id(7)));
    }

    #[test]
    fn what_a_node_sends_is_what_another_node_would_take() {
        let (node, _, _, _dir) = node_with_hooks();
        node.claim_role(Role::Projection).unwrap();
        let frames = node.record_frames();
        assert_eq!(frames.len(), 4);
        let Frame::Role {
            steps,
            holder,
            epoch,
            ..
        } = &frames[0]
        else {
            panic!("a record")
        };
        assert_eq!((holder.is_some(), *epoch, steps.len()), (true, 1, 1));
        // Unheld roles carry no steps, and no name travels at all.
        let Frame::Role { steps, holder, .. } = &frames[2] else {
            panic!("a record")
        };
        assert!(holder.is_none() && steps.is_empty());
    }

    /// A claim signed by device `n` against the node's Stream, as `n` would
    /// have made it after learning the node holds it.
    fn claim_against(node: &SyncNode, n: u8) -> Record {
        let mut d = device(n);
        d.seed_from(node.device());
        let held = node.roles_status().stream.record;
        assert!(
            d.merge(Role::Stream, &held, node.device(), &Friends)
                .changed
        );
        // It sees the holder as long gone.
        struct Gone(DeviceKey);
        impl Net for Gone {
            fn has_peers(&self) -> bool {
                true
            }
            fn holder(&self, d: DeviceKey) -> Holder {
                if d == self.0 {
                    Holder::Offline(Duration::from_secs(999))
                } else {
                    Holder::Online
                }
            }
            fn paired(&self, _: DeviceKey) -> bool {
                true
            }
            fn away(&self, _: DeviceKey) -> Option<Duration> {
                None
            }
        }
        d.claim(Role::Stream, &Gone(node.device())).unwrap()
    }

    #[test]
    fn a_claim_over_a_live_connection_does_not_take_a_role_from_a_connected_holder() {
        let (node, _, _, _dir) = node_with_hooks();
        paired(&node, 7);
        node.claim_role(Role::Stream).unwrap();
        let claim = claim_against(&node, 7);
        // Device 7 was heard from just now: it was not away.
        node.heard(id(7));
        node.record_received(id(7), Role::Stream, claim.clone());
        assert_eq!(
            node.roles_status().stream.record.holder,
            Some(node.device())
        );
        assert!(node.roles_status().stream.notice.is_none());
    }

    #[test]
    fn after_a_restart_a_claimant_that_was_away_longer_than_the_takeover_time_is_believed() {
        let (node, _, _, dir) = node_with_hooks();
        paired(&node, 7);
        node.claim_role(Role::Stream).unwrap();
        let claim = claim_against(&node, 7);
        // What the last run kept: device 7 was last heard from ten minutes
        // ago. This run has heard nothing.
        let ten_minutes_ago = now_ms() - 600_000;
        saved::save_contacts(
            &dir.path().join("sync"),
            &HashMap::from([(id(7), ten_minutes_ago)]),
        )
        .unwrap();
        node.forget_this_runs_contacts();
        assert!(lock(&node.peers).last_contact.is_empty());
        node.record_received(id(7), Role::Stream, claim);
        let stream = node.roles_status().stream;
        assert_eq!(stream.record.holder, Some(id(7)), "it yields");
        assert!(stream.notice.is_some());
    }

    #[test]
    fn when_devices_were_last_heard_survives_a_restart() {
        let (node, _, _, dir) = node_with_hooks();
        paired(&node, 7);
        node.heard(id(7));
        node.save_contacts();
        let kept = saved::load_contacts(&dir.path().join("sync"));
        assert!(kept.contains_key(&id(7)));
        assert!(now_ms() - kept[&id(7)] < 5_000);
    }

    #[test]
    fn a_wall_clock_that_ran_while_the_process_slept_counts_as_time_away() {
        // The laptop slept: `Instant` stood still, the wall clock did not.
        let (node, _, _, _dir) = node_with_hooks();
        paired(&node, 7);
        node.claim_role(Role::Stream).unwrap();
        let claim = claim_against(&node, 7);
        node.heard(id(7));
        node.record_received(id(7), Role::Stream, claim.clone());
        assert_eq!(
            node.roles_status().stream.record.holder,
            Some(node.device())
        );
        node.skip_wall_clock(Duration::from_secs(600));
        let silence = {
            let peers = lock(&node.peers);
            node.silence_or_run(&peers, id(7))
        };
        assert!(silence >= Duration::from_secs(600), "{silence:?}");
        node.record_received(id(7), Role::Stream, claim);
        assert_eq!(node.roles_status().stream.record.holder, Some(id(7)));
    }

    #[test]
    fn nothing_known_about_when_a_device_was_heard_errs_towards_not_away() {
        let (node, _, _, _dir) = node_with_hooks();
        paired(&node, 7);
        let silence = {
            let peers = lock(&node.peers);
            assert_eq!(node.silence_in(&peers, id(7)), None);
            node.silence_or_run(&peers, id(7))
        };
        assert!(silence < Duration::from_secs(60), "{silence:?}");
        // So a claim from it is refused: no evidence of an absence.
        node.claim_role(Role::Stream).unwrap();
        let claim = claim_against(&node, 7);
        node.record_received(id(7), Role::Stream, claim);
        assert_eq!(
            node.roles_status().stream.record.holder,
            Some(node.device())
        );
    }

    #[test]
    fn unpairing_forgets_when_the_device_was_last_heard() {
        let (node, _, _, dir) = node_with_hooks();
        paired(&node, 7);
        node.heard(id(7));
        node.save_contacts();
        assert!(saved::load_contacts(&dir.path().join("sync")).contains_key(&id(7)));
        node.unpair(id(7)).unwrap();
        assert!(lock(&node.contacts).is_empty());
        assert!(lock(&node.peers).last_contact.is_empty());
        // The file follows (off the runtime when there is none, at once).
        assert!(saved::load_contacts(&dir.path().join("sync")).is_empty());
    }

    #[test]
    fn a_lost_ask_is_made_again_when_a_connection_opens() {
        let (node, _, _, _dir) = node_with_hooks();
        paired(&node, 7);
        let (_, held) = claimed(7, Role::Stream);
        node.record_received(id(7), Role::Stream, held);
        // A connection to device 7, which this device can see frames on.
        let (control, mut frames) = mpsc::unbounded_channel();
        lock(&node.peers).sessions.insert(
            id(7),
            Session {
                id: 1,
                client: id(7),
                address: None,
                control,
                stop: None,
                done: None,
            },
        );
        node.ask_role(Role::Stream).unwrap();
        assert!(matches!(
            frames.try_recv(),
            Ok(Control::Frame(Frame::RoleAsk { role: Role::Stream }))
        ));
        // The ask was lost with the connection; a new one opens. The ask is
        // still waiting, so it is sent again, and a device that is not
        // waiting on anything is sent nothing.
        node.session_opened(id(7));
        assert!(matches!(
            frames.try_recv(),
            Ok(Control::Frame(Frame::RoleAsk { role: Role::Stream }))
        ));
        node.cancel_role_ask(Role::Stream);
        while frames.try_recv().is_ok() {}
        node.session_opened(id(7));
        assert!(frames.try_recv().is_err());
    }

    #[test]
    fn roles_that_were_lost_are_flagged_and_any_paired_device_teaches_them_again() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sync")).unwrap();
        std::fs::write(dir.path().join("sync/roles.json"), "{broken").unwrap();
        let node = node_at(dir.path());
        let r = node.roles_status();
        assert!(r.projection.unverified && r.operator.unverified && r.stream.unverified);
        assert!(dir.path().join("sync/roles.json.bad").is_file());
        // A paired device that is not "just paired" can teach it again.
        lock(&node.trust).add(id(7), "Mary's laptop", None).unwrap();
        let (_, held) = claimed(7, Role::Stream);
        node.record_received(id(7), Role::Stream, held);
        let r = node.roles_status();
        assert_eq!(r.stream.record.holder, Some(id(7)));
        assert!(!r.stream.unverified);
        assert!(r.projection.unverified, "the others still wait");
    }
}
