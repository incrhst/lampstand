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
//!   default, and then ten seconds more: [`TAKEOVER_MARGIN`]), or when nobody holds the role.
//! - A holder that comes back finds a newer record and drops the role, with
//!   a [`Notice`]. It never resumes by itself.
//! - A claim is judged by the device it was taken from, which alone knows
//!   whether it was away. It accepts with a signed ack on the claim; a
//!   device that has it connected leaves the claim to it.
//! - A holder that was there all along refuses a claim against it with a
//!   signed [`Kind::Reclaim`]: a newer record of its own that every device
//!   takes in, so the claimant hands the role back and waits
//!   [`REFUSAL_COOLDOWN`] before it may take it over again.
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

/// How much longer than the takeover time a claimant waits before it claims.
///
/// Every device counts a silence from the last frame it heard, and a session
/// sends a frame at least every [`HEARTBEAT`]. So when a holder drops off, the
/// last frame the claimant heard from it and the last frame it heard from the
/// claimant can be up to one heartbeat apart (a little more on a busy machine
/// whose timer runs late). The device that judges a claim counts the other's
/// absence for itself, strictly: it believes the claim only if that is at
/// least the takeover time. A claimant that claimed at exactly the takeover
/// time by its own count could therefore be refused by a holder that came
/// back at once, and both would believe they hold the role. Waiting this much
/// longer (two heartbeats) means the judge's count is at least the takeover
/// time whatever the phase of the heartbeats, so the judge's rule needs no
/// leniency and a claimant cannot lie its way past it: the judge's own
/// measurement still decides.
pub const TAKEOVER_MARGIN: Duration = Duration::from_secs(10);

/// How often a session sends something (see `net::session`).
pub const HEARTBEAT: Duration = Duration::from_secs(5);

/// How long a device whose takeover the holder refused (a
/// [`Kind::Reclaim`]) waits before it may take the role over from that
/// holder again, counted from when the refusal arrived. A claimant that was
/// wrong about the holder's absence (a link that failed one way only) would
/// otherwise claim again as soon as its own count allowed, and the role would
/// go back and forth every couple of minutes. Asking stays open: the holder
/// decides that.
pub const REFUSAL_COOLDOWN: Duration = Duration::from_secs(5 * 60);

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
    /// The holder refused a claim made against it and takes the role back:
    /// a step on top of the claim, or of whatever was built on it, to the
    /// holder the claim named as `prev_holder`, signed by that holder. Valid
    /// only above a claim against the device that signs it that it never
    /// acked, with nothing it signed in between (`covered`); it cancels the
    /// claim and everything up to it.
    Reclaim,
}

