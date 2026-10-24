//! Roles: who is doing what right now. A church's devices share four jobs,
//! each held by one paired device at a time: **Projection** (runs the
//! screens), **Operator** (runs the service), **Stream** (runs what goes
//! out online) and **Audio** (runs the sound). Every device still holds and
//! edits a full copy of the library and merges peer to peer; only these
//! jobs are single-holder. docs/decisions/0010-roles-and-handoff.md has the
//! reasoning; this module is the pure rules, and `net` carries the records
//! between devices.
//!
//! It generalises the presenter lease (0008, now the Projection record) and
//! the driver seat (0009, now the Operator record) into one shape per role,
//! a [`Record`]: the holder's device key and name and an epoch. Records
//! merge as a maximum ordered by `(epoch, holder)`, so devices that claim
//! at the same moment agree on one winner without talking, and a record
//! never goes back to an older holder.
//!
//! The same rules for every role:
//!
//! - A device with no paired devices holds all four, whatever an old record
//!   says.
//! - Another device takes a role by *asking* ([`Roles::ask`]); the holder
//!   answers [`Roles::allow`] or [`Roles::decline`]. It answers one ask at a
//!   time: a second asker is told who is already asking and is not queued
//!   ([`Answer::Busy`]).
//! - It may *take over* without asking ([`Roles::claim`]) only when the
//!   holder has been offline for [`Roles::takeover_after`] (two minutes by
//!   default), or when nobody holds the role.
//! - A holder that comes back finds a newer record and drops the role, with
//!   a [`Notice`]. It never resumes by itself.
//! - "Always allow this device" is remembered for Operator and Stream, never
//!   for Projection or Audio ([`Role::offers_always`]), because handing
//!   either over clears something on air ([`Role::on_air`]).
//!
//! The dependencies between roles:
//!
//! - **Projection** can be held only by a device that has screens set up
//!   ([`Roles::set_screens`]); [`Denied::NoScreens`] otherwise. It depends
//!   on nothing.
//! - **Operator** needs Projection held by somebody ([`Denied::NeedsProjection`]
//!   to take it). While nobody can be reached as Projection the controls wait
//!   ([`Roles::operator_ready`]) and the seat is kept. The Projection holder
//!   may always take the Operator seat back.
//! - **Audio** needs Projection only for new cues, which is playback and not
//!   holding; it can be held anywhere.
//! - **Stream** depends on nothing.
//!
//! Teams (T18) will gate who may hold a role at the two places that grant
//! one: [`Roles::claim`] and [`Roles::allow`] (both go through
//! `Roles::check_teams`). Nothing else changes when it lands.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::team::DeviceKey;

/// How long a holder must be offline before another device may take over
/// without asking.
pub const DEFAULT_TAKEOVER: Duration = Duration::from_secs(120);

/// The shortest time that can be set: a brief network blip must never hand
/// a role to another device.
pub const MIN_TAKEOVER: Duration = Duration::from_secs(1);

/// The longest time that can be set.
pub const MAX_TAKEOVER: Duration = Duration::from_secs(24 * 3600);

/// One of the four jobs. The order is the order they are listed in.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum Role {
    /// "Runs the screens": the projector and stage screens, the live engine
    /// and the local server OBS reads. Today's presenter lease.
    #[default]
    Projection,
    /// "Runs the service": next, previous, jump, take an item live. Today's
    /// driver seat.
    Operator,
    /// "Runs the stream": the OBS scene, mutes and preview.
    Stream,
    /// "Runs the sound": the sound Lampstand plays.
    Audio,
}

/// What passing a role on takes off air.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnAir {
    /// The screens go clear.
    Screens,
    /// What is playing stops.
    Music,
    /// Nothing changes: the slide stays up, OBS stays on its scene.
    Nothing,
}

impl Role {
    pub const ALL: [Role; 4] = [Role::Projection, Role::Operator, Role::Stream, Role::Audio];

    pub fn index(self) -> usize {
        self as usize
    }

    /// The name used in `roles.json`.
    pub fn key(self) -> &'static str {
        match self {
            Role::Projection => "projection",
            Role::Operator => "operator",
            Role::Stream => "stream",
            Role::Audio => "audio",
        }
    }

    /// Whether the holder may remember a device as always allowed.
    pub fn offers_always(self) -> bool {
        matches!(self, Role::Operator | Role::Stream)
    }

    /// What handing this role to another device takes off air: only a
    /// Projection hand-over clears the screens, only an Audio hand-over
    /// stops the music.
    pub fn on_air(self) -> OnAir {
        match self {
            Role::Projection => OnAir::Screens,
            Role::Audio => OnAir::Music,
            Role::Operator | Role::Stream => OnAir::Nothing,
        }
    }
}

/// The right to do one job, held by one device.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// `None` until a device first claims.
    #[serde(with = "opt_hex_key")]
    pub holder: Option<DeviceKey>,
    /// The holder's name when it claimed: "Sanctuary".
    pub name: String,
    /// Starts at 0 (unclaimed); every claim or hand-over adds one.
    pub epoch: u64,
    /// How the role came to be held, newest first: the signed steps from an
    /// earlier state to this one, at most [`CHAIN`]. Empty for a record moved
    /// over from 0.7.0, which is trusted once as the state this device starts
    /// from, and for a role nobody has held.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "bounded_chain"
    )]
    pub chain: Vec<Link>,
}

/// Reads at most [`CHAIN`] steps, and fails on the first one past that, so
/// a record cannot make a device read or verify more than it asked for.
pub fn bounded_chain<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Link>, D::Error> {
    struct Steps;
    impl<'de> serde::de::Visitor<'de> for Steps {
        type Value = Vec<Link>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "at most {CHAIN} steps")
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            let mut steps = Vec::new();
            while let Some(step) = seq.next_element::<Link>()? {
                if steps.len() == CHAIN {
                    return Err(serde::de::Error::custom("too many steps"));
                }
                steps.push(step);
            }
            Ok(steps)
        }
    }
    d.deserialize_seq(Steps)
}

/// How many steps a record carries, so a device that missed several
/// hand-overs can still check its way to the present.
pub const CHAIN: usize = 8;

/// What a step did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// The new holder took the role itself; signed by it.
    Claim,
    /// The holder handed the role to another device; signed by the holder.
    HandOver,
    /// The holder let go; signed by the holder. Nobody holds the role after.
    Release,
}

/// One signed step in a role's history: from `(prev_holder, prev_epoch)` to
/// `(holder, epoch)`. The signature covers the role, the kind, both states
/// and a fixed tag, so a step cannot be replayed onto another role or epoch.
/// The name is not signed; names come from pairing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub kind: Kind,
    #[serde(with = "opt_hex_key")]
    pub holder: Option<DeviceKey>,
    pub epoch: u64,
    #[serde(with = "opt_hex_key")]
    pub prev_holder: Option<DeviceKey>,
    pub prev_epoch: u64,
    #[serde(with = "hex_bytes")]
    pub sig: Vec<u8>,
}

impl Link {
    fn bytes(
        role: Role,
        kind: Kind,
        holder: Option<DeviceKey>,
        epoch: u64,
        prev_holder: Option<DeviceKey>,
        prev_epoch: u64,
    ) -> Vec<u8> {
        let mut out = b"lampstand-role-v1".to_vec();
        out.push(role as u8);
        out.push(kind as u8);
        for key in [holder, prev_holder] {
            match key {
                Some(k) => {
                    out.push(1);
                    out.extend_from_slice(&k.0);
                }
                None => out.push(0),
            }
        }
        out.extend_from_slice(&epoch.to_le_bytes());
        out.extend_from_slice(&prev_epoch.to_le_bytes());
        out
    }

    /// Who must have signed it: the new holder for a claim, the one who let
    /// go for the others.
    pub fn signer(&self) -> Option<DeviceKey> {
        match self.kind {
            Kind::Claim => self.holder,
            Kind::HandOver | Kind::Release => self.prev_holder,
        }
    }

    /// Whether it is well formed and signed by the device it names.
    fn valid(&self, role: Role) -> bool {
        let shape = match self.kind {
            Kind::Claim | Kind::HandOver => self.holder.is_some(),
            Kind::Release => self.holder.is_none(),
        } && self.epoch == self.prev_epoch.wrapping_add(1)
            && self.epoch != 0
            && self.epoch <= EPOCH_LIMIT;
        let (Some(signer), Ok(sig)) = (self.signer(), <[u8; 64]>::try_from(self.sig.as_slice()))
        else {
            return false;
        };
        shape
            && signer.verify(
                &Self::bytes(
                    role,
                    self.kind,
                    self.holder,
                    self.epoch,
                    self.prev_holder,
                    self.prev_epoch,
                ),
                &sig,
            )
    }
}

/// How this device signs a step. Supplied by the node, which holds the
/// device key.
#[derive(Clone)]
pub struct Signer(std::sync::Arc<SignFn>);

type SignFn = dyn Fn(&[u8]) -> [u8; 64] + Send + Sync;

impl Signer {
    pub fn new(f: impl Fn(&[u8]) -> [u8; 64] + Send + Sync + 'static) -> Self {
        Self(std::sync::Arc::new(f))
    }
}

impl std::fmt::Debug for Signer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Signer")
    }
}

mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&bytes.iter().map(|b| format!("{b:02x}")).collect::<String>())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let hex = String::deserialize(d)?;
        if hex.len() % 2 != 0 || !hex.is_ascii() {
            return Err(serde::de::Error::custom("not hex"));
        }
        (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16))
            .collect::<Result<_, _>>()
            .map_err(serde::de::Error::custom)
    }
}

impl Record {
    /// Whether this record replaces `other` when they meet: the higher
    /// epoch, and for claims made at the same moment the higher device key.
    ///
    /// For two records at the same epoch the one the holder signed away
    /// (a hand-over or a release) comes before one somebody took (a claim),
    /// which comes before one with no steps; after that the higher key.
    /// Every device applies the same order, so they all end on one record.
    pub fn beats(&self, other: &Record) -> bool {
        (self.epoch, self.rank(), self.holder) > (other.epoch, other.rank(), other.holder)
    }

    fn rank(&self) -> u8 {
        match self.chain.first().map(|l| l.kind) {
            Some(Kind::HandOver | Kind::Release) => 2,
            Some(Kind::Claim) => 1,
            None => 0,
        }
    }
}

/// No epoch past this is believed, and none is ever made: far below the
/// largest number, so a forged one cannot wedge a role.
pub const EPOCH_LIMIT: u64 = 1 << 62;

mod opt_hex_key {
    use serde::{Deserialize, Deserializer, Serializer};

    use crate::team::DeviceKey;

    pub fn serialize<S: Serializer>(key: &Option<DeviceKey>, s: S) -> Result<S::Ok, S::Error> {
        match key {
            Some(k) => s.serialize_some(&k.to_hex()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DeviceKey>, D::Error> {
        let hex = Option::<String>::deserialize(d)?;
        match hex {
            None => Ok(None),
            Some(h) => DeviceKey::from_hex(&h)
                .map(Some)
                .ok_or_else(|| serde::de::Error::custom("not a device key")),
        }
    }
}

/// What this device may do on screen: the Projection role, as the screens
/// and the live commands see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Drives the screens and may take a service live.
    Presenter,
    /// Edits and syncs freely; its screens stay dark.
    Planning,
}

/// What the node knows of a role holder's connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    /// This device holds the role.
    Me,
    /// Nobody holds it, or the holder is no longer paired with this device.
    Gone,
    /// Connected now.
    Online,
    /// Not connected, and not heard from for this long.
    Offline(Duration),
}

/// Who is paired and reachable, supplied by the node for each question.
pub trait Net {
    /// At least one device is paired with this one.
    fn has_peers(&self) -> bool;
    /// What is known of a paired device's connection (`Gone` for a device
    /// that is not paired).
    fn holder(&self, device: DeviceKey) -> Holder;
    /// How long a paired device had been out of reach when its present
    /// connection began, or has been since if it is not connected. `None`
    /// for a device that is not paired.
    fn away(&self, device: DeviceKey) -> Option<Duration>;
}

/// How this device could take a role right now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Standing {
    /// It holds the role (or has nobody to share with).
    #[default]
    Holding,
    /// Nobody holds the role: take it with one click, no permission.
    Open,
    /// The holder is connected: ask it.
    Ask,
    /// The holder has been offline long enough to take over.
    TakeOver { offline: Duration },
    /// The holder is offline but may be back: not yet.
    Wait {
        offline: Duration,
        remaining: Duration,
    },
}

/// What stops a device taking a role whatever the holder says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
    /// Projection needs screens set up on this device.
    NoScreens,
    /// Operator needs Projection held by somebody.
    NeedsProjection,
}

/// Why a claim or ask was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denied {
    /// The holder is connected: ask it instead.
    MustAsk,
    /// The holder is offline but only for `offline`; `remaining` to go.
    TooSoon {
        offline: Duration,
        remaining: Duration,
    },
    /// This device already holds the role.
    Already,
    /// The holder is offline, so there is nobody to ask.
    NobodyToAsk,
    /// Nothing to ask: take the role directly.
    NotNeeded,
    /// No screens are set up on this device, so it cannot hold Projection.
    NoScreens,
    /// Nobody holds Projection, so Operator cannot be taken.
    NeedsProjection,
    /// Teams (T18) does not let this device hold the role.
    NotPermitted,
}

