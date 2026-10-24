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
}

impl Record {
    /// Whether this record replaces `other` when they meet: the higher
    /// epoch, and for claims made at the same moment the higher device key.
    pub fn beats(&self, other: &Record) -> bool {
        (self.epoch, self.holder) > (other.epoch, other.holder)
    }
}

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
        Ok(hex.and_then(|h| DeviceKey::from_hex(&h)))
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
}

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
}

impl std::fmt::Display for NotAllowed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotAllowed::NotHolding => write!(f, "this device is not presenting"),
            NotAllowed::NotAsking => write!(f, "that device is no longer asking"),
            NotAllowed::NeedsProjection => write!(f, "nobody is presenting"),
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
}

impl Roles {
    pub fn new(
        me: DeviceKey,
        name: String,
        records: [Record; 4],
        takeover_after: Duration,
        always: [BTreeMap<DeviceKey, String>; 4],
    ) -> Self {
        Self {
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
    fn check_teams(&self, _role: Role, _device: DeviceKey) -> Result<(), Denied> {
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
        self.check_teams(role, self.me)?;
        Ok(self.take(role))
    }

    fn take(&mut self, role: Role) -> Record {
        let i = role.index();
        self.records[i] = Record {
            holder: Some(self.me),
            name: self.name.clone(),
            epoch: self.records[i].epoch + 1,
        };
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
            if open && with_screens && self.blocked(role, net).is_none() {
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
            .map_err(|_| NotAllowed::NotAsking)?;
        if always && role.offers_always() {
            self.always[i].insert(asker.device, asker.name.clone());
        }
        self.records[i] = Record {
            holder: Some(asker.device),
            name: asker.name,
            epoch: self.records[i].epoch + 1,
        };
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

    // ---- Meeting another device's record -----------------------------------

    /// Takes in a record from another device: the newer one wins. `known`:
    /// the new holder is a device paired with this one (a stranger's record
    /// is not worth a notice).
    pub fn merge(&mut self, role: Role, remote: &Record, known: bool) -> Merged {
        let i = role.index();
        if !remote.beats(&self.records[i]) {
            return Merged::default();
        }
        let was_holder = self.holds(role);
        self.records[i] = remote.clone();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u8) -> DeviceKey {
        DeviceKey([n; 32])
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    struct Peers<'a> {
        has_peers: bool,
        holder: &'a dyn Fn(DeviceKey) -> Holder,
    }

    impl Net for Peers<'_> {
        fn has_peers(&self) -> bool {
            self.has_peers
        }
        fn holder(&self, device: DeviceKey) -> Holder {
            (self.holder)(device)
        }
    }

    fn net(has_peers: bool, f: &dyn Fn(DeviceKey) -> Holder) -> Peers<'_> {
        Peers {
            has_peers,
            holder: f,
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
        );
        r.set_screens(true);
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
            assert!(b.merge(role, &record, true).changed);
        }
        (a, b)
    }

    #[test]
    fn a_lone_device_holds_all_four_whatever_an_old_record_says() {
        let mut r = device(1, "Sanctuary");
        let alone = |_| Holder::Gone;
        let n = net(false, &alone);
        for role in Role::ALL {
            r.merge(
                role,
                &Record {
                    holder: Some(key(9)),
                    name: "Gone".into(),
                    epoch: 4,
                },
                false,
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

    #[test]
    fn records_merge_as_a_maximum() {
        let low = Record {
            holder: Some(key(1)),
            name: "A".into(),
            epoch: 3,
        };
        let high = Record {
            holder: Some(key(9)),
            name: "B".into(),
            epoch: 2,
        };
        assert!(low.beats(&high), "a higher epoch beats a higher key");
        assert!(!high.beats(&low));
        assert!(!low.beats(&low));
        let same_epoch = Record {
            holder: Some(key(9)),
            name: "B".into(),
            epoch: 3,
        };
        assert!(same_epoch.beats(&low), "the higher key breaks a tie");
        // Merging in either order, for every role, gives the same record.
        for role in Role::ALL {
            let mut x = device(5, "X");
            let mut y = device(6, "Y");
            x.merge(role, &low, true);
            x.merge(role, &high, true);
            y.merge(role, &high, true);
            y.merge(role, &low, true);
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
            let merged = b.merge(role, &allowed.record, true);
            assert!(merged.changed && !merged.lost_role);
            assert!(b.holds(role));
            assert_eq!(b.asking(role), None, "the ask is settled");
            // The old holder chose this: no notice, and nothing changes
            // when it hears its own record back.
            assert!(!a.merge(role, &allowed.record, true).changed);
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
            c.merge(role, a.record(role), true);
        }
        let mut b2 = b.clone();
        let lb = b2.claim(Role::Projection, &n_gone).unwrap();
        let lc = c.claim(Role::Projection, &n_gone).unwrap();
        assert_eq!(lb.epoch, lc.epoch, "same epoch");
        let mut x = b2.clone();
        let mut y = c.clone();
        let mx = x.merge(Role::Projection, &lc, true);
        let my = y.merge(Role::Projection, &lb, true);
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
        let merged = a.merge(Role::Projection, &lease, true);
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
            !a.merge(
                Role::Projection,
                &Record {
                    holder: Some(key(1)),
                    name: "Sanctuary".into(),
                    epoch: 1
                },
                true
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
        b.merge(
            Role::Projection,
            &Record {
                holder: Some(key(1)),
                name: "Sanctuary".into(),
                epoch: 1,
            },
            true,
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
        b.merge(Role::Operator, &allowed.record, true);
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
        b.merge(Role::Operator, &op.record, true);
        let mut c = device(3, "Back room");
        for role in Role::ALL {
            c.merge(role, a.record(role), true);
        }
        c.merge(Role::Operator, &op.record, true);
        c.ask(Role::Projection, &n).unwrap();
        a.receive_ask(Role::Projection, key(3), "Back room".into());
        let proj = a.allow(Role::Projection, key(3), false, &n).unwrap();
        b.merge(Role::Projection, &proj.record, true);
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
        b.merge(Role::Operator, &op.record, true);
        assert_eq!(a.standing(Role::Operator, &n), Standing::Open);
        let back = a.claim(Role::Operator, &n).unwrap();
        assert_eq!(back.epoch, 3);
        let merged = b.merge(Role::Operator, &back, true);
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
            c.merge(role, a.record(role), true);
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
        b.merge(Role::Operator, &op.record, true);
        assert!(b.yield_role(Role::Operator));
        assert!(!a.yield_role(Role::Stream), "not held, nothing to give");
        let back = a.claim(Role::Operator, &n).unwrap();
        assert!(b.merge(Role::Operator, &back, true).lost_role);
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
}