/// One signed step in a role's history: from `(prev_holder, prev_epoch)` to
/// `(holder, epoch)`. The signature covers the role, the kind, both states
/// and a fixed tag, so a step cannot be replayed onto another role or epoch.
/// The name is not signed; names come from pairing.
///
/// A claim is judged by the device it was taken from, `prev_holder` (see
/// `Roles::admit`). Its verdict travels with the record, so every device
/// reads it the same way whatever path the record took: an `ack` on the
/// claim when it accepts it, a [`Kind::Reclaim`] above it when it refuses.
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
    /// On a claim: the signature of the device it was taken from over the
    /// same step, saying it accepts it. Not part of the step's own
    /// signature, so the judge adds it later. Always written (the wire
    /// format has no optional fields); missing in an older `roles.json`.
    #[serde(default, with = "opt_hex_bytes")]
    pub ack: Option<Vec<u8>>,
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

    /// What the judge of a claim signs to accept it: the step, under
    /// another tag, so an ack can never pass for a step.
    fn ack_bytes(&self, role: Role) -> Vec<u8> {
        let mut out = b"lampstand-role-ack-v1".to_vec();
        out.extend_from_slice(
            &Self::bytes(
                role,
                self.kind,
                self.holder,
                self.epoch,
                self.prev_holder,
                self.prev_epoch,
            )["lampstand-role-v1".len()..],
        );
        out
    }

    /// Who must have signed it: the new holder for a claim or a reclaim,
    /// the one who let go for the others.
    pub fn signer(&self) -> Option<DeviceKey> {
        match self.kind {
            Kind::Claim | Kind::Reclaim => self.holder,
            Kind::HandOver | Kind::Release => self.prev_holder,
        }
    }

    /// Whether it is well formed and signed by the device it names.
    fn valid(&self, role: Role) -> bool {
        let shape = match self.kind {
            Kind::Claim | Kind::HandOver => self.holder.is_some(),
            Kind::Release => self.holder.is_none(),
            // On whatever was built on the refused claim, which may have
            // been let go since.
            Kind::Reclaim => self.holder.is_some() && self.holder != self.prev_holder,
        } && self.epoch == self.prev_epoch.wrapping_add(1)
            && self.epoch != 0
            && self.epoch <= EPOCH_LIMIT;
        let (Some(signer), Ok(sig)) = (self.signer(), <[u8; 64]>::try_from(self.sig.as_slice()))
        else {
            return false;
        };
        // An ack is only ever on a claim from somebody, signed by them.
        let ack = match &self.ack {
            None => true,
            Some(ack) => match (
                self.kind,
                self.prev_holder,
                <[u8; 64]>::try_from(ack.as_slice()),
            ) {
                (Kind::Claim, Some(judge), Ok(ack)) => judge.verify(&self.ack_bytes(role), &ack),
                _ => false,
            },
        };
        shape
            && ack
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

/// For the reclaim at `i` in `steps` (newest first), the claim it refuses:
/// going down from it, an unacked claim against its signer reached before
/// any step that signer signed (which would mean it had built on the claim).
fn covered(steps: &[Link], i: usize) -> Option<usize> {
    let by = steps[i].holder?;
    for (j, l) in steps.iter().enumerate().skip(i + 1) {
        if l.signer() == Some(by) {
            return None;
        }
        if l.kind == Kind::Claim && l.prev_holder == Some(by) && l.ack.is_none() {
            return Some(j);
        }
    }
    None
}

/// Which states in `steps` (newest first; the state each step leads to) came
/// from a claim handed straight back: an unacked claim against a judge, and
/// later in the steps the role is the judge's again, with nothing it signed
/// in between. The claim stood only because the role came back to its
/// judge; the states from it up to the hand-back are marked. A claim taken
/// because its judge was gone is not one: forks above it stay usable, and
/// the judge acks it if it comes back.
fn doubtful(steps: &[Link]) -> Vec<bool> {
    let mut out = vec![false; steps.len()];
    for (j, l) in steps.iter().enumerate() {
        let (Kind::Claim, None, Some(judge)) = (l.kind, &l.ack, l.prev_holder) else {
            continue;
        };
        let back = (0..j).rev().find(|&h| steps[h].holder == Some(judge));
        if let Some(h) = back {
            if !steps[h..j].iter().any(|s| s.signer() == Some(judge)) {
                out[h + 1..=j].fill(true);
            }
        }
    }
    out
}

/// Whether two steps are the same step, whatever acks they carry.
fn same_step(a: &Link, b: &Link) -> bool {
    (
        a.kind,
        a.holder,
        a.epoch,
        a.prev_holder,
        a.prev_epoch,
        &a.sig,
    ) == (
        b.kind,
        b.holder,
        b.epoch,
        b.prev_holder,
        b.prev_epoch,
        &b.sig,
    )
}

/// Which steps a reclaim above them cancels: the refused claim and
/// everything built on it up to the reclaim. They gave nobody anything.
fn cancelled(steps: &[Link]) -> Vec<bool> {
    let mut out = vec![false; steps.len()];
    for i in 0..steps.len() {
        if steps[i].kind == Kind::Reclaim {
            if let Some(j) = covered(steps, i) {
                out[i + 1..=j].fill(true);
            }
        }
    }
    out
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

mod opt_hex_bytes {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        match bytes {
            Some(b) => s.serialize_some(&b.iter().map(|b| format!("{b:02x}")).collect::<String>()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        Option::<String>::deserialize(d)?
            .map(|hex| {
                if hex.len() % 2 != 0 || !hex.is_ascii() {
                    return Err(serde::de::Error::custom("not hex"));
                }
                (0..hex.len() / 2)
                    .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16))
                    .collect::<Result<_, _>>()
                    .map_err(serde::de::Error::custom)
            })
            .transpose()
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
    /// For two records at the same epoch a holder's refusal of a claim (a
    /// reclaim) comes first, then the one the holder signed away (a hand-over
    /// or a release), then one somebody took (a claim), then one with no
    /// steps; after that the higher key. A reclaim comes first because it
    /// says the state it sits on should not have been: a hand-over the
    /// claimant made from it at the same moment loses. Every device applies
    /// the same order, so they all end on one record. Last, the one whose
    /// claims carry more acks, so a judge's ack spreads like a newer record.
    pub fn beats(&self, other: &Record) -> bool {
        (self.epoch, self.rank(), self.holder, self.acks())
            > (other.epoch, other.rank(), other.holder, other.acks())
    }

    fn acks(&self) -> usize {
        self.chain.iter().filter(|l| l.ack.is_some()).count()
    }

    fn rank(&self) -> u8 {
        match self.chain.first().map(|l| l.kind) {
            Some(Kind::Reclaim) => 3,
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
    /// Whether the device is paired with this one, connected or not.
    fn paired(&self, device: DeviceKey) -> bool;
    /// The takeover setting the device last told this one about, if it has.
    /// A claimant waits at least this long for a holder (and never less than
    /// its own setting): see [`TAKEOVER_MARGIN`].
    fn takeover_of(&self, _device: DeviceKey) -> Option<Duration> {
        None
    }
    /// Now, in Unix milliseconds by the clock that counts a sleep (the one
    /// the refusal cooldown is measured by).
    fn now_ms(&self) -> i64 {
        0
    }
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
    /// The holder refused this device's takeover a short while ago
    /// ([`REFUSAL_COOLDOWN`]); `remaining` until it may take over again.
    Refused { remaining: Duration },
    /// This device has just connected and has not yet been told who holds
    /// the roles, so a role that looks unheld may not be.
    Settling,
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
            Denied::MustAsk => write!(f, "the holder is connected: ask it to allow this"),
            Denied::TooSoon { remaining, .. } => write!(
                f,
                "the holder went offline only recently; you can take over in {}s",
                remaining.as_secs().max(1)
            ),
            Denied::Already => write!(f, "this device already holds that role"),
            Denied::NobodyToAsk => write!(f, "the holder is not connected, so it cannot be asked"),
            Denied::NotNeeded => write!(f, "nobody needs to allow this: take it from here"),
            Denied::NoScreens => write!(f, "No screens are set up on this device"),
            Denied::NeedsProjection => write!(f, "nobody runs the screens"),
            Denied::NotPermitted => write!(f, "this device may not hold that role"),
            Denied::Refused { remaining } => write!(
                f,
                "the holder refused this device's takeover; you can take over in {}s",
                remaining.as_secs().max(1)
            ),
            // What the bar said in 0.7.0 before it had heard who presents.
            Denied::Settling => write!(f, "nobody is presenting"),
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
            NotAllowed::NotHolding => write!(f, "this device does not hold that role"),
            NotAllowed::NotAsking => write!(f, "that device is no longer asking"),
            NotAllowed::NeedsProjection => write!(f, "nobody runs the screens"),
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
    /// This device took the role over, and the holder, which had not been
    /// away as long as this device thought, refused it and kept the role.
    Refused { by: String },
}

/// A takeover the holder refused: no new one against it until
/// [`REFUSAL_COOLDOWN`] has passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Refusal {
    by: DeviceKey,
    /// When it arrived ([`Net::now_ms`]).
    at_ms: i64,
}

/// What a [`Roles::merge`] changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Merged {
    pub changed: bool,
    pub lost_role: bool,
    /// The record held a claim against this device that it refused: its own
    /// record is now a reclaim on top of it, to send to every device.
    pub reclaimed: bool,
    /// Not taken in: it holds a claim for this connected device to judge.
    /// Send it this record (the one that came, with any acks of this
    /// device's own), and its answer comes back like any record.
    pub refer: Option<(DeviceKey, Record)>,
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
    /// A takeover by this device the holder refused, per role. Not kept on
    /// disk: a restart ends the cooldown.
    refused: [Option<Refusal>; 4],
    signer: Signer,
    /// The device this one has just paired with: the only one whose present
    /// state a device with nothing of its own takes without joining it.
    seed: Option<DeviceKey>,
    /// Which roles the seed has spoken about; once all four, it is forgotten.
    seed_heard: [bool; 4],
    /// Roles whose state was lost (an unreadable `roles.json`): any paired
    /// device may seed them again.
    reseed: [bool; 4],
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
            seed_heard: [false; 4],
            reseed: [false; 4],
            #[cfg(test)]
            seed_any: false,
            unverified: [false; 4],
            refused: [None; 4],
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
            if self.refused[role.index()].is_some_and(|r| r.by == device) {
                self.refused[role.index()] = None;
            }
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
            Holder::Offline(offline) => {
                let wait = self.claim_wait(role, net);
                let cooling = self.cooldown(role, net);
                if offline >= wait && cooling.is_none() {
                    Standing::TakeOver { offline }
                } else {
                    Standing::Wait {
                        offline,
                        remaining: wait
                            .saturating_sub(offline)
                            .max(cooling.unwrap_or_default()),
                    }
                }
            }
        }
    }

    /// How long until this device may take `role` over again from a holder
    /// that refused its takeover; `None` when it may (no refusal, the role
    /// has moved to another holder since, or [`REFUSAL_COOLDOWN`] is over).
    pub fn cooldown(&self, role: Role, net: &dyn Net) -> Option<Duration> {
        let refusal = self.refused[role.index()]?;
        if self.record(role).holder != Some(refusal.by) {
            return None;
        }
        // A clock that went back past the refusal cannot say how long ago it
        // was: the cooldown is over rather than stretched by the jump.
        let since =
            Duration::from_millis(u64::try_from(net.now_ms().saturating_sub(refusal.at_ms)).ok()?);
        REFUSAL_COOLDOWN
            .checked_sub(since)
            .filter(|left| !left.is_zero())
    }

    /// How long the holder of `role` must have been silent before this
    /// device claims: the longer of its own takeover time and the one the
    /// holder last advertised (the holder judges a claim by its own setting),
    /// then the margin that lets the holder's count of this device's absence
    /// reach it too. What the holder advertised is honoured up to
    /// [`MAX_TAKEOVER`], so a device that lies about it can delay a takeover
    /// of its own role by at most that, and never hurry one.
    fn claim_wait(&self, role: Role, net: &dyn Net) -> Duration {
        let theirs = self
            .record(role)
            .holder
            .and_then(|h| net.takeover_of(h))
            .map_or(Duration::ZERO, |d| d.min(MAX_TAKEOVER));
        self.takeover_after.max(theirs) + TAKEOVER_MARGIN
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
                return Err(if self.cooldown(role, net).is_some() {
                    Denied::Refused { remaining }
                } else {
                    Denied::TooSoon { offline, remaining }
                });
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
            ack: None,
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
    /// Every claim among the steps it has not seen is judged, wherever it
    /// sits and whoever sent it ([`Roles::verdict`]).
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
            if pristine {
                // From anyone else it waits to be taught by the device it
                // pairs with.
                return if self.seeded_by(role, from) {
                    Admit::Yes { acks: Vec::new() }
                } else {
                    Admit::Unverified
                };
            }
            // The device that holds the lease says so itself.
            return if migrated && near && remote.holder == Some(from) {
                Admit::Yes { acks: Vec::new() }
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
        let known = |d: DeviceKey| d == self.me || net.paired(d) || in_state(d);
        // Where this device's own state sits in the steps.
        let here = |holder: Option<DeviceKey>, epoch: u64| {
            steps
                .iter()
                .position(|l| l.holder == holder && l.epoch == epoch)
        };
        let oldest = steps.last().expect("not empty");
        let migrated = local.chain.is_empty() && !pristine;
        // How many steps are newer than the state `(holder, epoch)`, if they
        // pass through it or start from it, and the history they sit on:
        // the steps themselves, and when they start from that state, what
        // this device holds below it (`below`), where a reclaim may find the
        // claim it refuses.
        let join_at = |holder: Option<DeviceKey>, epoch: u64, below: &[Link]| {
            if let Some(k) = here(holder, epoch) {
                Some((k, steps.clone()))
            } else if oldest.prev_holder == holder && oldest.prev_epoch == epoch {
                Some((steps.len(), steps.iter().chain(below).cloned().collect()))
            } else {
                None
            }
        };
        // The record forks from this device's own history: both came from a
        // state this device held before (claims made at the same moment, a
        // claim and a hand-over, a refused claim and the reclaim of it or of
        // another claim made with it, and whatever was done on top of
        // either). The record order has already said the other comes
        // first; its steps are checked from where the two parted, as if
        // this device were still there. A state this device's own reclaim
        // cancelled was never really held: a record built on it parts from
        // the state before the refused claim, so the claim is judged again.
        // Nor does one reached through a claim its judge never accepted (a
        // claim this device took because the judge was gone, or because the
        // role came back to the judge at once): a record parting from above
        // that claim is judged from below it, so the claim is judged again.
        let refused = cancelled(&local.chain);
        let doubtful = doubtful(&local.chain);
        let fork = || {
            local.chain.iter().enumerate().find_map(|(d, l)| {
                if refused.get(d + 1).copied().unwrap_or(false)
                    || doubtful.get(d + 1).copied().unwrap_or(false)
                {
                    return None;
                }
                join_at(l.prev_holder, l.prev_epoch, &local.chain[d + 1..])
            })
        };
        let joined = join_at(local.holder, local.epoch, &local.chain)
            .or_else(fork)
            .or_else(|| {
                // The first signed step on a state moved over from 0.7.0,
                // made on a device that was ahead of this one.
                (migrated
                    && oldest.prev_epoch >= local.epoch
                    && oldest.prev_epoch - local.epoch <= CHAIN as u64)
                    .then(|| (steps.len(), steps.clone()))
            });
        if joined.is_none() && pristine {
            // Nothing of its own to join: the present state, from the device
            // just paired.
            if self.seeded_by(role, from) {
                return Admit::Yes { acks: Vec::new() };
            }
            // Not from the device that seeds it: not taken, but waiting for
            // one that does, unless it is signed by keys nobody knows.
            return if steps.iter().all(|l| l.signer().is_some_and(known)) {
                Admit::Unverified
            } else {
                Admit::No
            };
        }
        if let Some((newer, context)) = joined {
            let off = cancelled(&context);
            let mut introduced: Vec<DeviceKey> = Vec::new();
            let mut acks = Vec::new();
            let mut refer = None;
            let mut refuse = false;
            // Oldest first.
            for i in (0..newer).rev() {
                let l = &context[i];
                let signer = l.signer().expect("valid steps have a signer");
                // A reclaim refuses a claim against its signer, in what it
                // sits on.
                if l.kind == Kind::Reclaim && covered(&context, i).is_none() {
                    return Admit::No;
                }
                // A step a reclaim cancels gave nobody anything: its signer
                // need not be known here, and there is nothing to judge.
                if off[i] {
                    continue;
                }
                // Whoever signs must be known, or have been handed the role
                // by a step already checked.
                if !known(signer) && !introduced.contains(&signer) {
                    return Admit::No;
                }
                if let Some(h) = l.holder {
                    introduced.push(h);
                }
                match self.verdict(role, remote.holder, &context, i, from, net) {
                    Verdict::Stands => {}
                    Verdict::Accept => acks.push(i),
                    Verdict::Refuse => refuse = true,
                    Verdict::AskJudge(judge) => {
                        refer.get_or_insert(judge);
                    }
                    Verdict::NotYet => return Admit::No,
                }
            }
            return if refuse {
                Admit::Refused
            } else if let Some(to) = refer {
                Admit::Refer { to, acks }
            } else {
                Admit::Yes { acks }
            };
        }
        // A claim from the device that made it, whatever it missed (within
        // what could have been missed): judged as a fresh claim here.
        if let (Kind::Claim, Some(claimant)) = (head.kind, head.holder) {
            if from == claimant
                && head.epoch <= local.epoch.saturating_add(CHAIN as u64)
                && self.claim_ok(role, claimant, local.holder, net)
            {
                return Admit::Yes { acks: Vec::new() };
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

    /// What this device makes of step `i` of `steps` (newest first), a step
    /// it has not seen before. Only a claim needs judging, and its judge is
    /// the device it was taken from, which alone knows whether it was really
    /// away. Its verdict is in the steps when it has given one (an ack, or a
    /// later step it signed); this device gives it when it is the judge; and
    /// a device that has the judge connected leaves it to the judge. Every
    /// device reads a verdict the same way, so where the record came from
    /// does not matter.
    fn verdict(
        &self,
        role: Role,
        holder: Option<DeviceKey>,
        steps: &[Link],
        i: usize,
        from: DeviceKey,
        net: &dyn Net,
    ) -> Verdict {
        let l = &steps[i];
        let (Kind::Claim, None, Some(judge), Some(claimant)) =
            (l.kind, &l.ack, l.prev_holder, l.holder)
        else {
            // Not a claim, an accepted one, or one from nobody.
            return Verdict::Stands;
        };
        // The judge built on it, so it knew of the claim and let it stand.
        if steps[..i].iter().any(|s| s.signer() == Some(judge)) {
            return Verdict::Stands;
        }
        // The Projection holder takes the Operator seat back when it likes.
        if role == Role::Operator && self.record(Role::Projection).holder == Some(claimant) {
            return Verdict::Stands;
        }
        if judge == self.me {
            // Accepted before (a copy made before the ack came this way).
            let acked = self
                .record(role)
                .chain
                .iter()
                .any(|mine| mine.ack.is_some() && same_step(mine, l));
            return if acked || self.claim_ok(role, claimant, Some(judge), net) {
                Verdict::Accept
            } else if holder == Some(judge) {
                // The role ends with this device in this record: nothing was
                // taken from it. The claim stands here only, unacked, so a
                // record that parts from above it is judged again (see
                // `doubtful`).
                Verdict::Stands
            } else if from == claimant && net.holder(claimant) != Holder::Online {
                // The claimant's own frame, still arriving on a connection
                // that is being replaced, is judged with nothing known of
                // the outage: not taken, to be judged again when the next
                // connection opens and brings it.
                Verdict::NotYet
            } else {
                Verdict::Refuse
            };
        }
        // The role ends with the judge in this record: nothing was taken
        // from it (see above).
        if holder == Some(judge) {
            return Verdict::Stands;
        }
        match net.holder(judge) {
            Holder::Gone | Holder::Me => Verdict::Stands,
            // The judge sends it as its own record: it took it in.
            Holder::Online if from == judge => Verdict::Stands,
            Holder::Online => Verdict::AskJudge(judge),
            // Gone long enough that it may be gone for good. If it comes
            // back it judges the claim then.
            Holder::Offline(offline) if offline >= self.takeover_after => Verdict::Stands,
            Holder::Offline(_) => Verdict::NotYet,
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

    fn seeded_by(&self, role: Role, from: DeviceKey) -> bool {
        #[cfg(test)]
        if self.seed_any {
            return true;
        }
        self.seed == Some(from) || self.reseed[role.index()]
    }

    /// The state of every role was lost (an unreadable `roles.json`): all
    /// four wait, flagged, to be taught again by any paired device.
    pub fn mark_for_reseeding(&mut self) {
        self.reseed = [true; 4];
        self.unverified = [true; 4];
    }

    /// Some role is still waiting to be taught again after its state was
    /// lost.
    pub fn reseeding(&self) -> bool {
        self.reseed.iter().any(|r| *r)
    }

    /// Roles this device has asked `holder` for and still waits on: to be
    /// asked again when a connection to it opens, in case the ask was lost.
    pub fn waiting_asks(&self, holder: DeviceKey) -> Vec<Role> {
        Role::ALL
            .into_iter()
            .filter(|r| {
                self.record(*r).holder == Some(holder)
                    && self
                        .asking(*r)
                        .is_some_and(|a| a.state == AskState::Waiting)
            })
            .collect()
    }

    /// Whether a newer record for `role` was seen whose steps do not join
    /// this device's state: it waits for one that does.
    pub fn unverified(&self, role: Role) -> bool {
        self.unverified[role.index()]
    }

    /// Takes in a record from another device (`from`): the newer one wins,
    /// if its signed steps check out ([`Roles::admit`]).
    pub fn merge(&mut self, role: Role, remote: &Record, from: DeviceKey, net: &dyn Net) -> Merged {
        let merged = self.merge_record(role, remote, from, net);
        // The device it just paired with has now told it about this role;
        // once it has told it about all four, nobody else is "just paired".
        if self.seed == Some(from) {
            self.seed_heard[role.index()] = true;
            if self.seed_heard.iter().all(|h| *h) {
                self.seed = None;
                self.seed_heard = [false; 4];
            }
        }
        let i = role.index();
        // Taught again: by a record that took, or by a device that has the
        // same or less to say.
        if self.reseed[i] && (merged.changed || !remote.beats(&self.records[i])) {
            self.reseed[i] = false;
            self.unverified[i] = false;
        }
        merged
    }

    fn merge_record(
        &mut self,
        role: Role,
        remote: &Record,
        from: DeviceKey,
        net: &dyn Net,
    ) -> Merged {
        let i = role.index();
        // An epoch that no claim could ever beat would lock the role for
        // good: it is nonsense, not a record.
        if remote.epoch > EPOCH_LIMIT {
            return Merged::default();
        }
        if !remote.beats(&self.records[i]) {
            // Acks it has for steps this device has too: two judges' acks
            // may each reach a device by another way first.
            return Merged {
                changed: self.take_acks(role, remote),
                ..Merged::default()
            };
        }
        let remote = &match self.admit(role, remote, from, net) {
            Admit::Yes { acks } => self.acked(role, remote, &acks),
            Admit::No => return Merged::default(),
            Admit::Unverified => {
                self.unverified[i] = true;
                return Merged::default();
            }
            Admit::Refused => return self.reclaim(role, remote),
            Admit::Refer { to, acks } => {
                return Merged {
                    refer: Some((to, self.acked(role, remote, &acks))),
                    ..Merged::default()
                };
            }
        };
        // A stranger's record is not worth a notice.
        let known = remote
            .holder
            .is_some_and(|h| h == self.me || net.holder(h) != Holder::Gone);
        // The holder refused this device's own takeover: the steps hold a
        // reclaim that cancels a claim made from the state this device
        // claimed from (its own claim, or another made at the same moment).
        let refused_by = self.records[i]
            .chain
            .first()
            .filter(|l| l.kind == Kind::Claim && l.holder == Some(self.me))
            .and_then(|mine| {
                let steps = &remote.chain;
                (0..steps.len()).find_map(|r| {
                    let j = covered(steps, r).filter(|_| steps[r].kind == Kind::Reclaim)?;
                    steps[r + 1..=j]
                        .iter()
                        .any(|l| {
                            l.kind == Kind::Claim
                                && l.prev_holder == mine.prev_holder
                                && l.prev_epoch == mine.prev_epoch
                        })
                        .then_some(steps[r].holder)
                        .flatten()
                })
            });
        let was_holder = self.holds(role);
        self.records[i] = remote.clone();
        self.records[i].chain.truncate(CHAIN);
        self.unverified[i] = false;
        self.pending[i] = None;
        let lost_role = was_holder && !self.holds(role);
        if lost_role {
            let chosen = std::mem::take(&mut self.yielding[i]);
            if let Some(by) = refused_by {
                // Handed back, and no new takeover from it for a while.
                self.refused[i] = Some(Refusal {
                    by,
                    at_ms: net.now_ms(),
                });
                if known {
                    self.notice[i] = Some(Notice::Refused {
                        by: remote.name.clone(),
                    });
                }
            } else if known && !chosen {
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
            ..Merged::default()
        }
    }

    /// Copies onto this device's own steps the valid acks `remote` has for
    /// them. True if it gained any.
    fn take_acks(&mut self, role: Role, remote: &Record) -> bool {
        let mut gained = false;
        let chain = &mut self.records[role.index()].chain;
        let off = cancelled(chain);
        for (l, off) in chain.iter_mut().zip(off) {
            // An ack under a reclaim would undo the reclaim: a judge that
            // accepted a claim one way and refused it another, the refusal
            // having come first here.
            if l.ack.is_some() || off {
                continue;
            }
            if let Some(theirs) = remote
                .chain
                .iter()
                .find(|r| r.ack.is_some() && same_step(r, l) && r.valid(role))
            {
                l.ack.clone_from(&theirs.ack);
                gained = true;
            }
        }
        gained
    }

    /// `remote` with this device's ack on the claims at `acks`, and the
    /// acks this device's own record already has for the same steps (a copy
    /// that came another way may lack them).
    fn acked(&self, role: Role, remote: &Record, acks: &[usize]) -> Record {
        let mut record = remote.clone();
        let off = cancelled(&record.chain);
        for (l, _) in record
            .chain
            .iter_mut()
            .zip(off)
            .filter(|(l, off)| l.ack.is_none() && !off)
        {
            l.ack = self
                .record(role)
                .chain
                .iter()
                .find(|mine| mine.ack.is_some() && same_step(mine, l))
                .and_then(|mine| mine.ack.clone());
        }
        for &k in acks {
            let l = &mut record.chain[k];
            l.ack = Some((self.signer.0)(&l.ack_bytes(role)).to_vec());
        }
        record
    }

    /// Refuses `claim`, a record holding a claim against this device: signs
    /// a reclaim on top of it, which beats it on every device it reached, so
    /// they all come back to this one.
    fn reclaim(&mut self, role: Role, claim: &Record) -> Merged {
        if claim.epoch >= EPOCH_LIMIT {
            return Merged::default();
        }
        let i = role.index();
        self.records[i] = claim.clone();
        self.records[i].chain.truncate(CHAIN);
        self.step(role, Kind::Reclaim, Some(self.me), self.name.clone());
        Merged {
            changed: true,
            reclaimed: true,
            ..Merged::default()
        }
    }
}

enum Admit {
    /// Taken in, with an ack from this device on the claims at these
    /// places in the steps (claims against it that it accepts).
    Yes {
        acks: Vec<usize>,
    },
    No,
    Unverified,
    /// A claim against this device that it refuses with a reclaim.
    Refused,
    /// Not taken: a claim in it is for `to`, connected here, to judge. It
    /// is passed on to it, with this device's acks.
    Refer {
        to: DeviceKey,
        acks: Vec<usize>,
    },
}

/// What a device makes of one step it has not seen ([`Roles::verdict`]).
enum Verdict {
    /// Nothing to judge, or judged already.
    Stands,
    /// A claim against this device that it accepts: it acks it.
    Accept,
    /// A claim against this device that it refuses: it reclaims.
    Refuse,
    /// A claim for a connected device to judge.
    AskJudge(DeviceKey),
    /// Not now: judged again when it comes again.
    NotYet,
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
        r.merge(
            role,
            rec,
            rec.holder.unwrap_or(key(0)),
            &net_away(&online, Some(secs(999))),
        )
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
        /// The wall clock, in Unix ms.
        now: i64,
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
        fn paired(&self, d: DeviceKey) -> bool {
            !self.strangers.contains(&d)
        }
        fn now_ms(&self) -> i64 {
            self.now
        }
    }

    fn net(has_peers: bool, f: &dyn Fn(DeviceKey) -> Holder) -> Peers<'_> {
        Peers {
            has_peers,
            holder: f,
            // A connection that has been up for a while: nobody is "away".
            away: None,
            strangers: Vec::new(),
            now: 0,
        }
    }

    /// Like [`net`], with a say in how long a claimant had been out of reach.
    fn net_away(f: &dyn Fn(DeviceKey) -> Holder, away: Option<Duration>) -> Peers<'_> {
        Peers {
            has_peers: true,
            holder: f,
            away,
            strangers: Vec::new(),
            now: 0,
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
        let seen = net_away(&online, Some(secs(999)));
        hand_seeing(from, role, to, to_key, &seen)
    }

    /// [`hand`], with `to` judging the record by what `seen` says.
    fn hand_seeing(
        from: &mut Roles,
        role: Role,
        to: &mut Roles,
        to_key: u8,
        seen: &dyn Net,
    ) -> Record {
        from.receive_ask(role, key(to_key), format!("Device {to_key}"));
        let record = from
            .allow(role, key(to_key), false, &net(true, &online))
            .unwrap()
            .record;
        let by = from.me;
        assert!(to.merge(role, &record, by, seen).changed);
        record
    }

    /// The Sanctuary (`key(1)`) seems gone; everyone else is connected.
    fn sanctuary_gone(d: DeviceKey) -> Holder {
        if d == key(1) {
            Holder::Offline(secs(999))
        } else {
            Holder::Online
        }
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
        // Each sees the holder gone too, so neither claim waits for it.
        let mx = x.merge(Role::Projection, &lc, key(3), &n_gone);
        let my = y.merge(Role::Projection, &lb, key(2), &n_gone);
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
        // The takeover time (120 s) plus the margin (10 s) before a claim.
        let (f30, f119, f120, f300) = (at(30), at(129), at(130), at(300));
        for role in [Role::Projection, Role::Audio] {
            assert_eq!(
                b.standing(role, &net(true, &f30)),
                Standing::Wait {
                    offline: secs(30),
                    remaining: secs(100)
                }
            );
            assert_eq!(
                b.claim(role, &net(true, &f119)),
                Err(Denied::TooSoon {
                    offline: secs(129),
                    remaining: secs(1)
                })
            );
            assert_eq!(b.ask(role, &net(true, &f30)), Err(Denied::NobodyToAsk));
            assert!(!b.holds(role), "a refused claim changes nothing");
            assert_eq!(
                b.standing(role, &net(true, &f120)),
                Standing::TakeOver { offline: secs(130) }
            );
            assert_eq!(b.ask(role, &net(true, &f300)), Err(Denied::NotNeeded));
        }
        let record = b.claim(Role::Projection, &net(true, &f300)).unwrap();
        assert_eq!((record.epoch, record.holder), (2, Some(key(2))));
        assert_eq!(b.mode(&net(true, &online)), Mode::Presenter);
    }

    /// The boundary, with the default takeover time and the margin: a claim
    /// is not allowed until the takeover time plus the margin, and the
    /// device that judges it believes it from the takeover time on.
    #[test]
    fn a_claim_waits_for_the_takeover_time_plus_the_margin() {
        let (_, mut b) = pair();
        assert_eq!(b.takeover_after(), DEFAULT_TAKEOVER);
        let after = |s: u64| move |_: DeviceKey| Holder::Offline(secs(s));
        let t = DEFAULT_TAKEOVER.as_secs();
        let m = TAKEOVER_MARGIN.as_secs();
        for (offline, allowed) in [
            (t - 1, false),
            (t, false),
            (t + m - 1, false),
            (t + m, true),
            (t + m + 5, true),
        ] {
            let f = after(offline);
            let standing = b.standing(Role::Audio, &net(true, &f));
            assert_eq!(
                matches!(standing, Standing::TakeOver { .. }),
                allowed,
                "offline {offline}s: {standing:?}"
            );
            assert_eq!(
                b.claim(Role::Audio, &net(true, &f)).is_ok(),
                allowed,
                "offline {offline}s"
            );
            if allowed {
                return;
            }
        }
    }

    /// The argument for the margin depends on this relation between the
    /// constants: two devices' counts of the same silence differ by at most a
    /// heartbeat, and a busy machine's timer runs late by up to another.
    #[test]
    fn the_margin_is_at_least_two_heartbeats() {
        assert!(TAKEOVER_MARGIN >= HEARTBEAT * 2);
    }

    /// A holder with a longer takeover setting than the claimant's: the
    /// claimant waits by the holder's, plus the margin. A shorter one changes
    /// nothing, and an absurd one is capped at the longest setting.
    #[test]
    fn a_claimant_waits_by_the_holders_takeover_setting_when_it_is_longer() {
        struct Advertised<'a>(Peers<'a>, Option<Duration>);
        impl Net for Advertised<'_> {
            fn has_peers(&self) -> bool {
                self.0.has_peers()
            }
            fn holder(&self, d: DeviceKey) -> Holder {
                self.0.holder(d)
            }
            fn away(&self, d: DeviceKey) -> Option<Duration> {
                self.0.away(d)
            }
            fn paired(&self, d: DeviceKey) -> bool {
                self.0.paired(d)
            }
            fn takeover_of(&self, _: DeviceKey) -> Option<Duration> {
                self.1
            }
        }
        let (_, mut b) = pair();
        b.set_takeover_after(secs(60));
        let standing = |b: &Roles, offline: u64, theirs: Option<Duration>| {
            let f = move |_: DeviceKey| Holder::Offline(secs(offline));
            let n = Advertised(net(true, &f), theirs);
            b.standing(Role::Audio, &n)
        };
        let opens = |s: Standing| matches!(s, Standing::TakeOver { .. });
        // Holder 120 s, claimant 60 s: not before 130 s.
        assert!(!opens(standing(&b, 70, Some(secs(120)))));
        assert!(!opens(standing(&b, 129, Some(secs(120)))));
        assert!(opens(standing(&b, 130, Some(secs(120)))));
        // Nothing advertised, or a shorter holder: the claimant's own.
        assert!(opens(standing(&b, 70, None)));
        assert!(opens(standing(&b, 70, Some(secs(30)))));
        // A lie is honoured only up to the longest setting there is.
        let huge = Some(secs(u64::from(u32::MAX)));
        assert!(!opens(standing(&b, 24 * 3600, huge)));
        assert!(opens(standing(&b, 24 * 3600 + 10, huge)));
        assert_eq!(
            match standing(&b, 100, Some(secs(120))) {
                Standing::Wait { remaining, .. } => remaining,
                other => panic!("{other:?}"),
            },
            secs(30)
        );
    }

    /// The judge's rule is unchanged and strict: a claim against it is
    /// believed from a device that was away for the takeover time and not
    /// before, and never from one that has been connected all along.
    #[test]
    fn the_device_that_judges_a_claim_still_asks_for_the_whole_takeover_time() {
        let t = DEFAULT_TAKEOVER;
        for (away, believed) in [
            (None, false),
            (Some(secs(3)), false),
            (Some(t - secs(1)), false),
            (Some(t), true),
            (Some(t + secs(30)), true),
        ] {
            let (mut a, _, _) = church();
            let mut c = device(3, "Laptop");
            for role in Role::ALL {
                m(&mut c, role, a.record(role));
            }
            let off = |_: DeviceKey| Holder::Offline(secs(999));
            let claim = c.claim(Role::Stream, &net(true, &off)).unwrap();
            let merged = a.merge(Role::Stream, &claim, key(3), &net_away(&online, away));
            assert!(merged.changed, "away {away:?}");
            assert_eq!(
                a.record(Role::Stream).holder == Some(key(3)),
                believed,
                "away {away:?}"
            );
            // Refused out loud: the judge's own reclaim on top of the claim.
            assert_eq!(merged.reclaimed, !believed, "away {away:?}");
        }
    }

    #[test]
    fn the_timeout_is_configurable() {
        let (_, mut b) = pair();
        let off = |s: u64| move |_: DeviceKey| Holder::Offline(secs(s));
        let (f300, f61) = (off(300), off(71));
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
                ack: None,
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

    /// `a` (the Sanctuary) holds Stream; the Office PC thinks it has been
    /// gone long enough and claims, but `a` saw the Office PC connected all
    /// along and refuses. Returns the claim and `a`'s answer to it.
    fn refused_claim(a: &mut Roles, b: &mut Roles) -> (Record, Record) {
        let off = |_: DeviceKey| Holder::Offline(secs(999));
        let claim = b.claim(Role::Stream, &net(true, &off)).unwrap();
        assert!(b.holds(Role::Stream), "the claimant believes it");
        let merged = a.merge(
            Role::Stream,
            &claim,
            key(2),
            &net_away(&online, Some(secs(3))),
        );
        assert!(merged.changed && merged.reclaimed && !merged.lost_role);
        (claim, a.record(Role::Stream).clone())
    }

    /// The holder's refusal is a step it signs on top of the claim, and the
    /// claimant (and any device that took the claim in) takes it in like any
    /// newer record: the role goes back to the holder everywhere.
    #[test]
    fn a_refused_claim_is_answered_with_a_signed_reclaim_and_handed_back() {
        let (mut a, mut b, mut c) = church();
        let held = a.record(Role::Stream).clone();
        // The Laptop took the claim in before the Sanctuary refused it.
        let off = |d: DeviceKey| {
            if d == key(1) {
                Holder::Offline(secs(999))
            } else {
                Holder::Online
            }
        };
        let (claim, reclaim) = refused_claim(&mut a, &mut b);
        assert!(
            c.merge(Role::Stream, &claim, key(2), &net(true, &off))
                .changed
        );
        assert_eq!(c.record(Role::Stream).holder, Some(key(2)));

        // Signed by the holder, on the claim, two steps past where it was.
        assert_eq!(reclaim.holder, Some(key(1)));
        assert_eq!(reclaim.epoch, held.epoch + 2);
        let step = &reclaim.chain[0];
        assert_eq!(step.kind, Kind::Reclaim);
        assert_eq!(step.signer(), Some(key(1)));
        assert!(step.valid(Role::Stream));
        assert_eq!(covered(&reclaim.chain, 0), Some(1));
        assert_eq!(reclaim.chain[1], claim.chain[0]);
        assert!(reclaim.beats(&claim));

        // The claimant hands the role back and is told why.
        let merged = m(&mut b, Role::Stream, &reclaim);
        assert!(merged.changed && merged.lost_role);
        assert!(!b.holds(Role::Stream));
        assert_eq!(b.record(Role::Stream).holder, Some(key(1)));
        assert_eq!(
            b.notice(Role::Stream),
            Some(&Notice::Refused {
                by: "Sanctuary".into()
            })
        );
        // The Laptop, relayed by the claimant, comes back too, with nothing to
        // say: it never held the role.
        let merged = c.merge(Role::Stream, &reclaim, key(2), &net(true, &online));
        assert!(merged.changed && !merged.lost_role);
        assert_eq!(c.record(Role::Stream), &reclaim);
        assert_eq!(c.notice(Role::Stream), None);
        // The claim, met again anywhere, changes nothing.
        for d in [&mut a, &mut b, &mut c] {
            assert!(
                !d.merge(Role::Stream, &claim, key(2), &net(true, &off))
                    .changed
            );
            assert_eq!(d.record(Role::Stream).holder, Some(key(1)));
        }
        // A device that never saw the claim follows the reclaim from where
        // it was, through the claim it never had.
        let mut fresh = device(4, "Back room");
        fresh.seed_any = false;
        fresh.records[Role::Stream.index()] = held.clone();
        assert!(
            fresh
                .merge(Role::Stream, &reclaim, key(1), &net(true, &online))
                .changed
        );
        assert_eq!(fresh.record(Role::Stream), &reclaim);
        // The other roles are untouched.
        assert!(a.holds(Role::Projection) && a.holds(Role::Audio));
    }

    /// A refusal only counts from the device the claim was made against,
    /// signed by it, above that claim.
    #[test]
    fn a_forged_reclaim_is_refused() {
        let (mut a, mut b, c) = church();
        let (claim, reclaim) = refused_claim(&mut a, &mut b);
        let signed = |by: u8, link: &Link| {
            let mut l = link.clone();
            l.sig = kp(by)
                .sign(&Link::bytes(
                    Role::Stream,
                    l.kind,
                    l.holder,
                    l.epoch,
                    l.prev_holder,
                    l.prev_epoch,
                ))
                .to_vec();
            l
        };
        let with = |head: Link, rest: &[Link]| {
            let mut chain = vec![head.clone()];
            chain.extend(rest.iter().cloned());
            Record {
                holder: head.holder,
                name: "Forged".into(),
                epoch: head.epoch,
                chain,
            }
        };
        let genuine = reclaim.chain[0].clone();
        // The Laptop, paired with everyone, signs a "refusal" for itself.
        let mut by_laptop = genuine.clone();
        by_laptop.holder = Some(key(3));
        let by_laptop = signed(3, &by_laptop);
        // The holder's own refusal, with its signature broken.
        let mut broken = genuine.clone();
        broken.sig[0] ^= 1;
        // The holder's signature, copied onto another epoch.
        let mut moved = genuine.clone();
        moved.epoch += 1;
        moved.prev_epoch += 1;
        // A refusal by the claimant of its own claim.
        let mut own = genuine.clone();
        own.holder = Some(key(2));
        let own = signed(2, &own);
        // Signed by the holder, but on a hand-over, not a claim against it.
        let hand_over = {
            let mut a2 = a.clone();
            a2.receive_ask(Role::Stream, key(3), "Laptop".into());
            a2.allow(Role::Stream, key(3), false, &net(true, &online))
                .unwrap()
                .record
        };
        let mut on_hand_over = genuine.clone();
        on_hand_over.prev_holder = Some(key(3));
        on_hand_over.prev_epoch = hand_over.epoch;
        on_hand_over.epoch = hand_over.epoch + 1;
        let on_hand_over = signed(1, &on_hand_over);
        let forgeries = [
            (
                "signed by another device",
                with(by_laptop, &claim.chain),
                claim.clone(),
            ),
            (
                "a broken signature",
                with(broken, &claim.chain),
                claim.clone(),
            ),
            ("another epoch", with(moved, &claim.chain), claim.clone()),
            ("the claimant's own", with(own, &claim.chain), claim.clone()),
            (
                "on a hand-over",
                with(on_hand_over, &hand_over.chain),
                hand_over.clone(),
            ),
        ];
        // The Laptop sees the Sanctuary gone, so it takes the claim in.
        let a_gone = |d: DeviceKey| {
            if d == key(1) {
                Holder::Offline(secs(999))
            } else {
                Holder::Online
            }
        };
        let take = |d: &mut Roles, rec: &Record| {
            assert!(
                d.merge(Role::Stream, rec, rec.holder.unwrap(), &net(true, &a_gone))
                    .changed
            );
        };
        for (what, forged, before) in forgeries {
            // Sent to a device that holds the claim (or the hand-over) the
            // forgery sits on, by every device that could send it.
            for from in [key(1), key(2), key(3)] {
                let mut d = c.clone();
                take(&mut d, &before);
                assert_eq!(d.record(Role::Stream), &before, "{what}");
                let merged = d.merge(Role::Stream, &forged, from, &net(true, &online));
                assert!(!merged.changed, "{what}");
                assert_eq!(d.record(Role::Stream), &before, "{what}");
                assert!(!d.unverified(Role::Stream), "{what}: not a gap either");
            }
        }
        // A reclaim whose claim is not in the steps is checked against the
        // claim this device holds: on another state, it is not taken.
        let mut alone = reclaim.clone();
        alone.chain.truncate(1);
        let mut d = c.clone();
        take(&mut d, &claim);
        assert!(m(&mut d.clone(), Role::Stream, &alone).changed);
        let mut d = c.clone();
        take(&mut d, &hand_over);
        let mut stray = alone.clone();
        stray.chain[0].prev_holder = Some(key(3));
        stray.chain[0] = signed(1, &stray.chain[0]);
        assert!(!m(&mut d, Role::Stream, &stray).changed);
    }

    /// The claimant waits out the cooldown before it may take the role over
    /// from that holder again, however long the holder seems gone; it may
    /// still ask. The cooldown ends by time, and with a new holder.
    #[test]
    fn a_refused_claimant_waits_out_the_cooldown_before_taking_over_again() {
        let (mut a, mut b, _) = church();
        let (_, reclaim) = refused_claim(&mut a, &mut b);
        let at = 1_000_000;
        let mut on = net_away(&online, None);
        on.now = at;
        b.merge(Role::Stream, &reclaim, key(1), &on);
        assert_eq!(b.cooldown(Role::Stream, &on), Some(REFUSAL_COOLDOWN));
        // A clock that went back past the refusal ends it, rather than
        // stretching it by the jump.
        let mut back = net_away(&online, None);
        back.now = at - 3_600_000;
        assert_eq!(b.cooldown(Role::Stream, &back), None);
        // The holder is connected: asking is open.
        assert_eq!(b.standing(Role::Stream, &on), Standing::Ask);
        assert!(b.ask(Role::Stream, &on).is_ok());
        b.cancel_ask(Role::Stream);
        // The holder seems gone again (a link failing one way): not yet.
        let off = |_: DeviceKey| Holder::Offline(secs(999));
        let mut later = net(true, &off);
        later.now = at + 60_000;
        assert_eq!(
            b.standing(Role::Stream, &later),
            Standing::Wait {
                offline: secs(999),
                remaining: REFUSAL_COOLDOWN - secs(60),
            }
        );
        assert_eq!(
            b.claim(Role::Stream, &later),
            Err(Denied::Refused {
                remaining: REFUSAL_COOLDOWN - secs(60)
            })
        );
        // Over: it may, and the claim is an ordinary one.
        later.now = at + i64::try_from(REFUSAL_COOLDOWN.as_millis()).unwrap();
        assert_eq!(b.cooldown(Role::Stream, &later), None);
        assert_eq!(
            b.standing(Role::Stream, &later),
            Standing::TakeOver { offline: secs(999) }
        );
        assert!(b.claim(Role::Stream, &later).is_ok());

        // A cooldown is for the holder that refused: once the role is
        // somebody else's, it does not apply.
        let (mut a, mut b, mut c) = church();
        let (_, reclaim) = refused_claim(&mut a, &mut b);
        b.merge(Role::Stream, &reclaim, key(1), &on);
        let to_laptop = hand(&mut a, Role::Stream, &mut c, 3);
        m(&mut b, Role::Stream, &to_laptop);
        assert_eq!(b.cooldown(Role::Stream, &on), None);
        assert_eq!(
            b.standing(Role::Stream, &net(true, &off)),
            Standing::TakeOver { offline: secs(999) }
        );
        // And unpairing the holder forgets it.
        let (mut a, mut b, _) = church();
        let (_, reclaim) = refused_claim(&mut a, &mut b);
        b.merge(Role::Stream, &reclaim, key(1), &on);
        b.forget(key(1));
        assert_eq!(b.cooldown(Role::Stream, &on), None);
    }

    /// A claim that arrives while the claimant is not connected here (a frame
    /// on a connection being replaced) is judged with nothing known of the
    /// outage: not taken, but not reclaimed either, so it can come again on
    /// the next connection and be judged then.
    #[test]
    fn a_claim_from_a_claimant_not_connected_is_not_reclaimed() {
        let (mut a, mut b, _) = church();
        let off = |_: DeviceKey| Holder::Offline(secs(999));
        let claim = b.claim(Role::Stream, &net(true, &off)).unwrap();
        let held = a.record(Role::Stream).clone();
        let gone = |_: DeviceKey| Holder::Offline(secs(0));
        let merged = a.merge(
            Role::Stream,
            &claim,
            key(2),
            &net_away(&gone, Some(secs(0))),
        );
        assert_eq!(merged, Merged::default());
        assert_eq!(a.record(Role::Stream), &held);
        // On the next connection, away long enough: taken.
        let merged = a.merge(
            Role::Stream,
            &claim,
            key(2),
            &net_away(&online, Some(secs(500))),
        );
        assert!(merged.changed && merged.lost_role && !merged.reclaimed);
    }

    /// At the same epoch a reclaim comes before a hand-over the claimant
    /// made from its claim at the same moment.
    #[test]
    fn a_reclaim_beats_a_hand_over_made_from_the_refused_claim() {
        let (mut a, mut b, mut c) = church();
        let (_, reclaim) = refused_claim(&mut a, &mut b);
        // The Laptop sees the Sanctuary gone too, so it takes the hand-over.
        let gone = net(true, &sanctuary_gone);
        let passed_on = hand_seeing(&mut b, Role::Stream, &mut c, 3, &gone);
        assert_eq!(passed_on.epoch, reclaim.epoch);
        assert!(reclaim.beats(&passed_on) && !passed_on.beats(&reclaim));
        assert!(m(&mut c, Role::Stream, &reclaim).changed);
        assert_eq!(c.record(Role::Stream).holder, Some(key(1)));
    }

    /// Two devices claim from the same state at the same moment. The holder
    /// refuses the first with a reclaim; the second claim then no longer
    /// beats its record, and what reaches the second claimant is the reclaim
    /// of the first. It does not pass through the second claimant's own
    /// state, but it shows the holder kept the role over every claim from
    /// that state: the second claimant gives the role back too, is told, and
    /// waits out the cooldown.
    #[test]
    fn two_claims_at_the_same_moment_are_both_handed_back() {
        let (mut a, mut b, mut c) = church();
        let off = |_: DeviceKey| Holder::Offline(secs(999));
        let claim_c = c.claim(Role::Stream, &net(true, &off)).unwrap();
        let (claim_b, reclaim) = refused_claim(&mut a, &mut b);
        assert_eq!(claim_b.epoch, claim_c.epoch);
        // The second claim, met at the holder: behind it now.
        let merged = a.merge(
            Role::Stream,
            &claim_c,
            key(3),
            &net_away(&online, Some(secs(3))),
        );
        assert!(!merged.changed);
        assert!(a.record(Role::Stream).beats(&claim_c));
        // The holder answers with its record, and both claimants give way.
        for d in [&mut b, &mut c] {
            let merged = d.merge(Role::Stream, &reclaim, key(1), &net_away(&online, None));
            assert!(merged.changed && merged.lost_role);
            assert!(!d.unverified(Role::Stream));
            assert_eq!(d.record(Role::Stream), &reclaim);
            assert_eq!(
                d.notice(Role::Stream),
                Some(&Notice::Refused {
                    by: "Sanctuary".into()
                })
            );
            assert_eq!(
                d.cooldown(Role::Stream, &net_away(&online, None)),
                Some(REFUSAL_COOLDOWN)
            );
        }
        assert!(a.holds(Role::Stream) && !b.holds(Role::Stream) && !c.holds(Role::Stream));
    }

    /// The refused claimant hands the role on twice before the refusal
    /// reaches it (a link failing one way), to devices that also believe the
    /// holder gone. The last of those beats the reclaim by its epoch, but it
    /// is built on the claim the holder refused: the holder reclaims again on
    /// top of it, and every device comes back to it. (Otherwise a device
    /// could take a role from a connected holder by handing it on twice.)
    #[test]
    fn the_holder_takes_the_role_back_from_hand_overs_made_from_the_claim_it_refused() {
        let (mut a, mut b, mut c) = church();
        let mut d = device(4, "Back room");
        m(&mut d, Role::Stream, a.record(Role::Stream));
        let (_, reclaim) = refused_claim(&mut a, &mut b);
        let gone = net(true, &sanctuary_gone);
        hand_seeing(&mut b, Role::Stream, &mut c, 3, &gone);
        let to_d = hand_seeing(&mut c, Role::Stream, &mut d, 4, &gone);
        assert!(to_d.beats(&reclaim));
        let merged = a.merge(Role::Stream, &to_d, key(3), &net(true, &online));
        assert!(
            merged.changed && merged.reclaimed && !merged.lost_role,
            "{merged:?}"
        );
        assert!(a.holds(Role::Stream));
        let answer = a.record(Role::Stream).clone();
        assert!(answer.beats(&to_d));
        assert_eq!(covered(&answer.chain, 0), Some(3), "it cancels the claim");
        for (x, by) in [(&mut b, "claimant"), (&mut c, "relay"), (&mut d, "last")] {
            assert!(
                x.merge(Role::Stream, &answer, key(1), &net(true, &online))
                    .changed,
                "{by}"
            );
            assert_eq!(x.record(Role::Stream), &answer, "{by}");
        }
        assert_eq!(
            d.notice(Role::Stream),
            Some(&Notice::Replaced {
                by: "Sanctuary".into()
            })
        );
        let holders = [&a, &b, &c, &d]
            .iter()
            .filter(|r| r.holds(Role::Stream))
            .count();
        assert_eq!(holders, 1);
    }

    /// The case reclaims are for, in any order: the holder is never really
    /// away, but devices that have its present record believe it gone and
    /// claim (several at once, or one after another), the claimants hand the
    /// role on before the refusal reaches them, and records reach devices in
    /// any order. Once every record has reached every device (all connected
    /// all along), they agree on who holds the role and exactly one device
    /// does. Small random runs with fixed seeds, so a failure repeats. (The
    /// wider model, with links that drop, is
    /// `whatever_the_links_do_every_device_ends_agreeing_on_one_holder`.)
    #[test]
    fn after_refused_claims_and_hand_overs_in_any_order_one_device_holds_the_role() {
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        let off = |_: DeviceKey| Holder::Offline(secs(999));
        let connected = net_away(&online, None);
        let names = ["Sanctuary", "Office PC", "Laptop", "Back room"];
        let k = |i: usize| key(u8::try_from(i).unwrap() + 1);
        let who = |key_of: Option<DeviceKey>| key_of.and_then(|d| (0..4).position(|i| k(i) == d));
        for run in 0..500 {
            let mut ds: Vec<Roles> = (0..4)
                .map(|i| device(u8::try_from(i).unwrap() + 1, names[i]))
                .collect();
            let first = ds[0].claim(Role::Stream, &net(true, &online)).unwrap();
            for d in ds.iter_mut().skip(1) {
                m(d, Role::Stream, &first);
            }
            // The holder, as the one that would judge a claim: device 0
            // until it hands the role on itself.
            let mut holder = 0;
            let mut log = Vec::new();
            for _ in 0..6 {
                let (x, y) = (next(4) as usize, next(4) as usize);
                match next(3) {
                    // `x`, with the holder's present record, believes it gone
                    // and takes over.
                    0 if x != holder => {
                        let rec = ds[holder].record(Role::Stream).clone();
                        ds[x].merge(Role::Stream, &rec, k(holder), &connected);
                        if ds[x].record(Role::Stream) == &rec
                            && ds[x].claim(Role::Stream, &net(true, &off)).is_ok()
                        {
                            log.push(format!("{x} claims from {holder}"));
                        }
                    }
                    // A device that believes it holds the role hands it on.
                    1 if x != y && ds[x].holds(Role::Stream) => {
                        ds[x].receive_ask(Role::Stream, k(y), names[y].into());
                        if ds[x].allow(Role::Stream, k(y), false, &connected).is_ok() {
                            if x == holder {
                                holder = y;
                            }
                            log.push(format!("{x} hands to {y}"));
                        }
                    }
                    // `x`'s record reaches `y`.
                    _ if x != y => {
                        let rec = ds[x].record(Role::Stream).clone();
                        if ds[y].merge(Role::Stream, &rec, k(x), &connected).changed {
                            log.push(format!("{x} -> {y}"));
                            if y == holder && !ds[y].holds(Role::Stream) {
                                holder = who(ds[y].record(Role::Stream).holder).unwrap();
                            }
                        }
                    }
                    _ => {}
                }
            }
            // Everyone meets everyone, connected, until nothing changes.
            for _ in 0..50 {
                let mut changed = false;
                for x in 0..4 {
                    for y in 0..4 {
                        if x != y {
                            let rec = ds[x].record(Role::Stream).clone();
                            changed |= ds[y].merge(Role::Stream, &rec, k(x), &connected).changed;
                        }
                    }
                }
                if !changed {
                    break;
                }
            }
            let states: Vec<_> = ds
                .iter()
                .map(|d| {
                    let r = d.record(Role::Stream);
                    let steps: Vec<_> = r
                        .chain
                        .iter()
                        .map(|l| (l.kind, who(l.holder), l.epoch))
                        .collect();
                    (who(r.holder), r.epoch, d.unverified(Role::Stream), steps)
                })
                .collect();
            let what = format!("run {run}: {log:?}\n{states:?}");
            let agreed = ds[0].record(Role::Stream).holder;
            assert!(agreed.is_some(), "{what}");
            assert!(
                ds.iter().all(|d| d.record(Role::Stream).holder == agreed),
                "{what}"
            );
            assert_eq!(
                ds.iter().filter(|d| d.holds(Role::Stream)).count(),
                1,
                "{what}"
            );
        }
    }

    /// What one device's node knows of a peer: whether a session is up, and
    /// when it last heard from it.
    #[derive(Debug, Clone, Copy, Default)]
    struct Contact {
        up: bool,
        /// Which session, so a frame sent on one that has ended is lost.
        session: u64,
        /// When this device last heard the peer (while up, it hears it now).
        heard: u64,
        /// When the present session opened, and how long the peer had been
        /// silent before it.
        opened: u64,
        away_before: u64,
    }

    /// One device's [`Net`], as a node builds it from its contacts: the
    /// same rules as `net::Reach`, with the ten second fresh session.
    struct Seen {
        me: usize,
        row: Vec<Contact>,
        now: u64,
    }

    /// Device `i` of the model has key `i + 1`.
    fn model_key(i: usize) -> DeviceKey {
        key(u8::try_from(i).unwrap() + 1)
    }

    impl Seen {
        fn index(&self, d: DeviceKey) -> Option<usize> {
            (0..self.row.len()).find(|i| model_key(*i) == d)
        }
    }

    impl Net for Seen {
        fn has_peers(&self) -> bool {
            true
        }
        fn holder(&self, d: DeviceKey) -> Holder {
            match self.index(d) {
                None => Holder::Gone,
                Some(i) if i == self.me => Holder::Me,
                Some(i) if self.row[i].up => Holder::Online,
                Some(i) => Holder::Offline(secs(self.now - self.row[i].heard)),
            }
        }
        fn away(&self, d: DeviceKey) -> Option<Duration> {
            let c = self.row[self.index(d)?];
            if c.up {
                (self.now - c.opened <= 10).then(|| secs(c.away_before))
            } else {
                Some(secs(self.now - c.heard))
            }
        }
        fn paired(&self, d: DeviceKey) -> bool {
            self.index(d).is_some()
        }
        fn now_ms(&self) -> i64 {
            i64::try_from(self.now * 1000).unwrap()
        }
    }

    /// Four paired devices whose links drop and come back, each judging by
    /// what its own node has seen, with frames that are lost when their
    /// session ends and otherwise arrive in any order. Each device does what
    /// its node does: a record that changes its own goes to every connected
    /// device, a record its own beats is answered with its own, and one
    /// with a claim for a connected device to judge is passed on to it.
    struct World {
        ds: Vec<Roles>,
        now: u64,
        /// `contact[x][y]`: what device `x` knows of device `y`.
        contact: Vec<Vec<Contact>>,
        sessions: u64,
        /// Frames on their way: from, to, the session, the record.
        queue: Vec<(usize, usize, u64, Record)>,
        log: Vec<String>,
    }

    impl World {
        fn new(n: usize) -> Self {
            let names = ["Sanctuary", "Office PC", "Laptop", "Back room"];
            let ds: Vec<Roles> = (0..n)
                .map(|i| device(u8::try_from(i).unwrap() + 1, names[i]))
                .collect();
            let mut w = World {
                ds,
                now: 1000,
                contact: vec![vec![Contact::default(); n]; n],
                sessions: 0,
                queue: Vec::new(),
                log: Vec::new(),
            };
            for x in 0..n {
                for y in 0..n {
                    if x < y {
                        w.link_up(x, y, false);
                    }
                }
            }
            // Device 0 holds the role, and everyone knows it.
            let seen = w.seen(0);
            w.ds[0].claim(Role::Stream, &seen).unwrap();
            let first = w.ds[0].record(Role::Stream).clone();
            for d in w.ds.iter_mut().skip(1) {
                m(d, Role::Stream, &first);
            }
            // A while later: no session is fresh any more.
            w.now += 60;
            w
        }

        fn seen(&self, me: usize) -> Seen {
            Seen {
                me,
                row: self.contact[me].clone(),
                now: self.now,
            }
        }

        fn up(&self, x: usize, y: usize) -> bool {
            self.contact[x][y].up
        }

        /// The link comes up: a session opens, each side counts how long the
        /// other had been silent, and both send their records.
        fn link_up(&mut self, x: usize, y: usize, exchange: bool) {
            self.sessions += 1;
            for (a, b) in [(x, y), (y, x)] {
                let c = &mut self.contact[a][b];
                c.away_before = self.now - c.heard;
                c.opened = self.now;
                c.heard = self.now;
                c.up = true;
                c.session = self.sessions;
            }
            if exchange {
                self.send(x, y);
                self.send(y, x);
            }
        }

        /// The link drops. Each side last heard the other a little before
        /// (a session is closed after some silence), not at the same moment.
        fn link_down(&mut self, x: usize, y: usize, lag: [u64; 2]) {
            for ((a, b), lag) in [(x, y), (y, x)].into_iter().zip(lag) {
                let c = &mut self.contact[a][b];
                c.up = false;
                c.heard = self.now.saturating_sub(lag).max(c.opened);
            }
        }

        fn send(&mut self, from: usize, to: usize) {
            let record = self.ds[from].record(Role::Stream).clone();
            self.send_record(from, to, record);
        }

        fn send_record(&mut self, from: usize, to: usize, record: Record) {
            if self.up(from, to) {
                let session = self.contact[from][to].session;
                self.queue.push((from, to, session, record));
            }
        }

        fn broadcast(&mut self, from: usize) {
            for to in 0..self.ds.len() {
                if to != from {
                    self.send(from, to);
                }
            }
        }

        /// The `n`th frame on its way arrives, if its session is still up.
        fn deliver(&mut self, n: usize) {
            let (from, to, session, record) = self.queue.swap_remove(n);
            let c = self.contact[to][from];
            if !c.up || c.session != session {
                return;
            }
            let seen = self.seen(to);
            let merged = self.ds[to].merge(Role::Stream, &record, model_key(from), &seen);
            if let Some((judge, passed)) = merged.refer {
                let judge = self.who(Some(judge)).unwrap();
                self.send_record(to, judge, passed);
            }
            if merged.changed {
                self.log.push(format!(
                    "{from} -> {to}: {:?}",
                    self.who(self.ds[to].record(Role::Stream).holder)
                ));
                self.broadcast(to);
            } else if self.ds[to].record(Role::Stream).beats(&record) {
                self.send(to, from);
            }
        }

        fn who(&self, d: Option<DeviceKey>) -> Option<usize> {
            d.and_then(|d| (0..self.ds.len()).find(|i| model_key(*i) == d))
        }

        fn max_epoch(&self) -> u64 {
            self.ds
                .iter()
                .map(|d| d.record(Role::Stream).epoch)
                .max()
                .unwrap_or(0)
        }

        /// One random thing happens.
        fn act(&mut self, next: &mut dyn FnMut(u64) -> u64) {
            let n = self.ds.len();
            let (x, y) = (next(n as u64) as usize, next(n as u64) as usize);
            // Keep histories inside what a record carries, so every device
            // can always follow (a longer gap is the "can't check" case).
            let room = self.max_epoch() < 7;
            match next(9) {
                0 => {
                    let d = 1 + next(200);
                    self.now += d;
                    self.log.push(format!("{d}s pass"));
                }
                1 if x != y && self.up(x, y) => {
                    self.link_down(x, y, [next(36), next(36)]);
                    self.log.push(format!("{x} and {y} part"));
                }
                2 if x != y && !self.up(x, y) => {
                    self.log.push(format!("{x} and {y} meet"));
                    self.link_up(x, y, true);
                }
                // `x` takes over, if it may by what it has seen.
                3 if room => {
                    let seen = self.seen(x);
                    if self.ds[x].claim(Role::Stream, &seen).is_ok() {
                        self.log.push(format!("{x} claims"));
                        self.broadcast(x);
                    }
                }
                // `y` asks `x`, which it believes holds the role; `x` allows.
                4 if room && x != y && self.up(x, y) => {
                    let (sx, sy) = (self.seen(x), self.seen(y));
                    if self.ds[y].record(Role::Stream).holder == Some(model_key(x))
                        && self.ds[y].ask(Role::Stream, &sy).is_ok()
                    {
                        let name = self.ds[y].name.clone();
                        self.ds[x].receive_ask(Role::Stream, model_key(y), name);
                        if self.ds[x]
                            .allow(Role::Stream, model_key(y), false, &sx)
                            .is_ok()
                        {
                            self.log.push(format!("{x} hands to {y}"));
                            self.broadcast(x);
                        }
                        self.ds[y].cancel_ask(Role::Stream);
                    }
                }
                _ if !self.queue.is_empty() => {
                    let k = next(self.queue.len() as u64) as usize;
                    self.deliver(k);
                }
                _ => {}
            }
        }

        /// Every link comes up and stays up, and every frame arrives.
        fn heal(&mut self, next: &mut dyn FnMut(u64) -> u64) -> bool {
            let n = self.ds.len();
            for x in 0..n {
                for y in x + 1..n {
                    if !self.up(x, y) {
                        self.link_up(x, y, true);
                    }
                }
            }
            for _ in 0..10_000 {
                if self.queue.is_empty() {
                    return true;
                }
                let k = next(self.queue.len() as u64) as usize;
                self.deliver(k);
            }
            false
        }

        /// What is wrong at the end, if anything: every device should agree
        /// on the holder, and exactly that one device should hold the role.
        fn split(&self) -> Option<String> {
            let agreed = self.ds[0].record(Role::Stream).holder;
            let holders = self.ds.iter().filter(|d| d.holds(Role::Stream)).count();
            if agreed.is_some()
                && holders == 1
                && self
                    .ds
                    .iter()
                    .all(|d| d.record(Role::Stream).holder == agreed)
            {
                return None;
            }
            let states: Vec<_> = self
                .ds
                .iter()
                .map(|d| {
                    let r = d.record(Role::Stream);
                    let steps: Vec<_> = r
                        .chain
                        .iter()
                        .map(|l| {
                            (
                                l.kind,
                                self.who(l.prev_holder),
                                self.who(l.holder),
                                l.epoch,
                                l.ack.is_some(),
                            )
                        })
                        .collect();
                    (
                        self.who(r.holder),
                        r.epoch,
                        d.unverified(Role::Stream),
                        steps,
                    )
                })
                .collect();
            Some(format!("{:?}\n{states:?}", self.log))
        }
    }

    /// The wider case: links drop and come back, at different moments on
    /// each side; devices take over when what they have seen allows it, hand
    /// the role on, and relay records in any order. Claims are made against
    /// devices that are no longer the holder, and records reach devices by
    /// every path. Once every link is back and every frame has arrived, all
    /// four agree on the holder and exactly that device holds the role.
    #[test]
    fn whatever_the_links_do_every_device_ends_agreeing_on_one_holder() {
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        let mut failed = Vec::new();
        let runs = 3000;
        for run in 0..runs {
            let mut w = World::new(4);
            for _ in 0..80 {
                w.act(&mut next);
            }
            let settled = w.heal(&mut next);
            if !settled {
                failed.push(format!("run {run}: never settles\n{:?}", w.log));
            } else if let Some(what) = w.split() {
                failed.push(format!("run {run}: {what}"));
            }
        }
        assert!(
            failed.is_empty(),
            "{} of {runs} runs end split; the first:\n{}",
            failed.len(),
            failed[0]
        );
    }

    /// A paired device that has been connected to the holder all along signs
    /// a claim against it and then hands the role on at once, so the claim
    /// is no longer the newest step; with two hand-overs or more its record
    /// also outruns the holder's refusal by epoch. The holder does not give
    /// the role up, a device that sees the holder connected does not take
    /// the record in, whoever passes it on, and once records have gone round
    /// every honest device is back on the holder.
    #[test]
    fn a_forged_chain_cannot_take_a_role_from_a_connected_holder() {
        // A step on top of `rec`, signed by `by`.
        let forge = |rec: &Record, kind: Kind, to: u8, by: u8| {
            let mut l = rec.chain[0].clone();
            l.kind = kind;
            l.holder = Some(key(to));
            l.epoch = rec.epoch + 1;
            l.prev_holder = rec.holder;
            l.prev_epoch = rec.epoch;
            l.sig = kp(by)
                .sign(&Link::bytes(
                    Role::Stream,
                    kind,
                    l.holder,
                    l.epoch,
                    l.prev_holder,
                    l.prev_epoch,
                ))
                .to_vec();
            let mut chain = vec![l];
            chain.extend(rec.chain.iter().take(CHAIN - 1).cloned());
            Record {
                holder: Some(key(to)),
                name: "Forged".into(),
                epoch: rec.epoch + 1,
                chain,
            }
        };
        let off = |_: DeviceKey| Holder::Offline(secs(999));
        // Everyone connected, on sessions that have been up for a while.
        let all_on = net_away(&online, None);
        for hops in 0..=3 {
            for last in [2, 3] {
                let (a, b, c) = church();
                let held = a.record(Role::Stream).clone();
                let mut forger = b.clone();
                let mut forged = forger.claim(Role::Stream, &net(true, &off)).unwrap();
                for hop in 0..hops {
                    let to = if hop + 1 == hops { last } else { 2 };
                    forged = forge(&forged, Kind::HandOver, to, 2);
                }
                let what = format!("{hops} hand-overs, to device {last}");
                // From the forger, or passed on by a fourth paired device.
                for from in [key(2), key(4)] {
                    // The holder keeps the role, and its record now beats the
                    // forged one everywhere.
                    let mut holder = a.clone();
                    holder.merge(Role::Stream, &forged, from, &all_on);
                    assert!(holder.holds(Role::Stream), "{what}");
                    assert!(holder.record(Role::Stream).beats(&forged), "{what}");
                    // A device that sees the holder connected does not move.
                    let mut third = c.clone();
                    third.merge(Role::Stream, &forged, from, &all_on);
                    assert_eq!(third.record(Role::Stream), &held, "{what}");
                    assert!(!third.holds(Role::Stream), "{what}");
                    // The holder's answer reaches it: still the holder.
                    let answer = holder.record(Role::Stream).clone();
                    third.merge(Role::Stream, &answer, key(1), &all_on);
                    assert_eq!(third.record(Role::Stream).holder, Some(key(1)), "{what}");
                    // And so does the forger's own honest twin, which only
                    // ever saw the forged record (it had been told of it).
                    let mut twin = b.clone();
                    twin.merge(Role::Stream, &forged, key(4), &net(true, &off));
                    twin.merge(Role::Stream, &answer, key(1), &all_on);
                    assert_eq!(twin.record(Role::Stream).holder, Some(key(1)), "{what}");
                    // The forged record, met again, changes nothing.
                    for d in [&mut holder, &mut third] {
                        assert!(
                            !d.merge(Role::Stream, &forged, from, &all_on).changed,
                            "{what}"
                        );
                    }
                }
            }
        }
    }

    /// A connected paired device signs a claim against the holder and a
    /// hand-over straight back to it, so the holder takes it in and keeps the
    /// role. Then it parts from its own claim the other way: it lets go and
    /// claims the role nobody holds. The holder never accepted the claim, so
    /// the record is judged from below it: the holder reclaims, and a device
    /// that sees the holder connected does not follow the forger.
    #[test]
    fn a_claim_handed_straight_back_cannot_be_reused_to_take_the_role() {
        let step = |rec: &Record, kind: Kind, to: Option<u8>| {
            let mut l = rec.chain[0].clone();
            l.kind = kind;
            l.holder = to.map(key);
            l.epoch = rec.epoch + 1;
            l.prev_holder = rec.holder;
            l.prev_epoch = rec.epoch;
            l.ack = None;
            l.sig = kp(2)
                .sign(&Link::bytes(
                    Role::Stream,
                    kind,
                    l.holder,
                    l.epoch,
                    l.prev_holder,
                    l.prev_epoch,
                ))
                .to_vec();
            let mut chain = vec![l];
            chain.extend(rec.chain.iter().take(CHAIN - 1).cloned());
            Record {
                holder: to.map(key),
                name: String::new(),
                epoch: rec.epoch + 1,
                chain,
            }
        };
        let (mut a, b, mut c) = church();
        let all_on = net_away(&online, None);
        let off = |_: DeviceKey| Holder::Offline(secs(999));
        let claim = b.clone().claim(Role::Stream, &net(true, &off)).unwrap();
        let back = step(&claim, Kind::HandOver, Some(1));
        for d in [&mut a, &mut c] {
            d.merge(Role::Stream, &back, key(2), &all_on);
            assert_eq!(d.record(Role::Stream).holder, Some(key(1)));
        }
        let mine = a.record(Role::Stream).clone();
        assert!(mine.chain.iter().all(|l| l.ack.is_none()), "nothing acked");
        let reused = step(&step(&claim, Kind::Release, None), Kind::Claim, Some(2));
        let merged = a.merge(Role::Stream, &reused, key(2), &all_on);
        assert!(merged.reclaimed, "{merged:?}");
        assert!(a.holds(Role::Stream) && a.record(Role::Stream).beats(&reused));
        c.merge(Role::Stream, &reused, key(2), &all_on);
        assert_eq!(c.record(Role::Stream).holder, Some(key(1)));
        let answer = a.record(Role::Stream).clone();
        c.merge(Role::Stream, &answer, key(1), &all_on);
        assert_eq!(c.record(Role::Stream), &answer);
    }

    /// The Sanctuary dies and the Office PC takes over; nobody can ack that
    /// claim. Hand-overs follow until the claim is the oldest step a record
    /// carries. The Back room, cut off, then takes over from the Office PC
    /// while it passes the role on and gets it back, so its record no longer
    /// carries the claim. The Back room still parts from the state above
    /// the unacked claim, and lets go: one holder.
    #[test]
    fn a_takeover_from_a_dead_holder_does_not_stop_later_forks_from_joining() {
        let (_, mut b, mut c) = church();
        let mut d = device(4, "Back room");
        let gone = net(true, &sanctuary_gone);
        let stream = c.record(Role::Stream).clone();
        m(&mut d, Role::Stream, &stream);
        let claim = b.claim(Role::Stream, &gone).unwrap();
        for x in [&mut c, &mut d] {
            assert!(x.merge(Role::Stream, &claim, key(2), &gone).changed);
        }
        for hop in 0..6 {
            let rec = if hop % 2 == 0 {
                hand_seeing(&mut b, Role::Stream, &mut c, 3, &gone)
            } else {
                hand_seeing(&mut c, Role::Stream, &mut b, 2, &gone)
            };
            assert!(d.merge(Role::Stream, &rec, key(3), &gone).changed);
        }
        assert!(b.holds(Role::Stream));
        assert!(d.record(Role::Stream).chain.contains(&claim.chain[0]));
        let b_gone = |k: DeviceKey| {
            if k == key(1) || k == key(2) {
                Holder::Offline(secs(999))
            } else {
                Holder::Online
            }
        };
        d.claim(Role::Stream, &net(true, &b_gone)).unwrap();
        hand_seeing(&mut b, Role::Stream, &mut c, 3, &gone);
        let back = hand_seeing(&mut c, Role::Stream, &mut b, 2, &gone);
        assert!(!back.chain.contains(&claim.chain[0]));
        let merged = d.merge(Role::Stream, &back, key(2), &gone);
        assert!(merged.changed && merged.lost_role, "{merged:?}");
        assert!(!d.unverified(Role::Stream));
        assert_eq!(d.record(Role::Stream), &back);
    }

    /// The holder dies and a device takes over from it; nobody can ever ack
    /// that claim. The role is handed on six times, so the claim is the
    /// oldest step a record still carries. Then a device cut off from the
    /// new holder takes over from it, while the holder hands the role on
    /// twice more, so the claim from the dead device drops out of that
    /// record. The device that took over must still join the holder's record
    /// where the two parted and let go: never two holders.
    #[test]
    fn a_fork_above_a_takeover_from_a_dead_holder_still_settles() {
        let (_a, mut b, mut c) = church();
        let mut d = device(4, "Back room");
        for role in Role::ALL {
            if c.record(role).holder.is_some() {
                m(&mut d, role, &c.record(role).clone());
            }
        }
        // The Sanctuary (1), which held Stream, is gone; the rest are up.
        let dead = |k: DeviceKey| {
            if k == key(1) {
                Holder::Offline(secs(999))
            } else {
                Holder::Online
            }
        };
        let n = net_away(&dead, None);
        let claim = b.claim(Role::Stream, &n).unwrap();
        c.merge(Role::Stream, &claim, key(2), &n);
        d.merge(Role::Stream, &claim, key(2), &n);
        assert_eq!(d.record(Role::Stream).holder, Some(key(2)));
        // `x` (key `xk`) hands to `y` (key `yk`), and `z` is told.
        let hand = |x: &mut Roles, y: &mut Roles, z: &mut Roles, xk: u8, yk: u8| {
            y.ask(Role::Stream, &n).unwrap();
            let name = y.name.clone();
            x.receive_ask(Role::Stream, key(yk), name);
            let rec = x.allow(Role::Stream, key(yk), false, &n).unwrap().record;
            y.cancel_ask(Role::Stream);
            y.merge(Role::Stream, &rec, key(xk), &n);
            z.merge(Role::Stream, &rec, key(xk), &n);
            rec
        };
        for _ in 0..3 {
            hand(&mut b, &mut c, &mut d, 2, 3);
            hand(&mut c, &mut b, &mut d, 3, 2);
        }
        assert!(b.holds(Role::Stream));
        assert!(d.record(Role::Stream).chain.iter().any(|l| {
            l.kind == Kind::Claim && l.prev_holder == Some(key(1)) && l.ack.is_none()
        }));
        // d, cut off from the Office PC (2), takes over from it ...
        let cut_off = |k: DeviceKey| {
            if k == key(1) || k == key(2) {
                Holder::Offline(secs(999))
            } else {
                Holder::Online
            }
        };
        d.claim(Role::Stream, &net_away(&cut_off, None)).unwrap();
        // ... while the Office PC hands to the Laptop and gets it back.
        let mut elsewhere = device(5, "Elsewhere");
        hand(&mut b, &mut c, &mut elsewhere, 2, 3);
        let last = hand(&mut c, &mut b, &mut elsewhere, 3, 2);
        assert!(!last.chain.iter().any(|l| l.prev_holder == Some(key(1))));
        d.merge(Role::Stream, &last, key(2), &n);
        assert!(b.holds(Role::Stream));
        assert!(!d.holds(Role::Stream), "two holders");
        assert!(!d.unverified(Role::Stream));
        assert_eq!(d.record(Role::Stream), &last);
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
        assert!(!a.merge(Role::Projection, &take, key(2), &b_net).changed);
        // And a paired device that had only just reconnected has not been
        // away long enough either.
        // (It is answered with a reclaim, as any claim against the holder
        // that the holder refuses.)
        let merged = a.merge(
            Role::Projection,
            &take,
            key(9),
            &net_away(&online, Some(secs(5))),
        );
        assert!(merged.reclaimed);
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
        let mut judge = a.clone();
        assert!(
            judge
                .merge(
                    Role::Audio,
                    &claim,
                    key(3),
                    &net_away(&online, Some(secs(3)))
                )
                .reclaimed
        );
        assert!(judge.holds(Role::Audio));
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
    fn a_takeover_inside_a_catch_up_chain_is_taken_once_the_old_holder_has_accepted_it() {
        // O holds Stream. O is away; P takes it; O returns and yields; P
        // hands it to Q. T missed all of it, and sees O online now. Only O
        // can say whether it was away, and it said so with its ack.
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
        // O's ack is in O's record; P made its hand-over before it came.
        assert!(ring[0].record(Role::Stream).chain[0].ack.is_some());
        assert!(to_q.chain[1].ack.is_none());
        // T sees O online and the claim carries no word from O: T leaves it
        // to O, and passes it on to O.
        let merged = t.merge(Role::Stream, &to_q, key(3), &net(true, &online));
        assert!(!merged.changed && !t.unverified(Role::Stream));
        let (judge, passed) = merged.refer.expect("passed on to its judge");
        assert_eq!(judge, key(1));
        // O takes the hand-over in, keeping its ack, and so beats the copy
        // without it; T then takes O's record from anyone.
        assert!(
            ring[0]
                .merge(Role::Stream, &passed, key(8), &net(true, &online))
                .changed
        );
        let answer = ring[0].record(Role::Stream).clone();
        assert_eq!(answer.holder, Some(key(3)));
        assert!(answer.chain[1].ack.is_some() && answer.beats(&to_q));
        assert!(
            t.merge(Role::Stream, &answer, key(3), &net(true, &online))
                .changed
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
        let (a, mut b) = pair();
        let off = |_: DeviceKey| Holder::Offline(secs(999));
        let claim = b.claim(Role::Audio, &net(true, &off)).unwrap();
        for role_net in [net_away(&online, None), net_away(&online, Some(secs(3)))] {
            let mut a = a.clone();
            assert!(a.merge(Role::Audio, &claim, key(2), &role_net).reclaimed);
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

    #[test]
    fn a_paired_claimant_on_an_old_connection_is_a_known_device() {
        // "Away" is only ever about a fresh connection; whether a device is
        // known is about pairing. A connection that has been up for hours
        // (no "away") must not make a paired device a stranger.
        let mut a = device(1, "A");
        let mut b = device(2, "B");
        let n = net(true, &online);
        assert_eq!(n.away(key(2)), None);
        assert!(n.paired(key(2)));
        let claim = b
            .claim(Role::Stream, &net(true, &|_| Holder::Gone))
            .unwrap();
        assert!(a.merge(Role::Stream, &claim, key(2), &n).changed);
        // And so can the very first claims of the main roles.
        let first = b
            .claim(Role::Projection, &net(true, &|_| Holder::Gone))
            .unwrap();
        assert!(a.merge(Role::Projection, &first, key(2), &n).changed);
        let op = b.claim(Role::Operator, &net(true, &online)).unwrap();
        assert!(a.merge(Role::Operator, &op, key(2), &n).changed);
    }

    #[test]
    fn a_device_not_taught_by_the_one_it_just_paired_with_waits_and_is_flagged() {
        let mut ring = [device(1, "O"), device(2, "P")];
        ring[0].claim(Role::Stream, &net(true, &online)).unwrap();
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
            assert!(
                !fresh
                    .merge(Role::Stream, record, key(7), &net(true, &online))
                    .changed
            );
            assert!(fresh.unverified(Role::Stream), "flagged, not silent");
        }
        // Chains signed by keys nobody knows are not a gap: no flag.
        let mut fresh = device(6, "New laptop");
        fresh.seed_any = false;
        let mut n = net(true, &online);
        n.strangers = vec![key(1), key(2)];
        assert!(!fresh.merge(Role::Stream, &signed, key(7), &n).changed);
        assert!(!fresh.unverified(Role::Stream));
    }

    #[test]
    fn the_first_state_it_is_taught_is_the_only_one_taken_on_trust() {
        let mut ring = [device(1, "O"), device(2, "P")];
        ring[0].claim(Role::Stream, &net(true, &online)).unwrap();
        let signed = pass_around(&mut ring, 0, Role::Stream, 10).pop().unwrap();
        let mut fresh = device(6, "New laptop");
        fresh.seed_any = false;
        fresh.seed_from(key(1));
        // The device just paired with speaks about every role (most of them
        // have no holder): after that nobody is "just paired" any more.
        for role in Role::ALL {
            fresh.merge(role, &Record::default(), key(1), &net(true, &online));
        }
        assert!(
            !fresh
                .merge(Role::Stream, &signed, key(1), &net(true, &online))
                .changed
        );
        assert!(fresh.unverified(Role::Stream));
    }

    #[test]
    fn moved_over_state_is_taken_from_the_device_that_holds_the_lease_only() {
        let migrated = |n: u8, holder: u8, epoch: u64| {
            Roles::new(
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
            )
        };
        let leader = migrated(2, 2, 3);
        let mut lagging = migrated(1, 1, 2);
        let said = leader.record(Role::Projection).clone();
        let n = net(true, &online);
        // Relayed by somebody else, or sent by a device that does not hold
        // it: no.
        assert!(!lagging.merge(Role::Projection, &said, key(5), &n).changed);
        assert!(lagging.merge(Role::Projection, &said, key(2), &n).changed);
    }
}