/// Teams (T18) does not let a device hold a role. Never returned until T18.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotPermitted;

impl From<Blocked> for Denied {
    fn from(b: Blocked) -> Self {
        match b {
            Blocked::NoScreens => Denied::NoScreens,
            Blocked::NeedsProjection => Denied::NeedsProjection,
        }
    }
}

impl std::fmt::Display for Denied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Denied::MustAsk => write!(
                f,
                "the presenting device is connected: ask it to allow this"
            ),
            Denied::TooSoon { remaining, .. } => write!(
                f,
                "the presenting device went offline only recently; you can take over in {}s",
                remaining.as_secs().max(1)
            ),
            Denied::Already => write!(f, "this device is already presenting"),
            Denied::NobodyToAsk => write!(
                f,
                "the presenting device is not connected, so it cannot be asked"
            ),
            Denied::NotNeeded => write!(f, "nobody needs to allow this: present from here"),
            Denied::NoScreens => write!(f, "No screens are set up on this device"),
            // The wording the driver's bar already used when nothing presents.
            Denied::NeedsProjection => write!(f, "nobody is presenting"),
            Denied::NotPermitted => write!(f, "this device may not hold that role"),
        }
    }
}

/// A device, as the prompts name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    pub device: DeviceKey,
    pub name: String,
}

/// The holder's reply to an ask that was not simply allowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The person said not now.
    Declined,
    /// Another device is already asking; `by` is its name.
    Busy { by: String },
}

/// What the holder does with an ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// Show the prompt: allow or not now.
    Prompt,
    /// The device is remembered: allow it now ([`Roles::allow`]).
    Remembered,
    /// Answer with this and show nothing.
    Refuse(Answer),
    /// This device does not hold the role, so the ask means nothing here.
    Ignored,
}

/// Why the holder could not allow a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotAllowed {
    /// This device does not hold the role.
    NotHolding,
    /// The device is not asking (it withdrew, or went offline).
    NotAsking,
    /// Operator cannot move while nobody holds Projection.
    NeedsProjection,
    /// Teams (T18) does not let that device hold the role.
    NotPermitted,
}

impl std::fmt::Display for NotAllowed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotAllowed::NotHolding => write!(f, "this device is not presenting"),
            NotAllowed::NotAsking => write!(f, "that device is no longer asking"),
            NotAllowed::NeedsProjection => write!(f, "nobody is presenting"),
            NotAllowed::NotPermitted => write!(f, "that device may not hold that role"),
        }
    }
}

/// A role handed over by [`Roles::allow`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allowed {
    /// The new record, to send to every device.
    pub record: Record,
    /// What this hand-over takes off air.
    pub on_air: OnAir,
}

/// Where this device's own ask stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskState {
    Waiting,
    Declined,
    Busy { by: String },
}

/// This device's own ask, as the asker sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asking {
    /// The record epoch it was asked at.
    pub epoch: u64,
    pub state: AskState,
}

/// Something the person should be told once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// This device held the role and another device now does.
    Replaced { by: String },
}

/// What a [`Roles::merge`] changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Merged {
    pub changed: bool,
    pub lost_role: bool,
}

/// The four records and the asking around them, for one device.
#[derive(Debug, Clone)]
pub struct Roles {
    me: DeviceKey,
    name: String,
    records: [Record; 4],
    takeover_after: Duration,
    screens: bool,
    /// An ask waiting for this device's answer, per role it holds.
    pending: [Option<Named>; 4],
    /// This device's own ask, per role.
    asking: [Option<Asking>; 4],
    notice: [Option<Notice>; 4],
    /// Devices answered at once, per role that offers it.
    always: [BTreeMap<DeviceKey, String>; 4],
    /// This device is giving the role up on purpose: no notice when it goes.
    yielding: [bool; 4],
    /// A newer record arrived whose steps do not join this device's state
    /// (it missed more than [`CHAIN`] of them); it waits for one that does.
    unverified: [bool; 4],
    signer: Signer,
    /// The device this one has just paired with: the only one whose present
    /// state a device with nothing of its own takes without joining it.
    seed: Option<DeviceKey>,
    /// Tests that do not care who seeds.
    #[cfg(test)]
    seed_any: bool,
}

impl Roles {
    pub fn new(
        me: DeviceKey,
        name: String,
        records: [Record; 4],
        takeover_after: Duration,
        always: [BTreeMap<DeviceKey, String>; 4],
        signer: Signer,
    ) -> Self {
        Self {
            signer,
            seed: None,
            #[cfg(test)]
            seed_any: false,
            unverified: [false; 4],
            me,
            name,
            records,
            takeover_after: takeover_after.clamp(MIN_TAKEOVER, MAX_TAKEOVER),
            screens: false,
            pending: Default::default(),
            asking: Default::default(),
            notice: Default::default(),
            always,
            yielding: [false; 4],
        }
    }

    pub fn record(&self, role: Role) -> &Record {
        &self.records[role.index()]
    }

    pub fn records(&self) -> &[Record; 4] {
        &self.records
    }

    pub fn takeover_after(&self) -> Duration {
        self.takeover_after
    }

    pub fn set_takeover_after(&mut self, after: Duration) {
        self.takeover_after = after.clamp(MIN_TAKEOVER, MAX_TAKEOVER);
    }

    /// Whether this device has screens set up (a projector, a stage display
    /// or its own display as an output).
    pub fn screens(&self) -> bool {
        self.screens
    }

    /// Says whether this device has screens set up. True if that changed.
    pub fn set_screens(&mut self, set_up: bool) -> bool {
        std::mem::replace(&mut self.screens, set_up) != set_up
    }

    /// The ask waiting for this device's answer, for a role it holds. At
    /// most one: a second asker is told who is asking and is not queued.
    pub fn request(&self, role: Role) -> Option<&Named> {
        self.pending[role.index()].as_ref()
    }

    pub fn asking(&self, role: Role) -> Option<&Asking> {
        self.asking[role.index()].as_ref()
    }

    pub fn notice(&self, role: Role) -> Option<&Notice> {
        self.notice[role.index()].as_ref()
    }

    pub fn dismiss_notice(&mut self, role: Role) {
        self.notice[role.index()] = None;
    }

    /// Devices answered at once when they ask, with the name they had when
    /// remembered.
    pub fn always(&self, role: Role) -> Vec<Named> {
        self.always[role.index()]
            .iter()
            .map(|(device, name)| Named {
                device: *device,
                name: name.clone(),
            })
            .collect()
    }

    /// Remembers a device as always allowed for `role`, or stops. True if
    /// that changed. Nothing is remembered for a role that does not offer it.
    pub fn set_always(&mut self, role: Role, device: DeviceKey, name: String, on: bool) -> bool {
        if !role.offers_always() {
            return false;
        }
        let always = &mut self.always[role.index()];
        if on {
            always.insert(device, name.clone()) != Some(name)
        } else {
            always.remove(&device).is_some()
        }
    }

    /// Forgets a device everywhere: it was unpaired. True if anything went.
    pub fn forget(&mut self, device: DeviceKey) -> bool {
        let mut changed = false;
        for role in Role::ALL {
            changed |= self.always[role.index()].remove(&device).is_some();
            changed |= self.receive_cancel(role, device);
        }
        changed
    }

    // ---- Who holds what ------------------------------------------------

    /// Whether the record names this device.
    pub fn holds(&self, role: Role) -> bool {
        self.record(role).holder == Some(self.me)
    }

    /// Whether this device does the job: it holds the record, or it has
    /// nobody to share with.
    pub fn holds_now(&self, role: Role, net: &dyn Net) -> bool {
        !net.has_peers() || self.holds(role)
    }

    /// What the node should report for a role's holder here.
    pub fn holder_of(&self, role: Role, net: &dyn Net) -> Holder {
        match self.record(role).holder {
            None => Holder::Gone,
            Some(h) if h == self.me => Holder::Me,
            Some(h) => net.holder(h),
        }
    }

    /// Whether Projection is held by somebody paired with this device.
    pub fn projection_held(&self, net: &dyn Net) -> bool {
        !net.has_peers() || self.holder_of(Role::Projection, net) != Holder::Gone
    }

    /// Whether the Operator controls work: Projection is here or connected.
    /// While it is not, they wait and the seat is kept.
    pub fn operator_ready(&self, net: &dyn Net) -> bool {
        !net.has_peers()
            || matches!(
                self.holder_of(Role::Projection, net),
                Holder::Me | Holder::Online
            )
    }

    /// What this device may do on screen: the Projection role.
    pub fn mode(&self, net: &dyn Net) -> Mode {
        if self.holds_now(Role::Projection, net) {
            Mode::Presenter
        } else {
            Mode::Planning
        }
    }

    /// What keeps this device from taking `role` whatever the holder says.
    pub fn blocked(&self, role: Role, net: &dyn Net) -> Option<Blocked> {
        if !net.has_peers() || self.holds(role) {
            return None;
        }
        match role {
            Role::Projection if !self.screens => Some(Blocked::NoScreens),
            Role::Operator if !self.projection_held(net) => Some(Blocked::NeedsProjection),
            _ => None,
        }
    }

    pub fn standing(&self, role: Role, net: &dyn Net) -> Standing {
        if !net.has_peers() {
            return Standing::Holding;
        }
        // The Projection holder runs the screens and may take the Operator
        // seat back whenever it likes (0009): nobody needs to allow it.
        if role == Role::Operator && self.holds(Role::Projection) && !self.holds(role) {
            return Standing::Open;
        }
        match self.holder_of(role, net) {
            Holder::Me => Standing::Holding,
            Holder::Gone => Standing::Open,
            Holder::Online => Standing::Ask,
            Holder::Offline(offline) if offline >= self.takeover_after => {
                Standing::TakeOver { offline }
            }
            Holder::Offline(offline) => Standing::Wait {
                offline,
                remaining: self.takeover_after - offline,
            },
        }
    }

    // ---- Taking a role -------------------------------------------------

    /// The one place that will ask Teams (T18) whether `device` may hold
    /// `role`. Both ways of getting a role pass through here: a device
    /// taking it for itself ([`Roles::claim`]) and a holder handing it to a
    /// device that asked ([`Roles::allow`]).
    fn check_teams(&self, _role: Role, _device: DeviceKey) -> Result<(), NotPermitted> {
        Ok(())
    }

    /// Takes a role without anyone's permission, when the rules allow it:
    /// nobody holds it, or the holder has been offline long enough (or this
    /// is the Projection holder taking the Operator seat back). Returns the
    /// new record, to send to every device.
    pub fn claim(&mut self, role: Role, net: &dyn Net) -> Result<Record, Denied> {
        if self.holds(role) {
            return Err(Denied::Already);
        }
        if let Some(blocked) = self.blocked(role, net) {
            return Err(blocked.into());
        }
        match self.standing(role, net) {
            // Alone: record the claim anyway, so a device that pairs later
            // starts out as the holder.
            Standing::Holding | Standing::Open | Standing::TakeOver { .. } => {}
            Standing::Ask => return Err(Denied::MustAsk),
            Standing::Wait { offline, remaining } => {
                return Err(Denied::TooSoon { offline, remaining })
            }
        }
        self.check_teams(role, self.me)
            .map_err(|_| Denied::NotPermitted)?;
        Ok(self.take(role))
    }

    /// Moves a role to `holder` (a signed step by this device) and returns
    /// the new record.
    fn step(&mut self, role: Role, kind: Kind, holder: Option<DeviceKey>, name: String) -> Record {
        let i = role.index();
        let old = &self.records[i];
        let epoch = old.epoch.saturating_add(1).min(EPOCH_LIMIT);
        let (prev_holder, prev_epoch) = (old.holder, old.epoch);
        let sig = (self.signer.0)(&Link::bytes(
            role,
            kind,
            holder,
            epoch,
            prev_holder,
            prev_epoch,
        ))
        .to_vec();
        let mut chain = vec![Link {
            kind,
            holder,
            epoch,
            prev_holder,
            prev_epoch,
            sig,
        }];
        chain.extend(old.chain.iter().take(CHAIN - 1).cloned());
        self.records[i] = Record {
            holder,
            name,
            epoch,
            chain,
        };
        self.unverified[i] = false;
        self.records[i].clone()
    }

    fn take(&mut self, role: Role) -> Record {
        let i = role.index();
        self.step(role, Kind::Claim, Some(self.me), self.name.clone());
        self.asking[i] = None;
        self.pending[i] = None;
        self.yielding[i] = false;
        self.records[i].clone()
    }

    /// A device that has just paired and finds roles nobody holds takes
    /// them: the one that showed the pairing code is the main machine, and
    /// gets Projection, Operator and Audio (Stream stays unheld, so
    /// Projection keeps the OBS duty). Each is taken only if the rules allow
    /// it, so a device with no screens gets none of the three. Returns the
    /// records it took, to send to every device.
    pub fn claim_if_unclaimed(&mut self, net: &dyn Net) -> Vec<(Role, Record)> {
        let mut taken = Vec::new();
        for role in [Role::Projection, Role::Operator, Role::Audio] {
            let open = self.record(role).holder.is_none();
            // The sound stays where the screens are, as it does today.
            let with_screens = role != Role::Audio || self.holds(Role::Projection);
            if open
                && with_screens
                && self.blocked(role, net).is_none()
                && self.check_teams(role, self.me).is_ok()
            {
                taken.push((role, self.take(role)));
            }
        }
        taken
    }

