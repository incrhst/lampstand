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
//! - **Scene cues.** The Projection holder sends the OBS scene of each item
//!   that goes live to the device that holds Stream (`ObsCue`, answered by
//!   `ObsCueAck`); see [`cue`]. Nobody holding Stream leaves the OBS duty
//!   with Projection.
//! - **Sound cues.** The Projection and Operator holders tell the device that
//!   holds Audio what to play, pause, stop or cut (`SoundCue`; a panic counts
//!   from any paired device), and the Audio holder tells every device what is
//!   playing (`SoundNow`) and why a sound did not play (`SoundProblem`, a
//!   reason from a fixed list); see [`sound_cue`]. Nothing is kept for an
//!   Audio holder that is away.
//! - **Blobs** are fetched by hash on request ([`SyncNode::fetch_blob`]),
//!   in 256 KiB chunks, and verified on arrival ([`BlobStore::put_verified`]).
//!
//! [`BlobStore::put_verified`]: crate::BlobStore::put_verified

mod cue;
mod discovery;
mod handshake;
mod saved;
mod secure;
mod session;
mod sound_cue;
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

pub use cue::{CueHook, CueRoute};
pub use sound_cue::{
    SoundCommand, SoundHook, SoundNow, SoundProblem, SoundReason, SoundRoute, MAX_ITEM, MAX_NAME,
};
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
    /// The takeover setting the holder last advertised, when it is known.
    pub holder_takeover: Option<Duration>,
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
    /// How long a device had been out of reach when its latest connection
    /// began, and when that was. A connection that opens soon after another
    /// one for the same device (two devices dialling each other at once, one
    /// connection dropped for the other) inherits it: the first connection's
    /// frames have just made the device "heard", which must not read as
    /// "never away".
    recent_away: HashMap<DeviceKey, (Duration, Instant)>,
    /// The network is cut (see [`SyncNode::set_offline`]): no connection may
    /// be registered.
    offline: bool,
    /// The takeover setting each paired device last advertised.
    takeover: HashMap<DeviceKey, Duration>,
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
    /// For tests: a device that is asleep hears nothing, so nothing counts as
    /// heard (see [`SyncNode::sleep`]).
    asleep: std::sync::atomic::AtomicBool,
    /// Tests: ignore asks that arrive.
    lose_asks: std::sync::atomic::AtomicBool,
    /// Which roles a connected device has told this one about in this run.
    heard_roles: Mutex<[bool; 4]>,
    /// Contact times are saved one at a time, newest last.
    contacts_save: Mutex<()>,
    /// Advertised takeover settings are saved one at a time.
    takeovers_save: Mutex<()>,
    /// Roles are saved one at a time, newest last.
    save_lock: Mutex<()>,
    /// What is on disk may be replaced (see [`saved::Loaded`]).
    persist_roles: bool,
    /// How long after a connection opens a claim made while apart is
    /// believed; one that arrives later cannot have been made out of reach.
    fresh_session: Mutex<Duration>,
    drive_hooks: Mutex<Option<Arc<DriveHooks>>>,
    /// The OBS scene cue kept for, or taken from, another device ([`cue`]).
    cue: Mutex<cue::CueState>,
    sound: Mutex<sound_cue::SoundState>,
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
    takeover: HashMap<DeviceKey, Duration>,
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

    fn takeover_of(&self, device: DeviceKey) -> Option<Duration> {
        // A paired device that has not said how long it waits (nothing saved,
        // or the file was lost) is waited for as long as any setting can be,
        // until it says: failing closed, so a claimant never claims sooner
        // than a holder it knows nothing about would allow.
        self.paired.contains(&device).then(|| {
            self.takeover
                .get(&device)
                .copied()
                .unwrap_or(crate::roles::MAX_TAKEOVER)
        })
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
            peers: Mutex::new(Peers {
                takeover: saved::load_takeovers(&dir)
                    .into_iter()
                    .map(|(d, secs)| (d, Duration::from_secs(u64::from(secs))))
                    .collect(),
                ..Peers::default()
            }),
            activity: Mutex::new(Activity::default()),
            changed: watch::Sender::new(0),
            port: Mutex::new(None),
            discovery: Mutex::new(None),
            roles: Mutex::new(roles),
            last_driven: Mutex::new(false),
            contacts: Mutex::new(saved::load_contacts(&dir)),
            contacts_dirty: std::sync::atomic::AtomicBool::new(false),
            wall_offset_ms: std::sync::atomic::AtomicI64::new(0),
            asleep: std::sync::atomic::AtomicBool::new(false),
            lose_asks: std::sync::atomic::AtomicBool::new(false),
            heard_roles: Mutex::new([false; 4]),
            contacts_save: Mutex::new(()),
            takeovers_save: Mutex::new(()),
            save_lock: Mutex::new(()),
            persist_roles: loaded.writable,
            fresh_session: Mutex::new(FRESH_SESSION),
            drive_hooks: Mutex::new(None),
            cue: Mutex::new(cue::CueState::new()),
            sound: Mutex::new(sound_cue::SoundState::default()),
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
            return Err(NetError::Unreachable(format!(
                "No computer nearby is showing a pairing code. Check that the other computer \
                 shows its code and is on the same network as this one.{LOCAL_NETWORK_HINT}"
            )));
        }
        // The addresses come most likely first (discovery's `reach_order`),
        // so the first failure is the one worth reporting.
        let mut first = None;
        for target in targets {
            match self.dial(&target, Some(&code)).await {
                Ok((peer, _session)) => return Ok(self.peer_status(peer.device)),
                Err(e) => {
                    first.get_or_insert(e);
                }
            }
        }
        Err(first.expect("at least one target"))
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
            peers.recent_away.remove(&device);
            peers.takeover.remove(&device);
        }
        self.save_takeovers_in_background();
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

    /// Cuts this node off, or reconnects it. While offline every connection
    /// is closed and none may be registered: a connection that was being
    /// accepted or dialled when this was called is dropped when it tries to
    /// register, under the same lock that closed the others, so nothing
    /// survives a cut. Diallers keep trying and succeed once it is back.
    #[doc(hidden)]
    pub fn set_offline(&self, offline: bool) {
        {
            let mut peers = lock(&self.peers);
            peers.offline = offline;
            if offline {
                for session in peers.sessions.values_mut() {
                    if let Some(stop) = session.stop.take() {
                        let _ = stop.send(());
                    }
                }
            }
        }
        self.bump();
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
            .map_err(|e| cannot_reach(address, &e))?;
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

    /// Notes, as a connection for `device` opens, how long it had been out
    /// of reach (from what was heard before this connection, kept across a
    /// restart; nothing known counts as not away at all, so a claim made on
    /// no evidence is refused however long this process has run) and when
    /// the connection began.
    ///
    /// A connection that opens within the fresh-session window of an earlier
    /// one for the same device (two devices dialling each other at once, one
    /// connection dropped for the other) inherits the earlier away time: the
    /// first connection's frames have just made the device "heard", which
    /// must not read as "never away". The inheritance keeps the *original*
    /// time stamp, so it lasts at most one fresh-session window from the
    /// connection that really measured it; a device that drops and redials
    /// again and again cannot keep the window open.
    fn note_session_begin(&self, peers: &mut Peers, device: DeviceKey) {
        let now = Instant::now();
        let measured = self.silence_in(peers, device).unwrap_or(Duration::ZERO);
        let window = *lock(&self.fresh_session);
        let (away, since) = match peers.recent_away.get(&device) {
            Some((before, at)) if at.elapsed() <= window => (measured.max(*before), *at),
            _ => (measured, now),
        };
        peers.recent_away.insert(device, (away, since));
        peers.away_before.insert(device, away);
        peers.session_started.insert(device, now);
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
                if peers.offline {
                    return None;
                }
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
                        // restart). Nothing known: not away at all, so a
                        // claim made on no evidence is refused rather than
                        // believed, however long this process has run.
                        self.note_session_begin(&mut peers, peer.device);
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
            // Not stamped with "now": when the device was last heard from
            // is when its last frame came. A laptop that slept with its
            // connection open is noticed long after it last spoke, and the
            // time since then is what counts.
            peers.session_started.remove(&device);
            // Under the peers lock, so a new session cannot start before
            // the old state is gone.
            self.repo.forget_peer(device);
            drop(peers);
            if lock(&self.trust).is_paired(device) {
                self.save_contacts_in_background();
            }
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
            takeover: peers.takeover.clone(),
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
            // What it advertised, not the longest-possible stand-in used for a
            // device that has not (see `Reach::takeover_of`).
            holder_takeover: r
                .record(role)
                .holder
                .and_then(|h| reach.takeover.get(&h).copied()),
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
        let record = {
            let mut roles = lock(&self.roles);
            // Just connected and not yet told who holds what: a role that
            // looks unheld may be held, and taking it now would be a guess.
            if roles.standing(role, &reach) == Standing::Open && self.settling(&reach) {
                return Err(Denied::Settling);
            }
            roles.claim(role, &reach)?
        };
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
        // The devices that would claim from this one wait by it too.
        let frame = self.takeover_frame();
        for s in lock(&self.peers).sessions.values() {
            let _ = s.control.send(Control::Frame(frame.clone()));
        }
        self.bump();
    }

    /// The takeover setting a paired device last advertised (tests).
    #[doc(hidden)]
    pub fn advertised_takeover(&self, device: DeviceKey) -> Option<Duration> {
        lock(&self.peers).takeover.get(&device).copied()
    }

    /// This device's takeover setting, for the devices it is connected to.
    pub(crate) fn takeover_frame(&self) -> Frame {
        let secs = lock(&self.roles).takeover_after().as_secs();
        Frame::Takeover {
            secs: u32::try_from(secs).unwrap_or(u32::MAX),
        }
    }

    /// A paired device says how long it waits before taking a role from a
    /// device that has gone. Kept (within the limits of the setting) so this
    /// device waits at least as long before claiming from it.
    pub(crate) fn takeover_received(&self, from: DeviceKey, secs: u32) {
        // A device that was unpaired while its connection was still closing
        // is not remembered (as in `heard`).
        if !lock(&self.trust).is_paired(from) {
            return;
        }
        let setting = Duration::from_secs(u64::from(secs))
            .clamp(crate::roles::MIN_TAKEOVER, crate::roles::MAX_TAKEOVER);
        let changed = lock(&self.peers).takeover.insert(from, setting) != Some(setting);
        if changed {
            self.save_takeovers_in_background();
            self.bump();
        }
    }

    /// Saves the advertised settings (kept so a restart while the holder is
    /// away still waits by them), off the async runtime when there is one.
    fn save_takeovers_in_background(&self) {
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                let node = self.arc();
                runtime.spawn_blocking(move || node.save_takeovers());
            }
            Err(_) => self.save_takeovers(),
        }
    }

    fn save_takeovers(&self) {
        // One at a time, and the snapshot is taken under the same lock the
        // write is made under, so an older one cannot land after a newer one.
        let _one_at_a_time = lock(&self.takeovers_save);
        let paired: HashSet<DeviceKey> = lock(&self.trust)
            .devices()
            .iter()
            .map(|d| d.device)
            .collect();
        let list: HashMap<DeviceKey, u32> = lock(&self.peers)
            .takeover
            .iter()
            .filter(|(d, _)| paired.contains(d))
            .map(|(d, t)| (*d, u32::try_from(t.as_secs()).unwrap_or(u32::MAX)))
            .collect();
        if let Err(e) = saved::save_takeovers(&self.dir, &list) {
            eprintln!("lampstand: cannot save the takeover settings: {e}");
        }
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
        self.flush_scene_cue();
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
        // A cue kept for this device while it was away.
        self.flush_scene_cue();
        self.sound_session_opened(peer);
    }

    /// A device sent its record for a role: the newer of the two stands.
    /// Whether this device has been told about every role by a connected
    /// device (or is connected to none): an unheld role is then really
    /// unheld as far as anyone here knows.
    pub fn settled(&self) -> bool {
        !self.settling(&self.reach())
    }

    /// Connected to someone who has not yet told this device about every
    /// role (the four records come together as a connection opens).
    fn settling(&self, reach: &Reach) -> bool {
        !reach.connected.is_empty() && !lock(&self.heard_roles).iter().all(|h| *h)
    }

    pub(crate) fn record_received(&self, from: DeviceKey, role: Role, mut record: Record) {
        lock(&self.heard_roles)[role.index()] = true;
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
        let (merged, was_reseeding, now_reseeding) = {
            let mut roles = lock(&self.roles);
            let was = roles.reseeding();
            let merged = roles.merge(role, &record, from, &reach);
            (merged, was, roles.reseeding())
        };
        if was_reseeding && !now_reseeding && !merged.changed {
            // Taught that nothing is held: the marker on disk goes.
            self.save_roles();
        }
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

    /// For tests: asks that arrive are ignored, as if the frames were lost.
    #[doc(hidden)]
    pub fn lose_asks(&self, lose: bool) {
        self.lose_asks
            .store(lose, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn role_ask_received(&self, from: DeviceKey, role: Role) {
        if self.lose_asks.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
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
        // A device that was unpaired while its connection was still closing
        // is not remembered.
        if !lock(&self.trust).is_paired(device) {
            return;
        }
        if self.asleep.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
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
        let by_wall = lock(&self.contacts).get(&device).map(|ms| {
            Duration::from_millis(u64::try_from(self.wall_now().saturating_sub(*ms)).unwrap_or(0))
        });
        match (by_run, by_wall) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        }
    }

    /// [`SyncNode::silence_in`], or, when nothing is known, how long this
    /// run has lasted: a device never heard from in this run is taken to
    /// have been silent since it began, so a holder that has never been heard
    /// can be taken over from after the takeover time, and not before. (How
    /// long a claimant was away, for a claim against this device, is
    /// different: nothing known counts as not away at all.)
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

    /// For tests: the device falls asleep (`true`) or wakes (`false`). While
    /// it sleeps no frame counts as heard, as a laptop with its lid shut
    /// hears none, so a beat that lands between the wall clock jumping and
    /// the connection ending cannot make the device look as if it had just
    /// been in touch.
    #[doc(hidden)]
    pub fn sleep(&self, asleep: bool) {
        self.asleep
            .store(asleep, std::sync::atomic::Ordering::Relaxed);
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
        let mut contacts = lock(&self.contacts).clone();
        {
            let trust = lock(&self.trust);
            contacts.retain(|d, _| trust.is_paired(*d));
        }
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
        // Whatever a background save has not written yet is written now.
        self.save_contacts();
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

/// On a Mac, where Local Network permission blocks other computers without
/// a word, where to turn it on.
#[cfg(target_os = "macos")]
const LOCAL_NETWORK_HINT: &str = " On a Mac, Lampstand needs Local Network access: System \
    Settings › Privacy & Security › Local Network.";
#[cfg(not(target_os = "macos"))]
const LOCAL_NETWORK_HINT: &str = "";

/// A failed connection to `address`, in words that say what to check.
fn cannot_reach(address: &str, e: &tokio_tungstenite::tungstenite::Error) -> NetError {
    use std::io::ErrorKind;
    let io = match e {
        tokio_tungstenite::tungstenite::Error::Io(io) => Some(io.kind()),
        _ => None,
    };
    NetError::Unreachable(match io {
        Some(ErrorKind::HostUnreachable | ErrorKind::NetworkUnreachable) => format!(
            "Cannot reach {address}: there is no route to it. Check that both computers \
             are on the same network.{LOCAL_NETWORK_HINT}"
        ),
        Some(ErrorKind::ConnectionRefused) => format!(
            "Nothing answered at {address}. Check that Lampstand is open on that computer \
             and that the address is right."
        ),
        _ => format!("Cannot reach {address}: {e}"),
    })
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
        node_with_key(dir, DeviceKeypair::generate())
    }

    /// The same node again, as after a restart.
    fn node_with_key(dir: &std::path::Path, key: DeviceKeypair) -> Arc<SyncNode> {
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
        let (node, _, _, _dir) = node_with_hooks();
        paired(&node, 7);
        node.claim_role(Role::Stream).unwrap();
        let claim = claim_against(&node, 7);
        // What the last run kept: device 7 was last heard from ten minutes
        // ago. This run has heard nothing.
        let ten_minutes_ago = now_ms() - 600_000;
        lock(&node.contacts).insert(id(7), ten_minutes_ago);
        node.forget_this_runs_contacts();
        assert!(lock(&node.peers).last_contact.is_empty());
        node.record_received(id(7), Role::Stream, claim);
        let stream = node.roles_status().stream;
        assert_eq!(stream.record.holder, Some(id(7)), "it yields");
        assert!(stream.notice.is_some());
    }

    /// Opens a connection for device 7 as `begin` does, and returns how long
    /// it had been out of reach.
    fn begin_for_7(node: &SyncNode) -> Duration {
        let mut peers = lock(&node.peers);
        node.note_session_begin(&mut peers, id(7));
        peers.away_before[&id(7)]
    }

    #[test]
    fn a_connection_that_replaces_another_keeps_how_long_the_device_was_away() {
        let (node, _, _, _dir) = node_with_hooks();
        paired(&node, 7);
        // Last heard a minute ago: the first connection measures that.
        lock(&node.contacts).insert(id(7), now_ms() - 60_000);
        node.forget_this_runs_contacts();
        let first = begin_for_7(&node);
        assert!(first >= Duration::from_secs(60), "{first:?}");
        // Its frames make the device "heard"; a second connection opens at
        // once (the first was dropped for it) and must not read as no time.
        node.heard(id(7));
        assert!(begin_for_7(&node) >= Duration::from_secs(60));
    }

    #[test]
    fn a_device_that_was_never_away_stays_never_away() {
        let (node, _, _, _dir) = node_with_hooks();
        paired(&node, 7);
        node.heard(id(7));
        assert!(begin_for_7(&node) < Duration::from_secs(2));
        node.heard(id(7));
        assert!(begin_for_7(&node) < Duration::from_secs(2));
    }

    #[test]
    fn dropping_and_redialling_cannot_keep_the_window_open() {
        let (node, _, _, _dir) = node_with_hooks();
        paired(&node, 7);
        node.set_fresh_session(Duration::from_millis(300));
        lock(&node.contacts).insert(id(7), now_ms() - 60_000);
        node.forget_this_runs_contacts();
        assert!(begin_for_7(&node) >= Duration::from_secs(60));
        // A flapping device redials every 200 ms: the first redial inherits,
        // but the inheritance is stamped from the connection that measured
        // it, so a few redials later it has run out.
        for _ in 0..4 {
            std::thread::sleep(Duration::from_millis(200));
            node.heard(id(7));
            begin_for_7(&node);
        }
        std::thread::sleep(Duration::from_millis(200));
        node.heard(id(7));
        assert!(
            begin_for_7(&node) < Duration::from_secs(2),
            "the window must not renew itself"
        );
    }

    #[test]
    fn an_offline_node_registers_no_connection() {
        let (node, _, _, _dir) = node_with_hooks();
        node.set_offline(true);
        assert!(lock(&node.peers).offline);
        node.set_offline(false);
        assert!(!lock(&node.peers).offline);
    }

    #[test]
    fn a_claimant_that_restarted_while_the_holder_was_away_still_waits_by_its_setting() {
        let dir = tempfile::tempdir().unwrap();
        let key = || DeviceKeypair::from_secret_bytes([42; 32]);
        let node = node_with_key(dir.path(), key());
        lock(&node.trust).add(id(7), "Mary's laptop", None).unwrap();
        // Before it has said anything, a paired device is waited for as long
        // as any setting can be: fail closed.
        assert_eq!(
            node.reach().takeover_of(id(7)),
            Some(crate::roles::MAX_TAKEOVER)
        );
        node.takeover_received(id(7), 300);
        assert_eq!(
            node.reach().takeover_of(id(7)),
            Some(Duration::from_secs(300))
        );
        // The device is away and this one restarts: it still knows.
        drop(node);
        let node = node_with_key(dir.path(), key());
        assert_eq!(
            node.reach().takeover_of(id(7)),
            Some(Duration::from_secs(300))
        );
        // Unpairing trims it, in memory and on disk.
        node.unpair(id(7)).unwrap();
        assert_eq!(node.reach().takeover_of(id(7)), None);
        assert!(saved::load_takeovers(&dir.path().join("sync")).is_empty());
        drop(node);
        let node = node_with_key(dir.path(), key());
        assert!(lock(&node.peers).takeover.is_empty());
    }

    #[test]
    fn a_takeover_setting_from_a_device_that_is_not_paired_is_not_kept() {
        let (node, _, _, dir) = node_with_hooks();
        node.takeover_received(id(9), 30);
        assert!(lock(&node.peers).takeover.is_empty());
        assert!(!dir.path().join("sync/takeover.json").exists());
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
        // A claim from it is refused, however long this process has run:
        // no evidence of an absence counts as no absence.
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

    #[test]
    fn roles_that_were_lost_stay_lost_across_restarts_until_they_are_taught() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sync")).unwrap();
        std::fs::write(dir.path().join("sync/roles.json"), "{broken").unwrap();
        // Older files that must not come back.
        std::fs::write(
            dir.path().join("sync/presenter.json"),
            r#"{"lease":{"holder":null,"name":"Old","epoch":9},"takeover_secs":300}"#,
        )
        .unwrap();
        let key = || DeviceKeypair::from_secret_bytes([42; 32]);
        for run in 0..3 {
            let node = node_with_key(dir.path(), key());
            let r = node.roles_status();
            assert!(r.stream.unverified && r.projection.unverified, "run {run}");
            assert_eq!(r.projection.record.epoch, 0, "the old lease is not read");
            assert_eq!(r.takeover_after, Duration::from_secs(120));
        }
        // Taught: the device it is paired with speaks about every role.
        let node = node_with_key(dir.path(), key());
        lock(&node.trust).add(id(7), "Mary's laptop", None).unwrap();
        let (_, held) = claimed(7, Role::Stream);
        node.record_received(id(7), Role::Stream, held);
        for role in [Role::Projection, Role::Operator, Role::Audio] {
            node.record_received(id(7), role, Record::default());
        }
        let r = node.roles_status();
        assert!(!r.stream.unverified && !r.projection.unverified && !r.audio.unverified);
        drop(node);
        let node = node_with_key(dir.path(), key());
        let r = node.roles_status();
        assert!(!r.stream.unverified && !r.projection.unverified);
        assert_eq!(r.stream.record.holder, Some(id(7)));
    }

    #[test]
    fn a_roles_json_set_aside_with_no_new_one_written_is_still_lost_on_the_next_start() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sync")).unwrap();
        std::fs::write(dir.path().join("sync/roles.json.bad"), "{broken").unwrap();
        std::fs::write(
            dir.path().join("sync/presenter.json"),
            r#"{"lease":{"holder":null,"name":"Old","epoch":9},"takeover_secs":300}"#,
        )
        .unwrap();
        let node = node_at(dir.path());
        assert!(node.roles_status().stream.unverified);
        assert_eq!(node.roles_status().projection.record.epoch, 0);
    }

    #[test]
    fn a_device_that_has_just_connected_does_not_take_a_role_it_has_not_been_told_about() {
        let (node, _, _, _dir) = node_with_hooks();
        paired(&node, 7);
        let (control, _frames) = mpsc::unbounded_channel();
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
        // Connected to device 7, which has not said anything yet.
        assert!(!node.settled());
        assert_eq!(node.claim_role(Role::Stream), Err(Denied::Settling));
        assert_eq!(node.roles_status().stream.record.holder, None);
        // Device 7 tells it about the four roles (it holds none).
        for role in Role::ALL {
            node.record_received(id(7), role, Record::default());
        }
        assert!(node.settled());
        assert!(node.claim_role(Role::Stream).is_ok());
    }

    // ---- Sound cues (the Audio holder plays) --------------------------------

    fn taking_sound(node: &SyncNode) -> Arc<Mutex<Vec<SoundCommand>>> {
        let taken = Arc::new(Mutex::new(Vec::new()));
        let t = taken.clone();
        node.set_sound_hook(Arc::new(move |cmd| lock(&t).push(cmd)));
        taken
    }

    fn sound_in(frames: &mut mpsc::UnboundedReceiver<Control>) -> Vec<Frame> {
        std::iter::from_fn(|| frames.try_recv().ok())
            .filter_map(|c| match c {
                Control::Frame(
                    f @ (Frame::SoundCue { .. }
                    | Frame::SoundNow { .. }
                    | Frame::SoundProblem { .. }),
                ) => Some(f),
                _ => None,
            })
            .collect()
    }

    fn play() -> SoundCommand {
        SoundCommand::Play {
            file: [3; 32],
            name: "Walk-in music".into(),
            item: "item-1".into(),
        }
    }

    fn now(name: &str) -> SoundNow {
        SoundNow {
            item: "item-1".into(),
            name: name.into(),
            paused: false,
            ending: false,
            position_ms: 2000,
            length_ms: 60_000,
        }
    }

    #[test]
    fn sound_plays_here_when_alone_or_when_this_device_holds_audio() {
        let (node, _, _, _dir) = node_with_hooks();
        assert_eq!(node.route_sound_cue(play()), SoundRoute::Local);
        paired(&node, 7);
        heard_roles_from(&node, 7);
        node.claim_role(Role::Audio).unwrap();
        assert!(node.audio_here() && node.audio_elsewhere().is_none());
        assert_eq!(node.route_sound_cue(SoundCommand::Panic), SoundRoute::Local);
    }

    #[test]
    fn a_cue_goes_to_the_audio_holder_and_never_waits_for_one_that_is_away() {
        let (node, _, _, _dir) = node_with_hooks();
        paired(&node, 7);
        node.set_screens(true);
        node.claim_role(Role::Projection).unwrap();
        // Nobody holds Audio: the sound plays nowhere.
        assert_eq!(node.route_sound_cue(play()), SoundRoute::Nobody);
        let (_, held) = claimed(7, Role::Audio);
        node.record_received(id(7), Role::Audio, held);
        assert_eq!(node.audio_elsewhere().map(|a| a.0), Some(id(7)));
        // The holder is away: told so, nothing sent, nothing kept.
        assert_eq!(node.route_sound_cue(play()), SoundRoute::Away);
        let mut frames = connected_to(&node, 7);
        node.session_opened(id(7));
        assert!(
            sound_in(&mut frames).is_empty(),
            "a late cue is never played"
        );
        // Connected: it goes.
        assert_eq!(node.route_sound_cue(play()), SoundRoute::Sent);
        assert_eq!(sound_in(&mut frames), vec![Frame::SoundCue { cmd: play() }]);
        assert_eq!(
            node.route_sound_cue(SoundCommand::Stop { fade_ms: 2000 }),
            SoundRoute::Sent
        );
    }

    #[test]
    fn only_projection_or_operator_may_start_but_anyone_paired_may_panic() {
        let (node, _, _, _dir) = node_with_hooks();
        paired(&node, 7);
        let (_, held) = claimed(7, Role::Audio);
        node.record_received(id(7), Role::Audio, held);
        let mut frames = connected_to(&node, 7);
        // This device holds neither Projection nor Operator.
        for cmd in [
            play(),
            SoundCommand::Pause,
            SoundCommand::Resume,
            SoundCommand::Stop { fade_ms: 100 },
        ] {
            assert_eq!(node.route_sound_cue(cmd), SoundRoute::NotAllowed);
        }
        assert!(sound_in(&mut frames).is_empty());
        assert_eq!(node.route_sound_cue(SoundCommand::Panic), SoundRoute::Sent);
        assert_eq!(
            sound_in(&mut frames),
            vec![Frame::SoundCue {
                cmd: SoundCommand::Panic
            }]
        );
        // A name or fade the other side would not believe is not sent.
        heard_roles_from(&node, 7);
        node.set_screens(true);
        node.claim_role(Role::Projection).unwrap();
        let long = SoundCommand::Play {
            file: [1; 32],
            name: "x".repeat(sound_cue::MAX_NAME + 1),
            item: String::new(),
        };
        assert_eq!(node.route_sound_cue(long), SoundRoute::NotAllowed);
        let slow = SoundCommand::Stop {
            fade_ms: sound_cue::MAX_FADE_MS + 1,
        };
        assert_eq!(node.route_sound_cue(slow), SoundRoute::NotAllowed);
        assert!(sound_in(&mut frames).is_empty());
    }

    #[test]
    fn the_audio_holder_obeys_projection_and_operator_and_a_panic_from_any_paired_device() {
        let (node, _, _, _dir) = node_with_hooks();
        let taken = taking_sound(&node);
        paired(&node, 7);
        paired(&node, 8);
        paired(&node, 9);
        let _frames = connected_to(&node, 7);
        heard_roles_from(&node, 7);
        node.claim_role(Role::Audio).unwrap();
        let (_, projection) = claimed(7, Role::Projection);
        node.record_received(id(7), Role::Projection, projection);
        let (_, operator) = claimed(8, Role::Operator);
        node.record_received(id(8), Role::Operator, operator);
        // Projection and Operator may start.
        node.sound_cue_received(id(7), play());
        node.sound_cue_received(id(8), SoundCommand::Pause);
        assert_eq!(*lock(&taken), vec![play(), SoundCommand::Pause]);
        // A paired device with no role may not start or control, but may
        // panic.
        node.sound_cue_received(id(9), play());
        node.sound_cue_received(id(9), SoundCommand::Resume);
        assert_eq!(lock(&taken).len(), 2);
        node.sound_cue_received(id(9), SoundCommand::Panic);
        assert_eq!(lock(&taken).last(), Some(&SoundCommand::Panic));
        // Strangers and this device's own id count for nothing, even to
        // panic.
        for from in [id(99), node.device()] {
            node.sound_cue_received(from, SoundCommand::Panic);
            node.sound_cue_received(from, play());
        }
        assert_eq!(lock(&taken).len(), 3);
        // Cues that are not believable are not acted on.
        node.sound_cue_received(
            id(7),
            SoundCommand::Stop {
                fade_ms: sound_cue::MAX_FADE_MS + 1,
            },
        );
        node.sound_cue_received(
            id(7),
            SoundCommand::Play {
                file: [0; 32],
                name: String::new(),
                item: "i".repeat(sound_cue::MAX_ITEM + 1),
            },
        );
        assert_eq!(lock(&taken).len(), 3);
    }

    #[test]
    fn a_sound_cue_is_ignored_where_audio_is_not_held() {
        let (node, _, _, _dir) = node_with_hooks();
        let taken = taking_sound(&node);
        paired(&node, 7);
        let (_, projection) = claimed(7, Role::Projection);
        node.record_received(id(7), Role::Projection, projection);
        // Nobody holds Audio.
        node.sound_cue_received(id(7), play());
        node.sound_cue_received(id(7), SoundCommand::Panic);
        // Another device holds it.
        lock(&node.trust).add(id(9), "Sound desk", None).unwrap();
        let (_, held) = claimed(9, Role::Audio);
        node.record_received(id(9), Role::Audio, held);
        node.sound_cue_received(id(7), play());
        node.sound_cue_received(id(7), SoundCommand::Panic);
        assert!(lock(&taken).is_empty());
    }

    #[test]
    fn what_is_playing_is_believed_only_from_the_connected_audio_holder() {
        let (node, _, _, _dir) = node_with_hooks();
        paired(&node, 7);
        paired(&node, 8);
        let (_, held) = claimed(7, Role::Audio);
        node.record_received(id(7), Role::Audio, held);
        let _frames = connected_to(&node, 7);
        // A device that does not hold Audio is not believed.
        node.sound_now_received(id(8), Some(now("Forged")));
        assert!(node.sound_elsewhere().is_none());
        node.sound_now_received(id(7), Some(now("Walk-in music")));
        let (who, playing, _) = node.sound_elsewhere().unwrap();
        assert_eq!(playing, now("Walk-in music"));
        assert_eq!(who, node.audio_elsewhere().unwrap().1);
        // It says it stopped.
        node.sound_now_received(id(7), None);
        assert!(node.sound_elsewhere().is_none());
        // A name that is too long is not believed.
        node.sound_now_received(id(7), Some(now(&"x".repeat(sound_cue::MAX_NAME + 1))));
        assert!(node.sound_elsewhere().is_none());
        // It goes with the connection.
        node.sound_now_received(id(7), Some(now("Walk-in music")));
        assert!(node.sound_elsewhere().is_some());
        lock(&node.peers).sessions.remove(&id(7));
        assert!(node.sound_elsewhere().is_none());
    }

    #[test]
    fn the_audio_holder_tells_every_device_what_is_playing_and_each_one_that_connects() {
        let (node, _, _, _dir) = node_with_hooks();
        paired(&node, 7);
        paired(&node, 8);
        let mut seven = connected_to(&node, 7);
        heard_roles_from(&node, 7);
        node.claim_role(Role::Audio).unwrap();
        node.report_sound_now(Some(now("Walk-in music")));
        assert_eq!(
            sound_in(&mut seven),
            vec![Frame::SoundNow {
                now: Some(now("Walk-in music"))
            }]
        );
        // The same state again says nothing new.
        node.report_sound_now(Some(now("Walk-in music")));
        assert!(sound_in(&mut seven).is_empty());
        // A device that connects later is told.
        let mut eight = connected_to(&node, 8);
        node.session_opened(id(8));
        assert_eq!(
            sound_in(&mut eight),
            vec![Frame::SoundNow {
                now: Some(now("Walk-in music"))
            }]
        );
        node.report_sound_now(None);
        assert_eq!(sound_in(&mut seven), vec![Frame::SoundNow { now: None }]);
        // Not told once this device no longer holds Audio.
        lock(&node.roles).release(Role::Audio);
        node.report_sound_now(Some(now("Again")));
        assert!(sound_in(&mut seven).is_empty());
    }

    fn trouble(reason: SoundReason) -> SoundProblem {
        SoundProblem {
            reason,
            item: "item-1".into(),
            name: "Walk-in music".into(),
        }
    }

    #[test]
    fn why_a_sound_did_not_play_is_believed_only_from_the_connected_audio_holder() {
        let (node, _, _, _dir) = node_with_hooks();
        paired(&node, 7);
        paired(&node, 8);
        let (_, held) = claimed(7, Role::Audio);
        node.record_received(id(7), Role::Audio, held);
        let _frames = connected_to(&node, 7);
        // A device that does not hold Audio is not believed.
        node.sound_problem_received(id(8), Some(trouble(SoundReason::OutputDown)));
        assert!(node.sound_problem_elsewhere().is_none());
        node.sound_problem_received(id(7), Some(trouble(SoundReason::NotOnDevice)));
        let (who, problem) = node.sound_problem_elsewhere().unwrap();
        assert_eq!(problem, trouble(SoundReason::NotOnDevice));
        assert_eq!(who, node.audio_elsewhere().unwrap().1);
        // It says the trouble is over.
        node.sound_problem_received(id(7), None);
        assert!(node.sound_problem_elsewhere().is_none());
        // A name or an id that is too long is not believed.
        let mut long = trouble(SoundReason::Unsupported);
        long.name = "x".repeat(sound_cue::MAX_NAME + 1);
        node.sound_problem_received(id(7), Some(long));
        let mut long = trouble(SoundReason::Unsupported);
        long.item = "i".repeat(sound_cue::MAX_ITEM + 1);
        node.sound_problem_received(id(7), Some(long));
        assert!(node.sound_problem_elsewhere().is_none());
        // Said again, unchanged: nothing new.
        node.sound_problem_received(id(7), Some(trouble(SoundReason::Unreadable)));
        assert!(node.sound_problem_elsewhere().is_some());
        node.sound_problem_received(id(7), None);
        // Left over from a device that no longer holds Audio: forgotten for
        // good, so it cannot come back if that device holds Audio again.
        lock(&node.sound).problem_theirs = Some((id(8), trouble(SoundReason::NoOutput)));
        assert!(node.sound_problem_elsewhere().is_none());
        assert!(lock(&node.sound).problem_theirs.is_none());
        // It goes with the connection, and is not brought back by a
        // reconnection until the holder says it again.
        node.sound_problem_received(id(7), Some(trouble(SoundReason::Unreadable)));
        assert!(node.sound_problem_elsewhere().is_some());
        lock(&node.peers).sessions.remove(&id(7));
        assert!(node.sound_problem_elsewhere().is_none());
        let _again = connected_to(&node, 7);
        node.session_opened(id(7));
        assert!(node.sound_problem_elsewhere().is_none());
    }

    #[test]
    fn the_audio_holder_tells_every_device_why_a_sound_did_not_play_and_each_one_that_connects() {
        let (node, _, _, _dir) = node_with_hooks();
        paired(&node, 7);
        paired(&node, 8);
        let mut seven = connected_to(&node, 7);
        heard_roles_from(&node, 7);
        node.claim_role(Role::Audio).unwrap();
        // Nothing wrong at the start is not news.
        node.report_sound_problem(None);
        assert!(sound_in(&mut seven).is_empty());
        let problem = trouble(SoundReason::OutputDown);
        node.report_sound_problem(Some(problem.clone()));
        assert_eq!(
            sound_in(&mut seven),
            vec![Frame::SoundProblem {
                problem: Some(problem.clone())
            }]
        );
        // The same again says nothing new.
        node.report_sound_problem(Some(problem.clone()));
        assert!(sound_in(&mut seven).is_empty());
        // A device that connects later is told.
        let mut eight = connected_to(&node, 8);
        node.session_opened(id(8));
        assert_eq!(
            sound_in(&mut eight),
            vec![Frame::SoundProblem {
                problem: Some(problem)
            }]
        );
        node.report_sound_problem(None);
        assert_eq!(
            sound_in(&mut seven),
            vec![Frame::SoundProblem { problem: None }]
        );
        // A device that connects when nothing is wrong hears nothing.
        let mut nine = connected_to(&node, 8);
        node.session_opened(id(8));
        assert!(sound_in(&mut nine).is_empty());
        // Not told once this device no longer holds Audio.
        lock(&node.roles).release(Role::Audio);
        node.report_sound_problem(Some(trouble(SoundReason::NoOutput)));
        assert!(sound_in(&mut seven).is_empty());
    }

    // ---- Scene cues (the Stream holder owns OBS) ----------------------------

    /// Records the scenes the hook is given.
    fn taking_cues(node: &SyncNode) -> Arc<Mutex<Vec<Option<String>>>> {
        let taken = Arc::new(Mutex::new(Vec::new()));
        let t = taken.clone();
        node.set_scene_cue_hook(Arc::new(move |scene| lock(&t).push(scene)));
        taken
    }

    /// A connection to device `n`, with the frames sent on it.
    fn connected_to(node: &SyncNode, n: u8) -> mpsc::UnboundedReceiver<Control> {
        let (control, frames) = mpsc::unbounded_channel();
        lock(&node.peers).sessions.insert(
            id(n),
            Session {
                id: 1,
                client: id(n),
                address: None,
                control,
                stop: None,
                done: None,
            },
        );
        frames
    }

    /// Device `n` has told this one about the four roles (it holds none), so
    /// the node is settled and may claim.
    fn heard_roles_from(node: &SyncNode, n: u8) {
        for role in Role::ALL {
            node.record_received(id(n), role, Record::default());
        }
        assert!(node.settled());
    }

    fn cues_in(frames: &mut mpsc::UnboundedReceiver<Control>) -> Vec<Frame> {
        std::iter::from_fn(|| frames.try_recv().ok())
            .filter_map(|c| match c {
                Control::Frame(f @ (Frame::ObsCue { .. } | Frame::ObsCueAck { .. })) => Some(f),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_scene_cue_counts_only_from_the_projection_holder_at_the_stream_holder() {
        let (node, _, _, _dir) = node_with_hooks();
        let taken = taking_cues(&node);
        paired(&node, 8);
        paired(&node, 7);
        let mut frames = connected_to(&node, 7);
        heard_roles_from(&node, 7);
        node.claim_role(Role::Stream).unwrap();
        let (_, projection) = claimed(7, Role::Projection);
        node.record_received(id(7), Role::Projection, projection);
        // Nobody else's cue counts: not a paired device with no role, not
        // this device, not a stranger.
        for from in [id(8), node.device(), id(99)] {
            node.scene_cue_received(from, 1, 1, Some("Forged".into()));
        }
        assert!(lock(&taken).is_empty());
        assert!(cues_in(&mut frames).is_empty(), "no answer to a forger");
        // The Projection holder's does, and is answered.
        node.scene_cue_received(id(7), 1, 1, Some("Worship".into()));
        assert_eq!(*lock(&taken), vec![Some("Worship".to_string())]);
        assert_eq!(
            cues_in(&mut frames),
            vec![Frame::ObsCueAck { run: 1, seq: 1 }]
        );
        // A cue for no scene is still a cue: a new item.
        node.scene_cue_received(id(7), 1, 2, None);
        assert_eq!(lock(&taken).len(), 2);
        assert_eq!(lock(&taken)[1], None);
    }

    #[test]
    fn an_old_or_repeated_scene_cue_is_not_applied_again() {
        let (node, _, _, _dir) = node_with_hooks();
        let taken = taking_cues(&node);
        paired(&node, 7);
        let mut frames = connected_to(&node, 7);
        heard_roles_from(&node, 7);
        node.claim_role(Role::Stream).unwrap();
        let (_, projection) = claimed(7, Role::Projection);
        node.record_received(id(7), Role::Projection, projection);
        node.scene_cue_received(id(7), 5, 3, Some("Sermon".into()));
        // The same cue again (its ack was lost), and an older one that
        // arrived late.
        node.scene_cue_received(id(7), 5, 3, Some("Sermon".into()));
        node.scene_cue_received(id(7), 5, 2, Some("Worship".into()));
        assert_eq!(*lock(&taken), vec![Some("Sermon".to_string())]);
        let acks = cues_in(&mut frames);
        assert_eq!(acks[..2], vec![Frame::ObsCueAck { run: 5, seq: 3 }; 2][..]);
        // A sender that restarted starts a new run at 1.
        node.scene_cue_received(id(7), 6, 1, Some("Wide".into()));
        assert_eq!(lock(&taken).len(), 2);
        // A name that is not a scene name is not believed.
        node.scene_cue_received(id(7), 6, 2, Some("x".repeat(cue::MAX_SCENE + 1)));
        assert_eq!(lock(&taken).len(), 2);
    }

    #[test]
    fn a_scene_cue_is_ignored_where_stream_is_not_held() {
        let (node, _, _, _dir) = node_with_hooks();
        let taken = taking_cues(&node);
        paired(&node, 7);
        let (_, projection) = claimed(7, Role::Projection);
        node.record_received(id(7), Role::Projection, projection);
        // Nobody holds Stream: Projection keeps the OBS duty, so a cue sent
        // here is not for this device.
        node.scene_cue_received(id(7), 1, 1, Some("Worship".into()));
        // Another device holds it.
        let (_, held) = claimed(9, Role::Stream);
        lock(&node.trust).add(id(9), "Back room", None).unwrap();
        node.record_received(id(9), Role::Stream, held);
        node.scene_cue_received(id(7), 1, 2, Some("Worship".into()));
        assert!(lock(&taken).is_empty());
    }

    #[test]
    fn projection_keeps_the_obs_duty_while_nobody_holds_stream_and_only_then() {
        let (node, _, _, _dir) = node_with_hooks();
        // Alone: all four roles, so this device's own OBS.
        assert!(node.obs_here());
        assert_eq!(node.route_scene_cue(Some("A".into())), CueRoute::Local);
        paired(&node, 7);
        node.set_screens(true);
        node.claim_role(Role::Projection).unwrap();
        // Paired, Stream unheld: the old duty, as in 0.7.0.
        assert!(node.obs_here() && node.stream_elsewhere().is_none());
        assert_eq!(node.route_scene_cue(Some("A".into())), CueRoute::Local);
        // Another device takes Stream: not this one's OBS any more.
        let (_, held) = claimed(7, Role::Stream);
        node.record_received(id(7), Role::Stream, held);
        assert!(!node.obs_here());
        assert_eq!(node.stream_elsewhere().map(|s| s.0), Some(id(7)));
        assert_eq!(node.route_scene_cue(Some("A".into())), CueRoute::Sent);
        // A device that holds neither has no cue to give.
        lock(&node.roles).release(Role::Projection);
        assert_eq!(node.route_scene_cue(Some("A".into())), CueRoute::Dropped);
    }

    #[test]
    fn the_latest_cue_waits_for_the_stream_holder_and_goes_when_it_connects() {
        let (node, _, _, _dir) = node_with_hooks();
        paired(&node, 7);
        node.set_screens(true);
        node.claim_role(Role::Projection).unwrap();
        let (_, held) = claimed(7, Role::Stream);
        node.record_received(id(7), Role::Stream, held);
        // The Stream holder is offline: nothing is sent, the cue waits, and a
        // newer one replaces it.
        assert_eq!(node.route_scene_cue(Some("Worship".into())), CueRoute::Sent);
        assert_eq!(node.route_scene_cue(Some("Sermon".into())), CueRoute::Sent);
        assert!(node.scene_cue_waiting());
        let mut frames = connected_to(&node, 7);
        assert!(cues_in(&mut frames).is_empty());
        // It connects: only the latest goes.
        node.session_opened(id(7));
        let run = lock(&node.cue).run;
        assert_eq!(
            cues_in(&mut frames),
            vec![Frame::ObsCue {
                run,
                seq: 2,
                scene: Some("Sermon".into())
            }]
        );
        // Until it says it has it, it goes again at the next connection.
        assert!(node.scene_cue_waiting());
        node.session_opened(id(7));
        assert_eq!(cues_in(&mut frames).len(), 1);
        // Only the Stream holder's ack for this run counts.
        node.scene_cue_ack_received(id(8), run, 2);
        node.scene_cue_ack_received(id(7), run ^ 1, 2);
        node.scene_cue_ack_received(id(7), run, 1);
        assert!(node.scene_cue_waiting());
        node.scene_cue_ack_received(id(7), run, 2);
        assert!(!node.scene_cue_waiting());
        node.session_opened(id(7));
        assert!(cues_in(&mut frames).is_empty());
    }

    #[test]
    fn a_cue_kept_for_one_stream_holder_is_dropped_when_stream_moves() {
        let (node, _, _, _dir) = node_with_hooks();
        paired(&node, 7);
        paired(&node, 9);
        node.set_screens(true);
        node.claim_role(Role::Projection).unwrap();
        let (mut seven, held) = claimed(7, Role::Stream);
        node.record_received(id(7), Role::Stream, held);
        assert_eq!(node.route_scene_cue(Some("Worship".into())), CueRoute::Sent);
        // Stream goes to device 9 (device 7 hands it on, signed).
        let next = handed(&mut seven, Role::Stream, 9);
        node.record_received(id(7), Role::Stream, next);
        assert_eq!(node.stream_elsewhere().map(|s| s.0), Some(id(9)));
        assert!(
            !node.scene_cue_waiting(),
            "passing Stream leaves OBS where it is"
        );
        let mut frames = connected_to(&node, 9);
        node.session_opened(id(9));
        assert!(cues_in(&mut frames).is_empty());
    }

    #[test]
    fn a_cue_from_a_displaced_former_projection_holder_is_ignored() {
        let (node, _, _, _dir) = node_with_hooks();
        let taken = taking_cues(&node);
        paired(&node, 9);
        paired(&node, 7);
        node.claim_role(Role::Stream).unwrap();
        let (mut seven, projection) = claimed(7, Role::Projection);
        node.record_received(id(7), Role::Projection, projection);
        node.scene_cue_received(id(7), 1, 1, Some("Worship".into()));
        assert_eq!(lock(&taken).len(), 1);
        // Projection is handed on to device 9: 7 is a former holder.
        let next = handed(&mut seven, Role::Projection, 9);
        node.record_received(id(7), Role::Projection, next);
        node.scene_cue_received(id(7), 1, 2, Some("Late".into()));
        assert_eq!(lock(&taken).len(), 1, "the former holder's cue is ignored");
        node.scene_cue_received(id(9), 1, 1, Some("Sermon".into()));
        assert_eq!(lock(&taken).len(), 2);
    }

    #[test]
    fn a_cue_before_the_app_is_ready_is_not_taken_or_answered_and_counts_when_resent() {
        let (node, _, _, _dir) = node_with_hooks();
        paired(&node, 7);
        let mut frames = connected_to(&node, 7);
        heard_roles_from(&node, 7);
        node.claim_role(Role::Stream).unwrap();
        let (_, projection) = claimed(7, Role::Projection);
        node.record_received(id(7), Role::Projection, projection);
        // No hook yet.
        node.scene_cue_received(id(7), 1, 1, Some("Worship".into()));
        assert!(
            cues_in(&mut frames).is_empty(),
            "no ack, so it is sent again"
        );
        // The app is ready; the sender's resend of the same cue is applied.
        let taken = taking_cues(&node);
        node.scene_cue_received(id(7), 1, 1, Some("Worship".into()));
        assert_eq!(*lock(&taken), vec![Some("Worship".to_string())]);
        assert_eq!(
            cues_in(&mut frames),
            vec![Frame::ObsCueAck { run: 1, seq: 1 }]
        );
    }

    #[test]
    fn a_scene_name_of_1024_bytes_is_believed_and_one_of_1025_is_not() {
        let (node, _, _, _dir) = node_with_hooks();
        let taken = taking_cues(&node);
        paired(&node, 7);
        node.claim_role(Role::Stream).unwrap();
        let (_, projection) = claimed(7, Role::Projection);
        node.record_received(id(7), Role::Projection, projection);
        node.scene_cue_received(id(7), 1, 1, Some("x".repeat(1024)));
        assert_eq!(lock(&taken).len(), 1);
        node.scene_cue_received(id(7), 1, 2, Some("x".repeat(1025)));
        assert_eq!(lock(&taken).len(), 1);
    }

    #[test]
    fn an_oversized_scene_name_is_not_sent_or_kept_to_be_sent_again() {
        let (node, _, _, _dir) = node_with_hooks();
        node.set_screens(true);
        paired(&node, 7);
        node.claim_role(Role::Projection).unwrap();
        let (_, held) = claimed(7, Role::Stream);
        node.record_received(id(7), Role::Stream, held);
        let mut frames = connected_to(&node, 7);
        assert_eq!(
            node.route_scene_cue(Some("x".repeat(cue::MAX_SCENE + 1))),
            CueRoute::Sent
        );
        let run = lock(&node.cue).run;
        // The item still goes live, without a scene, and that is what is
        // kept and sent.
        assert_eq!(
            cues_in(&mut frames),
            vec![Frame::ObsCue {
                run,
                seq: 1,
                scene: None
            }]
        );
        node.scene_cue_ack_received(id(7), run, 1);
        assert!(!node.scene_cue_waiting());
        // The largest name is sent as it is.
        node.route_scene_cue(Some("x".repeat(cue::MAX_SCENE)));
        assert!(matches!(
            cues_in(&mut frames).as_slice(),
            [Frame::ObsCue { scene: Some(s), .. }] if s.len() == cue::MAX_SCENE
        ));
    }

    #[test]
    fn an_ack_from_the_old_stream_holder_after_stream_moved_clears_nothing() {
        let (node, _, _, _dir) = node_with_hooks();
        node.set_screens(true);
        paired(&node, 7);
        paired(&node, 9);
        node.claim_role(Role::Projection).unwrap();
        let (mut seven, held) = claimed(7, Role::Stream);
        node.record_received(id(7), Role::Stream, held);
        node.route_scene_cue(Some("Worship".into()));
        let run = lock(&node.cue).run;
        let next = handed(&mut seven, Role::Stream, 9);
        node.record_received(id(7), Role::Stream, next);
        // The next item is for the new holder, and the old one's late ack
        // for the first cue does not clear it.
        assert_eq!(node.route_scene_cue(Some("Sermon".into())), CueRoute::Sent);
        node.scene_cue_ack_received(id(7), run, 2);
        assert!(node.scene_cue_waiting());
        node.scene_cue_ack_received(id(9), run, 2);
        assert!(!node.scene_cue_waiting());
    }
}