    /// Asks the holder to hand over. Only while it is connected.
    pub fn ask(&mut self, role: Role, net: &dyn Net) -> Result<u64, Denied> {
        if self.holds(role) {
            return Err(Denied::Already);
        }
        if let Some(blocked) = self.blocked(role, net) {
            return Err(blocked.into());
        }
        match self.standing(role, net) {
            Standing::Holding => Err(Denied::Already),
            Standing::Open | Standing::TakeOver { .. } => Err(Denied::NotNeeded),
            Standing::Wait { .. } => Err(Denied::NobodyToAsk),
            Standing::Ask => {
                let epoch = self.record(role).epoch;
                self.asking[role.index()] = Some(Asking {
                    epoch,
                    state: AskState::Waiting,
                });
                Ok(epoch)
            }
        }
    }

    /// Withdraws or dismisses this device's ask. True if one was open and
    /// not yet answered (the holder should be told).
    pub fn cancel_ask(&mut self, role: Role) -> bool {
        self.asking[role.index()]
            .take()
            .is_some_and(|a| a.state == AskState::Waiting)
    }

    /// Clears a "not now" or "busy" answer, leaving an ask still waiting.
    pub fn clear_answer(&mut self, role: Role) {
        if self.asking[role.index()]
            .as_ref()
            .is_some_and(|a| a.state != AskState::Waiting)
        {
            self.asking[role.index()] = None;
        }
    }

    // ---- Answering an ask ----------------------------------------------

    /// A device asks this one to hand a role over. Ignored unless this
    /// device holds the role.
    pub fn receive_ask(&mut self, role: Role, device: DeviceKey, name: String) -> Asked {
        let i = role.index();
        if !self.holds(role) || device == self.me {
            return Asked::Ignored;
        }
        if let Some(open) = &self.pending[i] {
            if open.device != device {
                return Asked::Refuse(Answer::Busy {
                    by: open.name.clone(),
                });
            }
        }
        if let Some(remembered) = self.always[i].get_mut(&device) {
            remembered.clone_from(&name);
            self.pending[i] = Some(Named { device, name });
            return Asked::Remembered;
        }
        self.pending[i] = Some(Named { device, name });
        Asked::Prompt
    }

    /// The asking device withdrew, or went away. True if it had been asking.
    pub fn receive_cancel(&mut self, role: Role, device: DeviceKey) -> bool {
        let i = role.index();
        if self.pending[i].as_ref().is_some_and(|r| r.device == device) {
            self.pending[i] = None;
            return true;
        }
        false
    }

    /// Hands a role to the device that asked; with `always`, remembers the
    /// device if the role offers that. This device stops holding it.
    pub fn allow(
        &mut self,
        role: Role,
        device: DeviceKey,
        always: bool,
        net: &dyn Net,
    ) -> Result<Allowed, NotAllowed> {
        if !self.holds(role) {
            return Err(NotAllowed::NotHolding);
        }
        let i = role.index();
        let Some(asker) = self.pending[i].clone().filter(|r| r.device == device) else {
            return Err(NotAllowed::NotAsking);
        };
        if role == Role::Operator && !self.projection_held(net) {
            return Err(NotAllowed::NeedsProjection);
        }
        self.check_teams(role, device)
            .map_err(|_| NotAllowed::NotPermitted)?;
        if always && role.offers_always() {
            self.always[i].insert(asker.device, asker.name.clone());
        }
        self.step(role, Kind::HandOver, Some(asker.device), asker.name);
        self.pending[i] = None;
        Ok(Allowed {
            record: self.records[i].clone(),
            on_air: role.on_air(),
        })
    }

    /// Says not now to a device that asked. True if it was asking.
    pub fn decline(&mut self, role: Role, device: DeviceKey) -> bool {
        self.receive_cancel(role, device)
    }

    /// The holder turned this device's ask down.
    pub fn receive_answer(&mut self, role: Role, from: DeviceKey, epoch: u64, answer: Answer) {
        let record = self.record(role);
        if record.holder != Some(from) || record.epoch != epoch {
            return;
        }
        if let Some(asking) = self.asking[role.index()].as_mut() {
            asking.state = match answer {
                Answer::Declined => AskState::Declined,
                Answer::Busy { by } => AskState::Busy { by },
            };
        }
    }

    // ---- Giving a role up -----------------------------------------------

    /// This device gives a role it holds back to the device that runs it by
    /// default (the Operator seat to the Projection holder, which then
    /// takes it with [`Roles::claim`]). No notice is shown when it goes.
    /// True if this device held the role.
    pub fn yield_role(&mut self, role: Role) -> bool {
        let held = self.holds(role);
        if held {
            self.yielding[role.index()] = true;
        }
        held
    }

    /// This device lets go of a role it holds with nobody to hand it to
    /// (an Operator stopping while the Projection holder is away): the role
    /// becomes unheld. Not a grant. Returns the new record.
    pub fn release(&mut self, role: Role) -> Option<Record> {
        if !self.holds(role) {
            return None;
        }
        let i = role.index();
        self.step(role, Kind::Release, None, String::new());
        self.yielding[i] = false;
        self.pending[i] = None;
        Some(self.records[i].clone())
    }

    // ---- Meeting another device's record -----------------------------------

    /// Whether this device may take `remote`, a record that beats its own,
    /// for `role`. Its steps must be few enough, signed by devices this
    /// device knows, and join this device's own state. Hand-overs and
    /// releases then prove themselves (the one who held it signed it away).
    /// Claims in the past are checked for their signature and place in the
    /// chain only; only a claim at the head, sent by the claimant itself, is
    /// judged by this device's view of things ([`Roles::claim_ok`]).
    fn admit(&self, role: Role, remote: &Record, from: DeviceKey, net: &dyn Net) -> Admit {
        let local = self.record(role);
        let pristine = local.holder.is_none() && local.epoch == 0 && local.chain.is_empty();
        let steps = &remote.chain;
        // A role handed to this device that it could not hold is not taken.
        if remote.holder == Some(self.me) && self.blocked(role, net).is_some() {
            return Admit::No;
        }
        let Some(head) = steps.first() else {
            // No steps: a state moved over from 0.7.0. A device with none of
            // its own takes it from the device it just paired with. One
            // that has only moved-over state of its own takes a newer one
            // (the other device of a 0.7.0 pair was ahead of it).
            let migrated = local.chain.is_empty() && !pristine;
            let near = remote.epoch <= local.epoch.saturating_add(CHAIN as u64);
            return if (pristine && self.seeded_by(from)) || (migrated && near) {
                Admit::Yes
            } else {
                Admit::No
            };
        };
        // Before any signature is looked at.
        if steps.len() > CHAIN || head.holder != remote.holder || head.epoch != remote.epoch {
            return Admit::No;
        }
        if !steps.iter().all(|l| l.valid(role)) {
            return Admit::No;
        }
        // Each step starts where the one before it ended.
        if !steps
            .windows(2)
            .all(|w| w[0].prev_holder == w[1].holder && w[0].prev_epoch == w[1].epoch)
        {
            return Admit::No;
        }
        // Devices whose word counts: this one, the ones it is paired with,
        // and those already in the state it holds.
        let in_state = |d: DeviceKey| {
            local.holder == Some(d)
                || local
                    .chain
                    .iter()
                    .any(|l| l.holder == Some(d) || l.prev_holder == Some(d))
        };
        let known = |d: DeviceKey| d == self.me || net.away(d).is_some() || in_state(d);
        // Where this device's own state sits in the steps.
        let here = |holder: Option<DeviceKey>, epoch: u64| {
            steps
                .iter()
                .position(|l| l.holder == holder && l.epoch == epoch)
        };
        let oldest = steps.last().expect("not empty");
        let migrated = local.chain.is_empty() && !pristine;
        let newer: Option<&[Link]> = if let Some(k) = here(local.holder, local.epoch) {
            Some(&steps[..k])
        } else if oldest.prev_holder == local.holder && oldest.prev_epoch == local.epoch {
            Some(&steps[..])
        } else if migrated
            && oldest.prev_epoch >= local.epoch
            && oldest.prev_epoch - local.epoch <= CHAIN as u64
        {
            // The first signed step on a state moved over from 0.7.0, made
            // on a device that was ahead of this one.
            Some(&steps[..])
        } else {
            None
        };
        if newer.is_none() && pristine {
            // Nothing of its own to join: the present state, from the device
            // just paired.
            return if self.seeded_by(from) {
                Admit::Yes
            } else {
                Admit::No
            };
        }
        if let Some(newer) = newer {
            // Oldest first. Whoever signs must be known, or have been handed
            // the role by a step already checked.
            let mut introduced: Vec<DeviceKey> = Vec::new();
            for (i, l) in newer.iter().enumerate().rev() {
                let signer = l.signer().expect("valid steps have a signer");
                if !known(signer) && !introduced.contains(&signer) {
                    return Admit::No;
                }
                if let Some(h) = l.holder {
                    introduced.push(h);
                }
                if l.kind == Kind::Claim && i == 0 && from == signer {
                    // The claimant itself says so: as this device sees it.
                    if !self.claim_ok(role, signer, l.prev_holder, net) {
                        return Admit::No;
                    }
                }
            }
            return Admit::Yes;
        }
        // The same step from the same state as this device's own, made at the
        // same moment by someone else: the order of records decides, and it
        // has already said this one comes first.
        if let Some(mine) = local.chain.first() {
            if mine.prev_holder == head.prev_holder
                && mine.prev_epoch == head.prev_epoch
                && mine.epoch == head.epoch
            {
                let ok = match (head.kind, head.holder) {
                    (Kind::Claim, Some(claimant)) => {
                        (from == claimant || known(claimant))
                            && self.claim_ok(role, claimant, head.prev_holder, net)
                    }
                    // Signed by the one who held it; it is in this state.
                    _ => head.signer().is_some_and(known),
                };
                if ok {
                    return Admit::Yes;
                }
            }
        }
        // A claim from the device that made it, whatever it missed (within
        // what could have been missed): judged as a fresh claim here.
        if let (Kind::Claim, Some(claimant)) = (head.kind, head.holder) {
            if from == claimant
                && head.epoch <= local.epoch.saturating_add(CHAIN as u64)
                && self.claim_ok(role, claimant, local.holder, net)
            {
                return Admit::Yes;
            }
        }
        // It does not join. If every device that signed is one this device
        // knows, it has missed more than it can follow and waits; a chain
        // signed by devices it has never heard of is not a gap, it is not
        // trusted at all.
        if steps.iter().all(|l| l.signer().is_some_and(known)) {
            Admit::Unverified
        } else {
            Admit::No
        }
    }

    /// Whether `claimant` taking the role from `prev` is one the claim rules
    /// allow, as this device sees things: nobody holds it or the holder is
    /// not paired here; the holder has been out of reach for the takeover
    /// time; or the Projection holder takes the Operator seat back. A claim
    /// against this device needs the claimant to have been out of reach of it
    /// for the takeover time. Nobody can prove that to anyone else, so every
    /// device judges for itself.
    fn claim_ok(
        &self,
        role: Role,
        claimant: DeviceKey,
        prev: Option<DeviceKey>,
        net: &dyn Net,
    ) -> bool {
        if role == Role::Operator && self.record(Role::Projection).holder == Some(claimant) {
            return true;
        }
        let Some(prev) = prev else { return true };
        if prev == self.me {
            // A claim against this device: it holds the role, so it cannot
            // say the holder was away. What it can say is whether the
            // claimant was out of reach of it for the takeover time before
            // it reconnected, which is the same outage seen from this end
            // (the rule a device is held to when it claims). A claimant that
            // has been connected all along is refused: it can ask instead.
            return net
                .away(claimant)
                .is_some_and(|away| away >= self.takeover_after);
        }
        match net.holder(prev) {
            Holder::Gone => true,
            Holder::Online | Holder::Me => false,
            Holder::Offline(offline) => offline >= self.takeover_after,
        }
    }

    /// This device starts over on every role whose present state it could
    /// not verify, so the next device it pairs with seeds it afresh. Done
    /// when it pairs with a device (unpair and pair again to recover).
    pub fn forget_unverified(&mut self) -> bool {
        let mut any = false;
        for i in 0..4 {
            if std::mem::take(&mut self.unverified[i]) {
                self.records[i] = Record::default();
                self.pending[i] = None;
                any = true;
            }
        }
        any
    }

    /// A session ended: a hand-back that was asked for on it will not
    /// arrive, so the role is no longer being given up quietly.
    pub fn forget_yields(&mut self) {
        self.yielding = [false; 4];
    }

    /// This device's record for `role` if it comes before `other` in the
    /// order every device uses: what to send back to a device that is behind
    /// or lost a tie, so it learns where things stand.
    pub fn better_than(&self, role: Role, other: &Record) -> Option<Record> {
        self.record(role)
            .beats(other)
            .then(|| self.record(role).clone())
    }

    /// Says which device this one has just paired with. A device with
    /// nothing of its own takes the present state from that device (trust on
    /// first use, as at pairing) and from no other.
    pub fn seed_from(&mut self, device: DeviceKey) {
        self.seed = Some(device);
    }

    fn seeded_by(&self, from: DeviceKey) -> bool {
        #[cfg(test)]
        if self.seed_any {
            return true;
        }
        self.seed == Some(from)
    }

    /// Whether a newer record for `role` was seen whose steps do not join
    /// this device's state: it waits for one that does.
    pub fn unverified(&self, role: Role) -> bool {
        self.unverified[role.index()]
    }

    /// Takes in a record from another device (`from`): the newer one wins,
    /// if its signed steps check out ([`Roles::admit`]).
    pub fn merge(&mut self, role: Role, remote: &Record, from: DeviceKey, net: &dyn Net) -> Merged {
        let i = role.index();
        // An epoch that no claim could ever beat would lock the role for
        // good: it is nonsense, not a record.
        if remote.epoch > EPOCH_LIMIT || !remote.beats(&self.records[i]) {
            return Merged::default();
        }
        match self.admit(role, remote, from, net) {
            Admit::Yes => {}
            Admit::No => return Merged::default(),
            Admit::Unverified => {
                self.unverified[i] = true;
                return Merged::default();
            }
        }
        // A stranger's record is not worth a notice.
        let known = remote
            .holder
            .is_some_and(|h| h == self.me || net.holder(h) != Holder::Gone);
        let was_holder = self.holds(role);
        self.records[i] = remote.clone();
        self.records[i].chain.truncate(CHAIN);
        self.unverified[i] = false;
        self.pending[i] = None;
        let lost_role = was_holder && !self.holds(role);
        if lost_role {
            let chosen = std::mem::take(&mut self.yielding[i]);
            if known && !chosen {
                self.notice[i] = Some(Notice::Replaced {
                    by: remote.name.clone(),
                });
            }
        }
        // An ask is settled once the record has moved on, whoever got it.
        if self.asking[i]
            .as_ref()
            .is_some_and(|a| a.epoch < remote.epoch)
        {
            self.asking[i] = None;
        }
        Merged {
            changed: true,
            lost_role,
        }
    }
}

enum Admit {
    Yes,
    No,
    Unverified,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::team::DeviceKeypair;

    fn kp(n: u8) -> DeviceKeypair {
        DeviceKeypair::from_secret_bytes([n; 32])
    }

    fn key(n: u8) -> DeviceKey {
        kp(n).public()
    }

    /// Takes in a record as if its holder had sent it, over a network where
    /// everyone is online and has been away a long time.
    fn m(r: &mut Roles, role: Role, rec: &Record) -> Merged {
        r.merge(role, rec, rec.holder.unwrap_or(key(0)), &net(true, &online))
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    struct Peers<'a> {
        has_peers: bool,
        holder: &'a dyn Fn(DeviceKey) -> Holder,
        away: Option<Duration>,
        /// Devices this one has never paired with.
        strangers: Vec<DeviceKey>,
    }

    impl Net for Peers<'_> {
        fn has_peers(&self) -> bool {
            self.has_peers
        }
        fn holder(&self, device: DeviceKey) -> Holder {
            (self.holder)(device)
        }
        fn away(&self, d: DeviceKey) -> Option<Duration> {
            if self.strangers.contains(&d) {
                None
            } else {
                self.away
            }
        }
    }

    fn net(has_peers: bool, f: &dyn Fn(DeviceKey) -> Holder) -> Peers<'_> {
        Peers {
            has_peers,
            holder: f,
            away: Some(secs(999)),
            strangers: Vec::new(),
        }
    }

    /// Like [`net`], with a say in how long a claimant had been out of reach.
    fn net_away(f: &dyn Fn(DeviceKey) -> Holder, away: Option<Duration>) -> Peers<'_> {
        Peers {
            has_peers: true,
            holder: f,
            away,
            strangers: Vec::new(),
        }
    }

    fn online(_: DeviceKey) -> Holder {
        Holder::Online
    }

    fn device(n: u8, name: &str) -> Roles {
        let mut r = Roles::new(
            key(n),
            name.into(),
            Default::default(),
            DEFAULT_TAKEOVER,
            Default::default(),
            Signer::new(move |msg| kp(n).sign(msg)),
        );
        r.set_screens(true);
        r.seed_any = true;
        r
    }

    /// Two devices that agree `a` holds Projection, Operator and Audio, as
    /// pairing leaves them. `b` has screens too.
    fn pair() -> (Roles, Roles) {
        let mut a = device(1, "Sanctuary");
        let mut b = device(2, "Office PC");
        let taken = a.claim_if_unclaimed(&net(true, &online));
        assert_eq!(
            taken.iter().map(|(r, _)| *r).collect::<Vec<_>>(),
            [Role::Projection, Role::Operator, Role::Audio]
        );
        for (role, record) in taken {
            assert!(m(&mut b, role, &record).changed);
        }
        (a, b)
    }

    #[test]
    fn a_lone_device_holds_all_four_whatever_an_old_record_says() {
        let mut r = device(1, "Sanctuary");
        let alone = |_| Holder::Gone;
        let n = net(false, &alone);
        for role in Role::ALL {
            m(
                &mut r,
                role,
                &Record {
                    holder: Some(key(9)),
                    name: "Gone".into(),
                    epoch: 4,
                    chain: vec![],
                },
            );
            assert!(r.holds_now(role, &n), "{role:?}");
            assert_eq!(r.standing(role, &n), Standing::Holding);
            assert_eq!(r.blocked(role, &n), None);
        }
        assert_eq!(r.mode(&n), Mode::Presenter);
        // Even with no screens at all.
        r.set_screens(false);
        assert_eq!(r.mode(&n), Mode::Presenter);
    }

    #[test]
    fn pairing_gives_the_first_device_projection_operator_and_audio() {
        let (a, b) = pair();
        let n = net(true, &online);
        for role in [Role::Projection, Role::Operator, Role::Audio] {
            assert!(a.holds(role), "{role:?}");
            assert_eq!(b.record(role).name, "Sanctuary");
            assert_eq!(b.record(role).epoch, 1);
        }
        assert!(!a.holds(Role::Stream), "Stream stays unheld");
        assert_eq!(a.mode(&n), Mode::Presenter);
        assert_eq!(b.mode(&n), Mode::Planning);
        assert_eq!(b.standing(Role::Stream, &n), Standing::Open);
    }

    #[test]
    fn a_first_device_with_no_screens_gets_none_of_the_three() {
        let mut a = device(1, "Laptop");
        a.set_screens(false);
        let n = net(true, &online);
        assert!(a.claim_if_unclaimed(&n).is_empty());
        assert!(!a.holds(Role::Projection));
    }

    /// `from` hands `role` to `to`, which takes the record in.
    fn hand(from: &mut Roles, role: Role, to: &mut Roles, to_key: u8) -> Record {
        from.receive_ask(role, key(to_key), format!("Device {to_key}"));
        let record = from
            .allow(role, key(to_key), false, &net(true, &online))
            .unwrap()
            .record;
        assert!(m(to, role, &record).changed);
        record
    }

    #[test]
    fn records_merge_as_a_maximum() {
        let plain = |holder: u8, epoch: u64| Record {
            holder: Some(key(holder)),
            name: "A".into(),
            epoch,
            chain: vec![],
        };
        assert!(
            plain(1, 3).beats(&plain(9, 2)),
            "a higher epoch beats a higher key"
        );
        assert!(!plain(9, 2).beats(&plain(1, 3)));
        assert!(!plain(1, 3).beats(&plain(1, 3)));
        assert!(
            plain(9, 3).beats(&plain(1, 3)),
            "the higher key breaks a tie"
        );
        // Merging in either order, for every role, gives the same record.
        for role in Role::ALL {
            let (mut one, mut nine) = (device(1, "One"), device(9, "Nine"));
            let first = one.claim(role, &net(true, &|_| Holder::Gone));
            // Operator needs Projection held: skip the claim chain for it.
            let first = match first {
                Ok(r) => r,
                Err(_) => continue,
            };
            assert!(m(&mut nine, role, &first).changed);
            let to_nine = hand(&mut one, role, &mut nine, 9);
            let back = hand(&mut nine, role, &mut one, 1);
            assert_eq!((back.epoch, back.holder), (3, Some(key(1))));
            let mut x = device(5, "X");
            let mut y = device(6, "Y");
            m(&mut x, role, &back);
            assert!(!m(&mut x, role, &to_nine).changed, "older: ignored");
            m(&mut y, role, &to_nine);
            assert!(
                m(&mut y, role, &back).changed,
                "newer steps join the older state"
            );
            assert_eq!(x.record(role), y.record(role));
            assert_eq!(x.record(role).holder, Some(key(1)));
        }
    }

    #[test]
    fn each_role_is_its_own_record() {
        let (mut a, b) = pair();
        let n = net(true, &online);
        // Handing Operator over leaves Projection and Audio where they are.
        let mut b = b;
        b.ask(Role::Operator, &n).unwrap();
        assert_eq!(
            a.receive_ask(Role::Operator, key(2), "Office PC".into()),
            Asked::Prompt
        );
        let allowed = a.allow(Role::Operator, key(2), false, &n).unwrap();
        assert_eq!(allowed.record.epoch, 2);
        assert!(a.holds(Role::Projection) && a.holds(Role::Audio));
        assert!(!a.holds(Role::Operator));
        assert_eq!(a.record(Role::Projection).epoch, 1);
    }

    #[test]
    fn a_planning_device_must_ask_while_the_holder_is_connected() {
        let (_, mut b) = pair();
        let n = net(true, &online);
        for role in [Role::Projection, Role::Operator, Role::Audio] {
            assert_eq!(b.standing(role, &n), Standing::Ask);
            assert_eq!(b.claim(role, &n), Err(Denied::MustAsk), "{role:?}");
            assert_eq!(b.ask(role, &n), Ok(1));
            assert_eq!(
                b.asking(role),
                Some(&Asking {
                    epoch: 1,
                    state: AskState::Waiting
                })
            );
        }
        assert_eq!(b.mode(&n), Mode::Planning);
    }

    #[test]
    fn an_unheld_role_is_taken_with_one_click() {
        let (_, mut b) = pair();
        let n = net(true, &online);
        assert_eq!(b.ask(Role::Stream, &n), Err(Denied::NotNeeded));
        let record = b.claim(Role::Stream, &n).unwrap();
        assert_eq!((record.epoch, record.holder), (1, Some(key(2))));
        assert_eq!(b.claim(Role::Stream, &n), Err(Denied::Already));
    }

    #[test]
    fn allowing_hands_the_role_over_and_declining_keeps_it() {
        let (mut a, mut b) = pair();
        let n = net(true, &online);
        // Projection last: whoever holds it may take the Operator seat
        // without asking, so Operator is asked for before it moves.
        for role in [Role::Audio, Role::Operator, Role::Projection] {
            b.ask(role, &n).unwrap();
            assert_eq!(
                a.receive_ask(role, key(2), "Office PC".into()),
                Asked::Prompt
            );
            assert_eq!(a.request(role).map(|r| r.device), Some(key(2)));
            // Not now: the holder keeps it and the asker can ask again.
            assert!(a.decline(role, key(2)));
            assert!(a.request(role).is_none());
            assert!(a.holds(role));
            b.receive_answer(role, key(1), 1, Answer::Declined);
            assert_eq!(b.asking(role).unwrap().state, AskState::Declined);
            b.ask(role, &n).unwrap();
            assert_eq!(b.asking(role).unwrap().state, AskState::Waiting);
            // Now allowed.
            a.receive_ask(role, key(2), "Office PC".into());
            let allowed = a.allow(role, key(2), false, &n).unwrap();
            assert_eq!(allowed.record.epoch, 2);
            assert!(!a.holds(role));
            let merged = m(&mut b, role, &allowed.record);
            assert!(merged.changed && !merged.lost_role);
            assert!(b.holds(role));
            assert_eq!(b.asking(role), None, "the ask is settled");
            // The old holder chose this: no notice, and nothing changes
            // when it hears its own record back.
            assert!(!m(&mut a, role, &allowed.record).changed);
            assert_eq!(a.notice(role), None);
        }
    }

    #[test]
    fn only_the_holder_hears_asks_and_only_from_askers() {
        let (mut a, mut b) = pair();
        let n = net(true, &online);
        assert_eq!(
            b.receive_ask(Role::Projection, key(3), "Laptop".into()),
            Asked::Ignored,
            "b does not hold it"
        );
        assert_eq!(
            a.allow(Role::Projection, key(3), false, &n),
            Err(NotAllowed::NotAsking),
            "nobody asked"
        );
        assert_eq!(
            a.receive_ask(Role::Projection, key(2), "Office PC".into()),
            Asked::Prompt
        );
        assert_eq!(
            a.receive_ask(Role::Projection, key(2), "Office PC".into()),
            Asked::Prompt,
            "the same ask twice is one prompt"
        );
        assert!(a.receive_cancel(Role::Projection, key(2)));
        assert_eq!(
            a.allow(Role::Projection, key(2), false, &n),
            Err(NotAllowed::NotAsking),
            "withdrawn"
        );
        assert_eq!(
            b.allow(Role::Projection, key(1), false, &n),
            Err(NotAllowed::NotHolding)
        );
        assert_eq!(
            a.receive_ask(Role::Projection, key(1), "Sanctuary".into()),
            Asked::Ignored,
            "nobody asks itself"
        );
    }

    #[test]
    fn a_second_asker_is_told_who_is_asking_and_is_not_queued() {
        let (mut a, _) = pair();
        let n = net(true, &online);
        assert_eq!(
            a.receive_ask(Role::Operator, key(2), "Mary's laptop".into()),
            Asked::Prompt
        );
        assert_eq!(
            a.receive_ask(Role::Operator, key(3), "Sam's tablet".into()),
            Asked::Refuse(Answer::Busy {
                by: "Mary's laptop".into()
            })
        );
        // Even a remembered device does not jump the queue.
        a.set_always(Role::Operator, key(3), "Sam's tablet".into(), true);
        assert!(matches!(
            a.receive_ask(Role::Operator, key(3), "Sam's tablet".into()),
            Asked::Refuse(Answer::Busy { .. })
        ));
        assert_eq!(a.request(Role::Operator).map(|r| r.device), Some(key(2)));
        // Sam cannot be allowed: he is not asking.
        assert_eq!(
            a.allow(Role::Operator, key(3), false, &n),
            Err(NotAllowed::NotAsking)
        );
        // Other roles have their own prompt.
        assert_eq!(
            a.receive_ask(Role::Audio, key(3), "Sam's tablet".into()),
            Asked::Prompt
        );
    }

    #[test]
    fn always_allow_is_offered_for_operator_and_stream_only() {
        let (mut a, _) = pair();
        let n = net(true, &online);
        a.claim(Role::Stream, &n).unwrap();
        for role in [Role::Operator, Role::Stream] {
            assert!(role.offers_always());
            a.receive_ask(role, key(2), "Mary's laptop".into());
            a.allow(role, key(2), true, &n).unwrap();
            assert_eq!(a.always(role).len(), 1, "{role:?}");
        }
        for role in [Role::Projection, Role::Audio] {
            assert!(!role.offers_always());
            a.receive_ask(role, key(2), "Mary's laptop".into());
            a.allow(role, key(2), true, &n).unwrap();
            assert!(a.always(role).is_empty(), "{role:?} is never remembered");
            assert!(!a.set_always(role, key(2), "Mary's laptop".into(), true));
        }
    }

    #[test]
    fn a_remembered_device_is_answered_at_once() {
        let (mut a, _) = pair();
        let n = net(true, &online);
        a.receive_ask(Role::Operator, key(2), "Mary's laptop".into());
        a.allow(Role::Operator, key(2), true, &n).unwrap();
        assert_eq!(a.always(Role::Operator).len(), 1);
        // She stops; the Projection holder has the seat again.
        a.claim(Role::Operator, &n).unwrap();
        assert_eq!(
            a.receive_ask(Role::Operator, key(2), "Mary's MacBook".into()),
            Asked::Remembered
        );
        assert_eq!(a.always(Role::Operator)[0].name, "Mary's MacBook");
        let allowed = a.allow(Role::Operator, key(2), false, &n).unwrap();
        assert_eq!(allowed.record.name, "Mary's MacBook");
        // Someone who was never remembered is still asked.
        a.claim(Role::Operator, &n).unwrap();
        assert_eq!(
            a.receive_ask(Role::Operator, key(3), "Sam's tablet".into()),
            Asked::Prompt
        );
        assert!(a.forget(key(2)));
        assert!(a.always(Role::Operator).is_empty());
    }

    #[test]
    fn what_a_hand_over_takes_off_air() {
        assert_eq!(Role::Projection.on_air(), OnAir::Screens);
        assert_eq!(Role::Audio.on_air(), OnAir::Music);
        assert_eq!(Role::Operator.on_air(), OnAir::Nothing);
        assert_eq!(Role::Stream.on_air(), OnAir::Nothing);
        let (mut a, _) = pair();
        let n = net(true, &online);
        for role in [Role::Projection, Role::Operator, Role::Audio] {
            a.receive_ask(role, key(2), "Office PC".into());
            let allowed = a.allow(role, key(2), false, &n).unwrap();
            assert_eq!(allowed.on_air, role.on_air());
        }
    }

    #[test]
    fn concurrent_claims_resolve_to_the_higher_device_key() {
        let (mut a, b) = pair();
        let n = net(true, &online);
        // The holder vanished; both take over at the same moment.
        let gone = |d: DeviceKey| {
            if d == key(1) {
                Holder::Offline(secs(300))
            } else {
                Holder::Online
            }
        };
        let n_gone = net(true, &gone);
        assert_eq!(a.claim(Role::Projection, &n), Err(Denied::Already));
        let mut c = device(3, "Laptop");
        for role in Role::ALL {
            m(&mut c, role, a.record(role));
        }
        let mut b2 = b.clone();
        let lb = b2.claim(Role::Projection, &n_gone).unwrap();
        let lc = c.claim(Role::Projection, &n_gone).unwrap();
        assert_eq!(lb.epoch, lc.epoch, "same epoch");
        let mut x = b2.clone();
        let mut y = c.clone();
        let mx = m(&mut x, Role::Projection, &lc);
        let my = m(&mut y, Role::Projection, &lb);
        assert!(mx.changed && mx.lost_role);
        assert!(!my.changed);
        assert_eq!(x.record(Role::Projection), y.record(Role::Projection));
        assert_eq!(x.record(Role::Projection).holder, Some(key(3)));
        assert_eq!(
            x.notice(Role::Projection),
            Some(&Notice::Replaced {
                by: "Laptop".into()
            })
        );
        assert!(y.holds(Role::Projection));
    }

    #[test]
    fn takeover_only_after_the_timeout() {
        let (_, mut b) = pair();
        let at = |s: u64| move |_: DeviceKey| Holder::Offline(secs(s));
        let (f30, f119, f120, f300) = (at(30), at(119), at(120), at(300));
        for role in [Role::Projection, Role::Audio] {
            assert_eq!(
                b.standing(role, &net(true, &f30)),
                Standing::Wait {
                    offline: secs(30),
                    remaining: secs(90)
                }
            );
            assert_eq!(
                b.claim(role, &net(true, &f119)),
                Err(Denied::TooSoon {
                    offline: secs(119),
                    remaining: secs(1)
                })
            );
            assert_eq!(b.ask(role, &net(true, &f30)), Err(Denied::NobodyToAsk));
            assert!(!b.holds(role), "a refused claim changes nothing");
            assert_eq!(
                b.standing(role, &net(true, &f120)),
                Standing::TakeOver { offline: secs(120) }
            );
            assert_eq!(b.ask(role, &net(true, &f300)), Err(Denied::NotNeeded));
        }
        let record = b.claim(Role::Projection, &net(true, &f300)).unwrap();
        assert_eq!((record.epoch, record.holder), (2, Some(key(2))));
        assert_eq!(b.mode(&net(true, &online)), Mode::Presenter);
    }

    #[test]
    fn the_timeout_is_configurable() {
        let (_, mut b) = pair();
        let off = |s: u64| move |_: DeviceKey| Holder::Offline(secs(s));
        let (f300, f61) = (off(300), off(61));
        b.set_takeover_after(secs(600));
        assert!(b.claim(Role::Audio, &net(true, &f300)).is_err());
        b.set_takeover_after(secs(60));
        assert!(b.claim(Role::Audio, &net(true, &f61)).is_ok());
        b.set_takeover_after(Duration::ZERO);
        assert_eq!(b.takeover_after(), MIN_TAKEOVER);
    }

    #[test]
    fn a_returning_holder_yields_and_is_told() {
        let (mut a, mut b) = pair();
        let off = |_: DeviceKey| Holder::Offline(secs(400));
        let lease = b.claim(Role::Projection, &net(true, &off)).unwrap();
        // `a` never knew: it still believes it presents until it hears.
        assert!(a.holds(Role::Projection));
        let merged = m(&mut a, Role::Projection, &lease);
        assert!(merged.lost_role);
        assert!(!a.holds(Role::Projection));
        assert_eq!(
            a.notice(Role::Projection),
            Some(&Notice::Replaced {
                by: "Office PC".into()
            })
        );
        // The other roles are untouched.
        assert!(a.holds(Role::Operator) && a.holds(Role::Audio));
        // Its own older record can never win it back.
        assert!(
            !m(
                &mut a,
                Role::Projection,
                &Record {
                    holder: Some(key(1)),
                    name: "Sanctuary".into(),
                    epoch: 1,
                    chain: vec![],
                },
            )
            .changed
        );
        assert!(!a.holds(Role::Projection));
        a.dismiss_notice(Role::Projection);
        assert_eq!(a.notice(Role::Projection), None);
    }

    #[test]
    fn projection_is_refused_on_a_device_with_no_screens() {
        let (_, mut b) = pair();
        b.set_screens(false);
        let n = net(true, &online);
        assert_eq!(b.blocked(Role::Projection, &n), Some(Blocked::NoScreens));
        assert_eq!(b.ask(Role::Projection, &n), Err(Denied::NoScreens));
        assert_eq!(b.claim(Role::Projection, &n), Err(Denied::NoScreens));
        assert_eq!(
            Denied::NoScreens.to_string(),
            "No screens are set up on this device"
        );
        let off = |_: DeviceKey| Holder::Offline(secs(999));
        assert_eq!(
            b.claim(Role::Projection, &net(true, &off)),
            Err(Denied::NoScreens),
            "not even after the holder has been gone a long time"
        );
        // Nothing else needs screens.
        for role in [Role::Operator, Role::Audio, Role::Stream] {
            assert_eq!(b.blocked(role, &n), None);
        }
        // Setting screens up opens the way.
        assert!(b.set_screens(true));
        assert_eq!(b.blocked(Role::Projection, &n), None);
        assert_eq!(b.ask(Role::Projection, &n), Ok(1));
    }

    #[test]
    fn a_holder_keeps_projection_when_its_screens_go() {
        let (mut a, _) = pair();
        a.set_screens(false);
        let n = net(true, &online);
        assert!(a.holds(Role::Projection), "it does not move by itself");
        assert_eq!(a.blocked(Role::Projection, &n), None);
        assert_eq!(a.mode(&n), Mode::Presenter);
    }

    #[test]
    fn operator_needs_projection_held() {
        let mut b = device(2, "Office PC");
        let gone = |_: DeviceKey| Holder::Gone;
        let n = net(true, &gone);
        // Nobody holds Projection: Operator cannot be taken or asked for.
        assert_eq!(
            b.blocked(Role::Operator, &n),
            Some(Blocked::NeedsProjection)
        );
        assert_eq!(b.claim(Role::Operator, &n), Err(Denied::NeedsProjection));
        assert_eq!(b.ask(Role::Operator, &n), Err(Denied::NeedsProjection));
        assert!(!b.operator_ready(&n));
        // Stream and Audio need nothing.
        assert!(b.claim(Role::Stream, &n).is_ok());
        assert!(b.claim(Role::Audio, &n).is_ok());
        // Once Projection is held, Operator can be taken.
        m(
            &mut b,
            Role::Projection,
            &Record {
                holder: Some(key(1)),
                name: "Sanctuary".into(),
                epoch: 1,
                chain: vec![],
            },
        );
        assert_eq!(
            b.claim(Role::Operator, &net(true, &online))
                .map(|r| r.epoch),
            Ok(1)
        );
    }

    #[test]
    fn operator_controls_wait_while_projection_is_unreachable_and_the_seat_is_kept() {
        let (a, mut b) = pair();
        // b is the Operator now.
        let mut a = a;
        let n = net(true, &online);
        b.ask(Role::Operator, &n).unwrap();
        a.receive_ask(Role::Operator, key(2), "Office PC".into());
        let allowed = a.allow(Role::Operator, key(2), false, &n).unwrap();
        m(&mut b, Role::Operator, &allowed.record);
        assert!(b.holds(Role::Operator));
        assert!(b.operator_ready(&n));
        // The Projection holder drops: the controls wait.
        let off = |_: DeviceKey| Holder::Offline(secs(10));
        assert!(!b.operator_ready(&net(true, &off)));
        assert!(b.holds(Role::Operator), "the seat is kept");
        // Projection unpaired altogether: still kept, controls still wait.
        let gone = |_: DeviceKey| Holder::Gone;
        assert!(!b.operator_ready(&net(true, &gone)));
        assert!(b.holds(Role::Operator));
        // It wakes when Projection is held again.
        assert!(b.operator_ready(&n));
    }

    #[test]
    fn the_operator_seat_is_kept_across_a_projection_hand_over() {
        let (mut a, mut b) = pair();
        let n = net(true, &online);
        // b becomes Operator, then Projection moves to a third device.
        b.ask(Role::Operator, &n).unwrap();
        a.receive_ask(Role::Operator, key(2), "Office PC".into());
        let op = a.allow(Role::Operator, key(2), false, &n).unwrap();
        m(&mut b, Role::Operator, &op.record);
        let mut c = device(3, "Back room");
        for role in Role::ALL {
            m(&mut c, role, a.record(role));
        }
        m(&mut c, Role::Operator, &op.record);
        c.ask(Role::Projection, &n).unwrap();
        a.receive_ask(Role::Projection, key(3), "Back room".into());
        let proj = a.allow(Role::Projection, key(3), false, &n).unwrap();
        m(&mut b, Role::Projection, &proj.record);
        assert!(b.holds(Role::Operator), "still the Operator");
        assert_eq!(b.record(Role::Projection).holder, Some(key(3)));
    }

    #[test]
    fn the_projection_holder_takes_the_operator_seat_back_at_once() {
        let (mut a, mut b) = pair();
        let n = net(true, &online);
        b.ask(Role::Operator, &n).unwrap();
        a.receive_ask(Role::Operator, key(2), "Office PC".into());
        let op = a.allow(Role::Operator, key(2), false, &n).unwrap();
        m(&mut b, Role::Operator, &op.record);
        assert_eq!(a.standing(Role::Operator, &n), Standing::Open);
        let back = a.claim(Role::Operator, &n).unwrap();
        assert_eq!(back.epoch, 3);
        let merged = m(&mut b, Role::Operator, &back);
        assert!(merged.lost_role);
        assert_eq!(
            b.notice(Role::Operator),
            Some(&Notice::Replaced {
                by: "Sanctuary".into()
            })
        );
        // Nobody else may do that.
        let mut c = device(3, "Back room");
        for role in Role::ALL {
            m(&mut c, role, a.record(role));
        }
        assert_eq!(c.claim(Role::Operator, &n), Err(Denied::MustAsk));
    }

    #[test]
    fn giving_the_operator_seat_up_on_purpose_shows_no_notice() {
        let (mut a, mut b) = pair();
        let n = net(true, &online);
        b.ask(Role::Operator, &n).unwrap();
        a.receive_ask(Role::Operator, key(2), "Office PC".into());
        let op = a.allow(Role::Operator, key(2), false, &n).unwrap();
        m(&mut b, Role::Operator, &op.record);
        assert!(b.yield_role(Role::Operator));
        assert!(!a.yield_role(Role::Stream), "not held, nothing to give");
        let back = a.claim(Role::Operator, &n).unwrap();
        assert!(m(&mut b, Role::Operator, &back).lost_role);
        assert_eq!(b.notice(Role::Operator), None);
    }

    #[test]
    fn busy_and_declined_answers_only_count_from_the_holder_at_that_epoch() {
        let (_, mut b) = pair();
        let n = net(true, &online);
        b.ask(Role::Operator, &n).unwrap();
        b.receive_answer(Role::Operator, key(7), 1, Answer::Declined);
        assert_eq!(b.asking(Role::Operator).unwrap().state, AskState::Waiting);
        b.receive_answer(Role::Operator, key(1), 9, Answer::Declined);
        assert_eq!(b.asking(Role::Operator).unwrap().state, AskState::Waiting);
        b.receive_answer(
            Role::Operator,
            key(1),
            1,
            Answer::Busy {
                by: "Sam's tablet".into(),
            },
        );
        assert_eq!(
            b.asking(Role::Operator).unwrap().state,
            AskState::Busy {
                by: "Sam's tablet".into()
            }
        );
        b.clear_answer(Role::Operator);
        assert_eq!(b.asking(Role::Operator), None);
        b.ask(Role::Operator, &n).unwrap();
        b.clear_answer(Role::Operator);
        assert!(b.asking(Role::Operator).is_some(), "a waiting ask stays");
        assert!(b.cancel_ask(Role::Operator));
        assert!(!b.cancel_ask(Role::Operator));
    }

    #[test]
    fn an_epoch_nothing_could_beat_is_not_taken_in_and_none_is_made() {
        let (_, mut b) = pair();
        let forged = |epoch| Record {
            holder: Some(key(9)),
            name: "x".into(),
            epoch,
            chain: vec![],
        };
        for role in Role::ALL {
            for epoch in [u64::MAX, u64::MAX - 1, EPOCH_LIMIT + 1] {
                assert!(!m(&mut b, role, &forged(epoch)).changed, "{role:?} {epoch}");
            }
        }
        assert_eq!(b.record(Role::Projection).epoch, 1);
        // Taking at the top of the range stays at the top: no wrap, and no
        // epoch past the limit is ever made.
        let mut top = device(3, "Laptop");
        m(&mut top, Role::Stream, &forged(EPOCH_LIMIT));
        let off = |_: DeviceKey| Holder::Offline(secs(999));
        let taken = top.claim(Role::Stream, &net(true, &off)).unwrap();
        assert_eq!(taken.epoch, EPOCH_LIMIT);
    }

    #[test]
    fn a_claim_that_jumps_far_is_not_taken_on_the_claimants_word() {
        // The attack: a claim from "nothing" at an epoch just short of the
        // end, from the device that signs it, would wedge the role.
        let (mut a, mut b) = pair();
        let n = net(true, &online);
        let stream = a.claim(Role::Stream, &n).unwrap();
        m(&mut b, Role::Stream, &stream);
        // Mallory signs a claim at a far epoch by hand.
        let far = EPOCH_LIMIT - 2;
        let sig = kp(9)
            .sign(&Link::bytes(
                Role::Stream,
                Kind::Claim,
                Some(key(9)),
                far,
                None,
                far - 1,
            ))
            .to_vec();
        let forged = Record {
            holder: Some(key(9)),
            name: "Mallory".into(),
            epoch: far,
            chain: vec![Link {
                kind: Kind::Claim,
                holder: Some(key(9)),
                epoch: far,
                prev_holder: None,
                prev_epoch: far - 1,
                sig,
            }],
        };
        let gone = |_: DeviceKey| Holder::Gone;
        assert!(
            !b.merge(Role::Stream, &forged, key(9), &net(true, &gone))
                .changed
        );
        assert_eq!(b.record(Role::Stream).epoch, 1);
        // The same, one step past what could have been missed.
        let mut near = forged.clone();
        near.epoch = 1 + CHAIN as u64 + 1;
        near.chain[0].epoch = near.epoch;
        near.chain[0].prev_epoch = near.epoch - 1;
        near.chain[0].sig = kp(9)
            .sign(&Link::bytes(
                Role::Stream,
                Kind::Claim,
                Some(key(9)),
                near.epoch,
                None,
                near.epoch - 1,
            ))
            .to_vec();
        assert!(
            !b.merge(Role::Stream, &near, key(9), &net(true, &gone))
                .changed
        );
    }

    #[test]
    fn letting_go_with_nobody_to_hand_to_leaves_the_role_unheld() {
        let (mut a, mut b) = pair();
        assert!(b.release(Role::Operator).is_none(), "not held");
        let record = a.release(Role::Operator).unwrap();
        assert_eq!((record.holder, record.epoch), (None, 2));
        assert!(!a.holds(Role::Operator));
        assert!(m(&mut b, Role::Operator, &record).changed);
        assert_eq!(a.notice(Role::Operator), None);
    }

    #[test]
    fn a_holder_that_was_unpaired_leaves_the_seat_open() {
        let (_, b) = pair();
        let gone = |_: DeviceKey| Holder::Gone;
        let n = net(true, &gone);
        for role in [Role::Projection, Role::Operator, Role::Audio] {
            assert_eq!(b.holder_of(role, &n), Holder::Gone);
        }
        assert_eq!(b.standing(Role::Projection, &n), Standing::Open);
        assert_eq!(b.standing(Role::Audio, &n), Standing::Open);
    }

    #[test]
    fn forgetting_a_device_drops_its_ask_and_its_standing_permission() {
        let (mut a, _) = pair();
        let n = net(true, &online);
        a.receive_ask(Role::Operator, key(2), "Office PC".into());
        a.allow(Role::Operator, key(2), true, &n).unwrap();
        a.receive_ask(Role::Audio, key(2), "Office PC".into());
        assert!(a.forget(key(2)));
        assert!(a.request(Role::Audio).is_none());
        assert!(a.always(Role::Operator).is_empty());
        assert!(!a.forget(key(2)));
    }

    // ---- Signed records ----------------------------------------------------

    /// Four devices that agree `a` (key 1) holds Projection and Stream,
    /// built the way they would be: a claims, the others take the record in.
    fn church() -> (Roles, Roles, Roles) {
        let (mut a, mut b) = pair();
        let mut c = device(3, "Laptop");
        for role in Role::ALL {
            if a.record(role).holder.is_some() {
                m(&mut c, role, a.record(role));
            }
        }
        let n = net(true, &online);
        let stream = a.claim(Role::Stream, &n).unwrap();
        m(&mut b, Role::Stream, &stream);
        m(&mut c, Role::Stream, &stream);
        (a, b, c)
    }

    #[test]
    fn a_forged_record_from_a_paired_device_is_ignored() {
        let (mut a, mut b, _) = church();
        let before: Vec<Record> = Role::ALL.iter().map(|r| a.record(*r).clone()).collect();
        let genuine = {
            b.ask(Role::Audio, &net(true, &online)).unwrap();
            a.receive_ask(Role::Audio, key(2), "Office PC".into());
            a.allow(Role::Audio, key(2), false, &net(true, &online))
                .unwrap()
                .record
        };
        // Nothing but the genuine record changes anything, whoever sends it.
        let mut stolen = genuine.clone();
        stolen.holder = Some(key(3));
        stolen.chain[0].holder = Some(key(3));
        let mut bumped = genuine.clone();
        bumped.epoch += 1;
        bumped.chain[0].epoch += 1;
        let mut unsigned = genuine.clone();
        unsigned.chain.clear();
        let mut wrong_signer = genuine.clone();
        wrong_signer.chain[0].sig = kp(3)
            .sign(&Link::bytes(
                Role::Audio,
                Kind::HandOver,
                Some(key(2)),
                2,
                Some(key(1)),
                1,
            ))
            .to_vec();
        let mut short = genuine.clone();
        short.chain[0].sig.truncate(10);
        for (what, forged) in [
            ("a stolen holder", stolen),
            ("a bumped epoch", bumped),
            ("no steps", unsigned),
            ("another device's signature", wrong_signer),
            ("a short signature", short),
        ] {
            // From a third device that is paired with this one.
            let mut c = device(3, "Laptop");
            for role in Role::ALL {
                m(&mut c, role, &before[role.index()]);
            }
            let kept = c.record(Role::Audio).clone();
            let merged = c.merge(Role::Audio, &forged, key(3), &net(true, &online));
            assert!(!merged.changed, "{what}");
            assert_eq!(c.record(Role::Audio), &kept, "{what}");
            assert!(!c.unverified(Role::Audio), "{what}");
        }
        // The genuine record does.
        let mut c = device(3, "Laptop");
        for role in Role::ALL {
            m(&mut c, role, &before[role.index()]);
        }
        assert!(
            c.merge(Role::Audio, &genuine, key(1), &net(true, &online))
                .changed
        );
    }

    #[test]
    fn a_stranger_who_signs_for_itself_cannot_take_a_role_somebody_holds() {
        let (mut a, b, _) = church();
        // A device the church never paired makes its own claim on Projection
        // as if it had seen the record, and a paired device passes it on.
        let mut stranger = device(9, "Stranger");
        m(&mut stranger, Role::Projection, a.record(Role::Projection));
        let gone = |d: DeviceKey| {
            if d == key(9) {
                Holder::Gone
            } else {
                Holder::Online
            }
        };
        let take = stranger
            .claim(
                Role::Projection,
                &net_away(
                    &|d| {
                        if d == key(1) {
                            Holder::Offline(secs(999))
                        } else {
                            gone(d)
                        }
                    },
                    None,
                ),
            )
            .unwrap();
        // Another device has never heard of the stranger's key: no. Even
        // passed on by a paired device, a step signed by an invented key is
        // not trusted, and it is not a "gap" either.
        let mut b = b;
        let mut b_net = net(true, &online);
        b_net.strangers.push(key(9));
        assert!(!b.merge(Role::Projection, &take, key(2), &b_net).changed);
        assert!(!b.unverified(Role::Projection));
        assert!(!b.forget_unverified(), "nothing was wiped");
        // The holder itself: the stranger is not paired with it, so it
        // cannot have been out of reach.
        assert!(
            !a.merge(Role::Projection, &take, key(2), &net_away(&online, None))
                .changed
        );
        // And a paired device that had only just reconnected has not been
        // away long enough either.
        assert!(
            !a.merge(
                Role::Projection,
                &take,
                key(9),
                &net_away(&online, Some(secs(5)))
            )
            .changed
        );
        assert!(a.holds(Role::Projection));
        assert_eq!(a.notice(Role::Projection), None);
    }

    #[test]
    fn a_holder_none_clear_needs_the_holders_own_signature() {
        let (mut a, mut b, _) = church();
        let n = net(true, &online);
        // A clear with no steps, or signed by somebody else, does nothing.
        let unsigned = Record {
            holder: None,
            name: String::new(),
            epoch: 2,
            chain: vec![],
        };
        assert!(!m(&mut b, Role::Stream, &unsigned).changed);
        let mut by_b = device(2, "Office PC");
        m(&mut by_b, Role::Stream, a.record(Role::Stream));
        let mut stolen = a.clone();
        let forged = stolen.release(Role::Stream).unwrap();
        // `release` signed by a, but a record that says b let go is not.
        let mut says_b = forged.clone();
        says_b.chain[0].prev_holder = Some(key(2));
        assert!(!b.merge(Role::Stream, &says_b, key(2), &n).changed);
        assert_eq!(b.record(Role::Stream).holder, Some(key(1)));
        // The holder's own release does.
        let released = a.release(Role::Stream).unwrap();
        assert!(b.merge(Role::Stream, &released, key(1), &n).changed);
        assert_eq!(b.record(Role::Stream).holder, None);
    }

    #[test]
    fn a_record_cannot_be_replayed_onto_another_role_or_epoch() {
        let (mut a, mut b, _) = church();
        let n = net(true, &online);
        let before: Vec<Record> = Role::ALL.iter().map(|r| a.record(*r).clone()).collect();
        // A genuine hand-over of Audio...
        b.ask(Role::Audio, &n).unwrap();
        a.receive_ask(Role::Audio, key(2), "Office PC".into());
        let audio = a.allow(Role::Audio, key(2), false, &n).unwrap().record;
        // ...replayed as a hand-over of Operator, or of Stream (a holds both).
        for other in [Role::Operator, Role::Stream] {
            let mut c = device(3, "Laptop");
            for role in Role::ALL {
                m(&mut c, role, &before[role.index()]);
            }
            // a still holds `other` as far as c knows; c has not heard of
            // the hand-over, and the steps do not carry across roles.
            let mut replay = audio.clone();
            replay.epoch = c.record(other).epoch + 1;
            replay.chain[0].epoch = replay.epoch;
            replay.chain[0].prev_epoch = c.record(other).epoch;
            assert!(!c.merge(other, &replay, key(1), &n).changed, "{other:?}");
            let unchanged = audio.clone();
            assert!(!c.merge(other, &unchanged, key(1), &n).changed, "{other:?}");
        }
        // An old record replayed after the role has moved on changes nothing.
        let mut c = device(3, "Laptop");
        for role in Role::ALL {
            m(&mut c, role, &before[role.index()]);
        }
        assert!(m(&mut c, Role::Audio, &audio).changed);
        let back = {
            b.receive_ask(Role::Audio, key(1), "Sanctuary".into());
            m(&mut b, Role::Audio, &audio);
            b.receive_ask(Role::Audio, key(1), "Sanctuary".into());
            b.allow(Role::Audio, key(1), false, &n).unwrap().record
        };
        assert!(m(&mut c, Role::Audio, &back).changed);
        assert!(!m(&mut c, Role::Audio, &audio).changed, "an old record");
        assert_eq!(c.record(Role::Audio).holder, Some(key(1)));
    }

    /// `holder` passes `role` on through `n` hand-overs between the given
    /// devices in turn, and returns the records after each.
    fn pass_around(devices: &mut [Roles], first: usize, role: Role, hops: usize) -> Vec<Record> {
        let n = net(true, &online);
        let mut at = first;
        let mut out = Vec::new();
        for _ in 0..hops {
            let to = (at + 1) % devices.len();
            let to_key = devices[to].me;
            let from_key = devices[at].me;
            devices[at].receive_ask(role, to_key, "x".into());
            let record = devices[at].allow(role, to_key, false, &n).unwrap().record;
            m(&mut devices[to], role, &record);
            let _ = from_key;
            out.push(record);
            at = to;
        }
        out
    }

    #[test]
    fn a_hand_over_relayed_by_a_device_paired_with_only_one_peer_is_taken() {
        // t is paired only with s. o hands Stream to p and p to q; t hears
        // of it only from s, which has never been the holder.
        let mut ring = vec![device(1, "O"), device(2, "P"), device(3, "Q")];
        let first = ring[0].claim(Role::Stream, &net(true, &online)).unwrap();
        let mut t = device(8, "T");
        let only_s = |d: DeviceKey| {
            if d == key(7) {
                Holder::Online
            } else {
                Holder::Gone
            }
        };
        let s_net = net(true, &only_s);
        assert!(t.merge(Role::Stream, &first, key(7), &s_net).changed);
        let records = pass_around(&mut ring, 0, Role::Stream, 2);
        // Just the last record, from s: its steps carry the first hop too.
        let last = records.last().unwrap();
        assert_eq!(last.holder, Some(key(3)));
        let merged = t.merge(Role::Stream, last, key(7), &s_net);
        assert!(merged.changed);
        assert_eq!(t.record(Role::Stream).holder, Some(key(3)));
        assert_eq!(t.record(Role::Stream).epoch, 3);
        // And a hand-over after that, one hop at a time.
        let more = pass_around(&mut ring, 2, Role::Stream, 1);
        assert!(t.merge(Role::Stream, &more[0], key(7), &s_net).changed);
        assert_eq!(t.record(Role::Stream).holder, Some(key(1)));
    }

    #[test]
    fn a_session_open_sync_after_missed_hand_overs_catches_up() {
        let mut ring = vec![device(1, "O"), device(2, "P"), device(3, "Q")];
        let first = ring[0].claim(Role::Stream, &net(true, &online)).unwrap();
        let mut t = device(8, "T");
        assert!(m(&mut t, Role::Stream, &first).changed);
        // It misses six hand-overs, then meets a device that has them all.
        let records = pass_around(&mut ring, 0, Role::Stream, 6);
        let now = records.last().unwrap();
        assert_eq!(now.epoch, 7);
        assert!(now.chain.len() <= CHAIN);
        assert!(
            t.merge(Role::Stream, now, key(2), &net(true, &online))
                .changed
        );
        assert_eq!(t.record(Role::Stream), now);
        assert!(!t.unverified(Role::Stream));
    }

    #[test]
    fn a_gap_longer_than_the_chain_waits_as_unverified_until_something_joins_it() {
        let mut ring = vec![device(1, "O"), device(2, "P"), device(3, "Q")];
        let first = ring[0].claim(Role::Stream, &net(true, &online)).unwrap();
        let mut t = device(8, "T");
        m(&mut t, Role::Stream, &first);
        let records = pass_around(&mut ring, 0, Role::Stream, 12);
        let now = records.last().unwrap();
        assert_eq!(
            now.chain.len(),
            CHAIN,
            "a record carries at most eight steps"
        );
        // The present record does not join what t knows: not taken, flagged.
        let merged = t.merge(Role::Stream, now, key(1), &net(true, &online));
        assert!(!merged.changed);
        assert!(t.unverified(Role::Stream));
        assert_eq!(t.record(Role::Stream), &first);
        // An older record from a device that is behind joins it...
        let middle = &records[4];
        assert!(
            t.merge(Role::Stream, middle, key(2), &net(true, &online))
                .changed
        );
        assert!(!t.unverified(Role::Stream));
        // ...and then the present one joins that.
        assert!(
            t.merge(Role::Stream, now, key(1), &net(true, &online))
                .changed
        );
        assert_eq!(t.record(Role::Stream), now);
    }

    #[test]
    fn a_claim_after_more_steps_than_a_record_carries_waits_instead_of_being_taken_on_trust() {
        let mut ring = vec![device(1, "O"), device(2, "P"), device(3, "Q")];
        let first = ring[0].claim(Role::Stream, &net(true, &online)).unwrap();
        let mut t = device(8, "T");
        m(&mut t, Role::Stream, &first);
        let records = pass_around(&mut ring, 0, Role::Stream, 12);
        let now = records.last().unwrap();
        let mut taker = device(5, "Taker");
        assert!(
            m(&mut taker, Role::Stream, now).changed,
            "a new device takes the present as it is"
        );
        let holder_key = now.holder.unwrap();
        let off = |d: DeviceKey| {
            if d == holder_key {
                Holder::Offline(secs(999))
            } else {
                Holder::Online
            }
        };
        let claim = taker.claim(Role::Stream, &net(true, &off)).unwrap();
        // T is thirteen steps behind: it cannot tell the claim from a jump
        // an attacker made up, from the claimant or from anyone else.
        let gone = |_: DeviceKey| Holder::Gone;
        for from in [key(5), key(7)] {
            assert!(
                !t.merge(Role::Stream, &claim, from, &net(true, &gone))
                    .changed
            );
        }
        assert!(t.unverified(Role::Stream));
        assert_eq!(t.record(Role::Stream), &first);
        // Pairing again starts it over, and it takes the present state.
        assert!(t.forget_unverified());
        assert_eq!(t.record(Role::Stream), &Record::default());
        t.seed_any = false;
        t.seed_from(key(5));
        assert!(
            !t.merge(Role::Stream, &claim, key(7), &net(true, &gone))
                .changed
        );
        assert!(
            t.merge(Role::Stream, &claim, key(5), &net(true, &gone))
                .changed
        );
    }

    #[test]
    fn two_claims_at_the_same_moment_agree_on_the_higher_key_with_signed_steps() {
        let mut x = device(4, "X");
        let mut y = device(5, "Y");
        let n = net(true, &|_| Holder::Gone);
        let cx = x.claim(Role::Stream, &n).unwrap();
        let cy = y.claim(Role::Stream, &n).unwrap();
        assert_eq!(cx.epoch, cy.epoch);
        let (hi, lo) = if key(5) > key(4) {
            (&mut y, &mut x)
        } else {
            (&mut x, &mut y)
        };
        let (c_hi, c_lo) = if key(5) > key(4) { (cy, cx) } else { (cx, cy) };
        assert!(
            !m(hi, Role::Stream, &c_lo).changed,
            "the higher key keeps it"
        );
        assert!(
            m(lo, Role::Stream, &c_hi).changed,
            "and takes it from the other"
        );
        assert_eq!(
            hi.record(Role::Stream).holder,
            lo.record(Role::Stream).holder
        );
        assert_eq!(hi.record(Role::Stream).holder, Some(key(5).max(key(4))));
    }

    #[test]
    fn unsigned_state_from_0_7_0_is_trusted_once_and_the_first_new_step_signs_from_there() {
        let migrated = |n: u8| {
            let mut r = Roles::new(
                key(n),
                format!("Device {n}"),
                [
                    Record {
                        holder: Some(key(2)),
                        name: "Office PC".into(),
                        epoch: 2,
                        chain: vec![],
                    },
                    Record::default(),
                    Record::default(),
                    Record::default(),
                ],
                DEFAULT_TAKEOVER,
                Default::default(),
                Signer::new(move |msg| kp(n).sign(msg)),
            );
            r.set_screens(true);
            r
        };
        let (mut office, mut sanctuary) = (migrated(2), migrated(1));
        let n = net(true, &online);
        // The same state on both: an unsigned record equal to it changes
        // nothing, and one that would change anything is refused.
        let same = office.record(Role::Projection).clone();
        assert!(!m(&mut sanctuary, Role::Projection, &same).changed);
        let other = Record {
            holder: Some(key(3)),
            name: "x".into(),
            epoch: 5,
            chain: vec![],
        };
        // The other 0.7.0 device was ahead (a lease epoch higher): the one
        // behind takes its moved-over state, as 0.7.0 merged leases. Not a
        // jump that could not have been missed.
        let mut far = other.clone();
        far.epoch = 2 + CHAIN as u64 + 1;
        assert!(!m(&mut sanctuary, Role::Projection, &far).changed);
        assert!(m(&mut sanctuary, Role::Projection, &other).changed);
        assert_eq!(sanctuary.record(Role::Projection), &other);
        // Back to the shared start for the rest.
        let mut sanctuary = migrated(1);
        // The first new step is signed and anchored on the migrated state.
        sanctuary.ask(Role::Projection, &n).unwrap();
        office.receive_ask(Role::Projection, key(1), "Sanctuary".into());
        let step = office
            .allow(Role::Projection, key(1), false, &n)
            .unwrap()
            .record;
        assert_eq!(step.chain.len(), 1);
        assert_eq!(
            (step.chain[0].prev_holder, step.chain[0].prev_epoch),
            (Some(key(2)), 2)
        );
        assert!(m(&mut sanctuary, Role::Projection, &step).changed);
        assert_eq!(sanctuary.record(Role::Projection).epoch, 3);
        // A device with nothing of its own takes the unsigned state as it is.
        let mut fresh = device(6, "New laptop");
        assert!(m(&mut fresh, Role::Projection, &same).changed);
        assert_eq!(fresh.record(Role::Projection).holder, Some(key(2)));
        // And then follows signed steps from it.
        assert!(m(&mut fresh, Role::Projection, &step).changed);
    }

    #[test]
    fn a_device_that_holds_a_role_yields_to_a_claim_only_from_one_that_was_away() {
        let (mut a, _, _) = church();
        let mut c = device(3, "Laptop");
        for role in Role::ALL {
            m(&mut c, role, a.record(role));
        }
        let off = |_: DeviceKey| Holder::Offline(secs(999));
        let claim = c.claim(Role::Audio, &net(true, &off)).unwrap();
        // Connected the whole time: refused. Away for the takeover time: yes.
        assert!(
            !a.merge(
                Role::Audio,
                &claim,
                key(3),
                &net_away(&online, Some(secs(3)))
            )
            .changed
        );
        assert!(a.holds(Role::Audio));
        let merged = a.merge(
            Role::Audio,
            &claim,
            key(3),
            &net_away(&online, Some(secs(500))),
        );
        assert!(merged.changed && merged.lost_role);
        assert!(a.notice(Role::Audio).is_some());
    }

    #[test]
    fn a_takeover_inside_a_catch_up_chain_is_taken_though_the_old_holder_is_back() {
        // O holds Stream. O is away; P takes it; O returns and yields; P
        // hands it to Q. T missed all of it, and sees O online now.
        let mut ring = [device(1, "O"), device(2, "P"), device(3, "Q")];
        let first = ring[0].claim(Role::Stream, &net(true, &online)).unwrap();
        let mut t = device(8, "T");
        m(&mut t, Role::Stream, &first);
        let o_gone = |d: DeviceKey| {
            if d == key(1) {
                Holder::Offline(secs(999))
            } else {
                Holder::Online
            }
        };
        m(&mut ring[1], Role::Stream, &first);
        m(&mut ring[2], Role::Stream, &first);
        let claim = ring[1].claim(Role::Stream, &net(true, &o_gone)).unwrap();
        // O comes back and meets the claim: it had been away.
        assert!(m(&mut ring[0], Role::Stream, &claim).lost_role);
        assert!(
            ring[2]
                .merge(Role::Stream, &claim, key(2), &net(true, &o_gone))
                .changed
        );
        ring[1].receive_ask(Role::Stream, key(3), "Q".into());
        let to_q = ring[1]
            .allow(Role::Stream, key(3), false, &net(true, &online))
            .unwrap()
            .record;
        assert_eq!(to_q.chain.len(), 3);
        assert_eq!(to_q.chain[1].kind, Kind::Claim);
        // T sees O online: judging the old claim by that would refuse it.
        let merged = t.merge(Role::Stream, &to_q, key(3), &net(true, &online));
        assert!(
            merged.changed,
            "the old claim is checked for its signature only"
        );
        assert_eq!(t.record(Role::Stream).holder, Some(key(3)));
    }

    #[test]
    fn a_record_with_more_than_eight_steps_is_refused_before_any_signature_is_checked() {
        let mut ring = vec![device(1, "O"), device(2, "P"), device(3, "Q")];
        let first = ring[0].claim(Role::Stream, &net(true, &online)).unwrap();
        let mut t = device(8, "T");
        m(&mut t, Role::Stream, &first);
        let records = pass_around(&mut ring, 0, Role::Stream, 12);
        let now = records.last().unwrap().clone();
        assert_eq!(now.chain.len(), CHAIN);
        // Eight valid steps that do not join: a gap, not an attack.
        assert!(!m(&mut t, Role::Stream, &now).changed);
        assert!(t.unverified(Role::Stream));
        t.forget_unverified();
        m(&mut t, Role::Stream, &first);
        // One more step, every one of them validly signed and chained: no.
        let mut long = now.clone();
        long.chain.push(records[4].chain[0].clone());
        assert_eq!(long.chain.len(), CHAIN + 1);
        assert!(!m(&mut t, Role::Stream, &long).changed);
        assert!(!t.unverified(Role::Stream), "refused, not waited for");
        // And such a record cannot even be read.
        let json = serde_json::to_string(&long).unwrap();
        assert!(serde_json::from_str::<Record>(&json).is_err());
        assert!(serde_json::from_str::<Record>(&serde_json::to_string(&now).unwrap()).is_ok());
    }

    #[test]
    fn steps_signed_by_invented_keys_are_refused_outright_and_wipe_nothing() {
        let mut ring = vec![device(1, "O"), device(2, "P"), device(3, "Q")];
        let first = ring[0].claim(Role::Stream, &net(true, &online)).unwrap();
        let mut t = device(8, "T");
        m(&mut t, Role::Stream, &first);
        let records = pass_around(&mut ring, 0, Role::Stream, 12);
        let now = records.last().unwrap();
        // P and Q are devices T has never heard of. Which of them signed what
        // is invisible to it: the chain does not join, and nobody it knows
        // vouches for the others: no gap flag, no state wiped.
        let mut n = net(true, &online);
        n.strangers = vec![key(2), key(3)];
        assert!(!t.merge(Role::Stream, now, key(7), &n).changed);
        assert!(!t.unverified(Role::Stream));
        assert!(!t.forget_unverified());
        assert_eq!(t.record(Role::Stream), &first);
        // The same chain from devices T does know is a gap.
        assert!(!m(&mut t, Role::Stream, now).changed);
        assert!(t.unverified(Role::Stream));

        // A chain that joins T's state but is signed by a key nobody
        // introduced is refused too.
        let mut t2 = device(9, "T2");
        m(&mut t2, Role::Stream, &first);
        let mut invented = device(5, "Invented");
        m(&mut invented, Role::Stream, &first);
        let take = invented
            .claim(Role::Stream, &net(true, &|_| Holder::Gone))
            .unwrap();
        let mut n2 = net(true, &online);
        n2.strangers = vec![key(5)];
        assert!(!t2.merge(Role::Stream, &take, key(7), &n2).changed);
        assert!(!t2.unverified(Role::Stream));
    }

    #[test]
    fn a_device_with_nothing_takes_the_present_only_from_the_device_it_just_paired_with() {
        let mut ring = vec![device(1, "O"), device(2, "P")];
        ring[0].claim(Role::Stream, &net(true, &online)).unwrap();
        // Long enough that its steps no longer start from nothing.
        let signed = pass_around(&mut ring, 0, Role::Stream, 10).pop().unwrap();
        let unsigned = Record {
            holder: Some(key(2)),
            name: "P".into(),
            epoch: 2,
            chain: vec![],
        };
        for record in [&signed, &unsigned] {
            let mut fresh = device(6, "New laptop");
            fresh.seed_any = false;
            // Nobody it paired with a while ago, nor a stranger, seeds it.
            assert!(
                !fresh
                    .merge(Role::Stream, record, key(7), &net(true, &online))
                    .changed
            );
            assert_eq!(fresh.record(Role::Stream), &Record::default());
            // The device it just paired with does.
            fresh.seed_from(key(1));
            assert!(
                !fresh
                    .merge(Role::Stream, record, key(7), &net(true, &online))
                    .changed
            );
            assert!(
                fresh
                    .merge(Role::Stream, record, key(1), &net(true, &online))
                    .changed
            );
        }
    }

    #[test]
    fn a_lagging_0_7_0_device_follows_the_leaders_first_signed_step() {
        let migrated = |n: u8, holder: u8, epoch: u64| {
            let mut r = Roles::new(
                key(n),
                format!("Device {n}"),
                [
                    Record {
                        holder: Some(key(holder)),
                        name: "x".into(),
                        epoch,
                        chain: vec![],
                    },
                    Record::default(),
                    Record::default(),
                    Record::default(),
                ],
                DEFAULT_TAKEOVER,
                Default::default(),
                Signer::new(move |msg| kp(n).sign(msg)),
            );
            r.set_screens(true);
            r
        };
        // The office PC took the lease at epoch 3; the sanctuary never heard.
        let mut leader = migrated(2, 2, 3);
        let mut lagging = migrated(1, 1, 2);
        let n = net(true, &online);
        leader.receive_ask(Role::Projection, key(3), "x".into());
        let step = leader
            .allow(Role::Projection, key(3), false, &n)
            .unwrap()
            .record;
        assert_eq!(step.chain[0].prev_epoch, 3);
        assert!(m(&mut lagging, Role::Projection, &step).changed);
        assert_eq!(lagging.record(Role::Projection).holder, Some(key(3)));
    }

    #[test]
    fn a_hand_over_by_the_holder_beats_a_claim_at_the_same_epoch_and_everyone_agrees() {
        // S holds Stream. O, seeing S away, claims; at the same moment S
        // hands it to X. Whatever order devices hear it in, X's wins.
        let mut ring = [device(1, "S"), device(2, "O"), device(3, "X")];
        let first = ring[0].claim(Role::Stream, &net(true, &online)).unwrap();
        for d in &mut ring[1..] {
            m(d, Role::Stream, &first);
        }
        let s_gone = |d: DeviceKey| {
            if d == key(1) {
                Holder::Offline(secs(999))
            } else {
                Holder::Online
            }
        };
        let claim = ring[1].claim(Role::Stream, &net(true, &s_gone)).unwrap();
        ring[0].receive_ask(Role::Stream, key(3), "X".into());
        let hand = ring[0]
            .allow(Role::Stream, key(3), false, &net(true, &online))
            .unwrap()
            .record;
        assert_eq!(claim.epoch, hand.epoch);
        assert!(hand.beats(&claim) && !claim.beats(&hand));
        let mut seen = Vec::new();
        for order in [[&claim, &hand], [&hand, &claim]] {
            let mut t = device(8, "T");
            m(&mut t, Role::Stream, &first);
            for r in order {
                // From the device that made it, and from a relay.
                t.merge(Role::Stream, r, r.holder.unwrap(), &net(true, &s_gone));
            }
            seen.push(t.record(Role::Stream).clone());
        }
        assert_eq!(seen[0], seen[1]);
        assert_eq!(seen[0].holder, Some(key(3)));
        // O, who claimed, takes the hand-over (the same step from the same
        // state, signed by the one who held it), and X ignores the claim and
        // has the better record to answer with.
        assert!(m(&mut ring[1], Role::Stream, &hand).changed);
        assert_eq!(ring[1].record(Role::Stream).holder, Some(key(3)));
        assert!(m(&mut ring[2], Role::Stream, &hand).changed);
        assert!(
            !ring[2]
                .merge(Role::Stream, &claim, key(2), &net(true, &s_gone))
                .changed
        );
        assert_eq!(
            ring[2].better_than(Role::Stream, &claim),
            Some(hand.clone())
        );
        assert_eq!(ring[2].better_than(Role::Stream, &hand), None);
    }

    #[test]
    fn two_hand_overs_from_the_same_state_end_on_one_record() {
        let mut s = device(1, "S");
        let first = s.claim(Role::Stream, &net(true, &online)).unwrap();
        let mut s2 = s.clone();
        s.receive_ask(Role::Stream, key(2), "A".into());
        let to_a = s
            .allow(Role::Stream, key(2), false, &net(true, &online))
            .unwrap()
            .record;
        s2.receive_ask(Role::Stream, key(3), "B".into());
        let to_b = s2
            .allow(Role::Stream, key(3), false, &net(true, &online))
            .unwrap()
            .record;
        let winner = if key(3) > key(2) { &to_b } else { &to_a };
        for order in [[&to_a, &to_b], [&to_b, &to_a]] {
            let mut t = device(8, "T");
            m(&mut t, Role::Stream, &first);
            for r in order {
                m(&mut t, Role::Stream, r);
            }
            assert_eq!(t.record(Role::Stream), winner);
        }
    }

    #[test]
    fn a_holder_that_restarted_yields_to_a_claim_made_while_it_was_down() {
        // The sanctuary restarts and so remembers nothing of this run; what
        // it kept says the office PC had been away for longer than the
        // takeover time when it reconnected: the net says so, and it yields.
        let (mut a, mut b) = pair();
        let off = |_: DeviceKey| Holder::Offline(secs(999));
        let claim = b.claim(Role::Audio, &net(true, &off)).unwrap();
        let reconnect = net_away(&online, Some(secs(300)));
        let merged = a.merge(Role::Audio, &claim, key(2), &reconnect);
        assert!(merged.changed && merged.lost_role);
        assert!(a.notice(Role::Audio).is_some());
    }

    #[test]
    fn a_claimant_connected_all_along_never_takes_a_role_from_a_connected_holder() {
        // A paired device that has been connected for hours (its connection
        // is not a fresh one, so there is no "away") signs a claim.
        let (mut a, mut b) = pair();
        let off = |_: DeviceKey| Holder::Offline(secs(999));
        let claim = b.claim(Role::Audio, &net(true, &off)).unwrap();
        for role_net in [net_away(&online, None), net_away(&online, Some(secs(3)))] {
            assert!(!a.merge(Role::Audio, &claim, key(2), &role_net).changed);
            assert!(a.holds(Role::Audio));
            assert!(a.notice(Role::Audio).is_none());
        }
    }

    #[test]
    fn a_late_joiner_takes_the_state_after_a_takeover() {
        let mut ring = vec![device(1, "S"), device(2, "O")];
        ring[0].claim(Role::Stream, &net(true, &online)).unwrap();
        // A long history: the present record no longer starts from nothing.
        pass_around(&mut ring, 0, Role::Stream, 10);
        let s_gone = |d: DeviceKey| {
            if d == key(1) {
                Holder::Offline(secs(999))
            } else {
                Holder::Online
            }
        };
        let claim = ring[1].claim(Role::Stream, &net(true, &s_gone)).unwrap();
        // A new device pairs with O after the takeover. It is pristine and
        // seeded by O; S is a device it has never met.
        let mut late = device(7, "Late");
        late.seed_any = false;
        let unknown_s = |d: DeviceKey| {
            if d == key(1) {
                Holder::Gone
            } else {
                Holder::Online
            }
        };
        assert!(
            !late
                .merge(Role::Stream, &claim, key(5), &net(true, &unknown_s))
                .changed
        );
        late.seed_from(key(2));
        assert!(
            late.merge(Role::Stream, &claim, key(2), &net(true, &unknown_s))
                .changed
        );
        assert_eq!(late.record(Role::Stream).holder, Some(key(2)));
    }

    #[test]
    fn a_role_handed_to_a_device_that_cannot_hold_it_is_not_taken() {
        let (mut a, mut b) = pair();
        b.set_screens(false);
        let n = net(true, &online);
        a.receive_ask(Role::Projection, key(2), "Office PC".into());
        let hand = a.allow(Role::Projection, key(2), false, &n).unwrap().record;
        assert!(
            !m(&mut b, Role::Projection, &hand).changed,
            "no screens here"
        );
        assert!(!b.holds(Role::Projection));
    }
}
