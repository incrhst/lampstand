//! Sound cues: the devices that run a service tell the Audio holder what to
//! play, and the Audio holder tells everyone what is playing (decision 0010,
//! points 8 and 9).
//!
//! - **Where a cue goes.** The Audio holder plays. With nobody paired, or
//!   with this device holding Audio, the app plays the sound itself
//!   ([`SoundRoute::Local`]). Otherwise the cue goes to the device that
//!   holds Audio ([`SoundRoute::Sent`]).
//! - **Nothing is kept.** A cue is sent only to a connected Audio holder. A
//!   cue for an Audio holder that is away is not kept to be played when it
//!   returns, since music that starts ten minutes late is worse than music
//!   that did not start: the caller is told ([`SoundRoute::Away`]) and shows
//!   it. For the same reason nothing new starts while Projection is away:
//!   only the Projection and Operator holders can start a sound.
//! - **Who may send which.** `Play` counts only from the Projection holder or
//!   the Operator holder, as the receiving device sees it. `Pause`, `Resume`
//!   and `Stop` too. `Panic` counts from any paired device: it only ever makes
//!   things quieter, and it must work whoever is asking (a helper's panic
//!   button reaches the Audio holder through the device the helper is
//!   connected to). From an unpaired device nothing counts. A cue sent to a
//!   device that does not hold Audio is ignored.
//! - **What a cue carries.** The hash of the file (never a path), a name for
//!   the screens, and the id of the item it belongs to. Names and ids that are
//!   too long are not believed. The Audio holder plays only a file it has.
//! - **What is playing.** The Audio holder sends its state ([`SoundNow`]) to
//!   every connected device when it changes, and again when a device
//!   connects. A device believes it only from the device that holds Audio
//!   and is connected, so it goes when that device goes.
//! - **Why a sound cannot play.** When a sound does not play on the Audio
//!   holder (the file is not there yet, cannot be read, or the sound output
//!   is not working) it says so with a [`SoundProblem`]: a reason from a
//!   fixed list ([`SoundReason`]), the id of the item and the name of the
//!   sound. Never a path, never free text: each screen puts the words to
//!   it, so the Operator and the Projection holder can say it in plain words
//!   next to the item. It goes to the same devices as [`SoundNow`], is
//!   believed only from the connected Audio holder, and the holder says
//!   `None` when it is over. Names and ids that are too long are not believed.

use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::wire::Frame;
use super::{lock, SyncNode};
use crate::roles::Role;
use crate::team::DeviceKey;

/// The longest sound name or item id believed, in bytes.
pub const MAX_NAME: usize = 256;
pub const MAX_ITEM: usize = 64;
/// The longest fade believed (ten minutes), in milliseconds.
pub const MAX_FADE_MS: u32 = 600_000;

/// What the Audio holder is asked to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SoundCommand {
    /// Play this file from the start, replacing what plays.
    Play {
        file: [u8; 32],
        name: String,
        item: String,
    },
    Pause,
    Resume,
    /// Fade out over this long and stop.
    Stop {
        fade_ms: u32,
    },
    /// Silence at once.
    Panic,
}

impl SoundCommand {
    /// Whether the fields are sane enough to act on.
    fn believable(&self) -> bool {
        match self {
            SoundCommand::Play { name, item, .. } => {
                name.len() <= MAX_NAME && item.len() <= MAX_ITEM
            }
            SoundCommand::Stop { fade_ms } => *fade_ms <= MAX_FADE_MS,
            _ => true,
        }
    }
}

/// What is playing on the Audio holder, as it last said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoundNow {
    /// The item it belongs to ("" when it is not from an item).
    pub item: String,
    pub name: String,
    pub paused: bool,
    /// A fade out is running.
    pub ending: bool,
    /// Where it was when this was said.
    pub position_ms: u32,
    pub length_ms: u32,
}

impl SoundNow {
    fn believable(&self) -> bool {
        self.name.len() <= MAX_NAME && self.item.len() <= MAX_ITEM
    }
}

/// Why a sound did not play, from a fixed list: the receiving screen has the
/// words, so nothing here can carry a path or any text of the sender's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SoundReason {
    /// The file is not on the Audio holder yet.
    NotOnDevice,
    /// A kind of file that cannot be played.
    Unsupported,
    /// The file could not be read, or is damaged.
    Unreadable,
    /// The sound output stopped or could not be opened.
    OutputDown,
    /// The Audio holder has no sound output at all.
    NoOutput,
}

/// A sound the Audio holder could not play, and why. Sent with
/// `Frame::SoundProblem` (protocol 7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoundProblem {
    pub reason: SoundReason,
    /// The item it belongs to ("" when it is not from an item, or none was
    /// playing).
    pub item: String,
    pub name: String,
}

impl SoundProblem {
    fn believable(&self) -> bool {
        self.name.len() <= MAX_NAME && self.item.len() <= MAX_ITEM
    }
}

/// What the app does with a cue made here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundRoute {
    /// This device is the Audio holder (or has nobody to share with): play
    /// it here.
    Local,
    /// Sent to the device that holds Audio.
    Sent,
    /// Another device holds Audio but is not connected: nothing was sent and
    /// nothing is kept.
    Away,
    /// Nobody holds Audio, so this sound plays nowhere.
    Nobody,
    /// This device may not start or control this sound (it holds neither
    /// Projection nor Operator).
    NotAllowed,
}

/// What the app does with a cue that arrived on the Audio holder. Called on a
/// sync task: it must not block.
pub type SoundHook = Arc<dyn Fn(SoundCommand) + Send + Sync>;

#[derive(Default)]
pub(super) struct SoundState {
    hook: Option<SoundHook>,
    /// What this device, as the Audio holder, last said it was playing.
    mine: Option<SoundNow>,
    /// What the Audio holder said, with who said it and when it arrived.
    theirs: Option<(DeviceKey, SoundNow, Instant)>,
    /// What this device, as the Audio holder, last said went wrong.
    problem_mine: Option<SoundProblem>,
    /// What the Audio holder said went wrong, with who said it.
    problem_theirs: Option<(DeviceKey, SoundProblem)>,
}

impl SyncNode {
    /// Where the app wants a sound cue that arrives here (this device holds
    /// Audio) to go. Set once at start.
    pub fn set_sound_hook(&self, hook: SoundHook) {
        lock(&self.sound).hook = Some(hook);
    }

    /// The device that holds Audio when it is not this one and this device
    /// has paired devices.
    pub fn audio_elsewhere(&self) -> Option<(DeviceKey, String)> {
        let has_peers = !lock(&self.trust).devices().is_empty();
        let r = lock(&self.roles);
        let record = r.record(Role::Audio);
        let holder = record.holder.filter(|h| has_peers && *h != self.device())?;
        Some((holder, record.name.clone()))
    }

    /// Whether this device plays the sound now: it holds Audio, or it has
    /// nobody to share with.
    pub fn audio_here(&self) -> bool {
        let has_peers = !lock(&self.trust).devices().is_empty();
        !has_peers || lock(&self.roles).holds(Role::Audio)
    }

    /// A sound cue was made on this device: says where it goes, and sends it
    /// when it goes to another device. See [`SoundRoute`].
    pub fn route_sound_cue(&self, cmd: SoundCommand) -> SoundRoute {
        if self.audio_here() {
            return SoundRoute::Local;
        }
        let Some((target, _)) = self.audio_elsewhere() else {
            return SoundRoute::Nobody;
        };
        let may = match cmd {
            SoundCommand::Panic => true,
            _ => {
                let r = lock(&self.roles);
                r.holds(Role::Projection) || r.holds(Role::Operator)
            }
        };
        if !may {
            return SoundRoute::NotAllowed;
        }
        if !cmd.believable() {
            eprintln!("lampstand: a sound cue with an unreasonable name or fade is not sent");
            return SoundRoute::NotAllowed;
        }
        if !self.connected(target) {
            return SoundRoute::Away;
        }
        self.send_to(target, Frame::SoundCue { cmd });
        SoundRoute::Sent
    }

    /// On the Audio holder: a device sent a cue.
    pub(crate) fn sound_cue_received(&self, from: DeviceKey, cmd: SoundCommand) {
        let has_peers = !lock(&self.trust).devices().is_empty();
        let allowed = has_peers && lock(&self.trust).is_paired(from) && {
            let r = lock(&self.roles);
            r.holds(Role::Audio)
                && (matches!(cmd, SoundCommand::Panic)
                    || r.record(Role::Projection).holder == Some(from)
                    || r.record(Role::Operator).holder == Some(from))
        };
        if !allowed {
            eprintln!(
                "lampstand: ignored a sound cue from {}: it may not start or control the sound, or Audio is not held here",
                self.paired_name(from)
            );
            return;
        }
        if !cmd.believable() {
            eprintln!("lampstand: ignored a sound cue with an unreasonable name or fade");
            return;
        }
        let hook = lock(&self.sound).hook.clone();
        match hook {
            Some(hook) => hook(cmd),
            None => eprintln!("lampstand: a sound cue arrived before anything was ready for it"),
        }
    }

    /// On the Audio holder: this is what is playing now (`None`: nothing).
    /// Tells every connected device, and each device that connects later.
    pub fn report_sound_now(&self, now: Option<SoundNow>) {
        {
            let mut s = lock(&self.sound);
            if s.mine == now {
                return;
            }
            s.mine = now.clone();
        }
        let has_peers = !lock(&self.trust).devices().is_empty();
        if !has_peers || !lock(&self.roles).holds(Role::Audio) {
            return;
        }
        let frame = Frame::SoundNow { now };
        for s in lock(&self.peers).sessions.values() {
            let _ = s.control.send(super::Control::Frame(frame.clone()));
        }
    }

    /// What the Audio holder, another device, last said it was playing, with
    /// its name and when that arrived. `None` when this device holds Audio,
    /// when nobody does, when the holder is not connected, or when it is
    /// playing nothing.
    pub fn sound_elsewhere(&self) -> Option<(String, SoundNow, Instant)> {
        let (holder, name) = self.audio_elsewhere()?;
        if !self.connected(holder) {
            return None;
        }
        match &lock(&self.sound).theirs {
            Some((from, now, at)) if *from == holder => Some((name, now.clone(), *at)),
            _ => None,
        }
    }

    /// A connection to `peer` opened: tell it what is playing here, and what
    /// went wrong.
    pub(super) fn sound_session_opened(&self, peer: DeviceKey) {
        let (mine, problem) = {
            let mut s = lock(&self.sound);
            // What `peer` said before is not believed across a reconnection:
            // it says again if it is still so.
            if s.problem_theirs
                .as_ref()
                .is_some_and(|(from, _)| *from == peer)
            {
                s.problem_theirs = None;
            }
            (s.mine.clone(), s.problem_mine.clone())
        };
        if !lock(&self.roles).holds(Role::Audio) {
            return;
        }
        if mine.is_some() {
            self.send_to(peer, Frame::SoundNow { now: mine });
        }
        if problem.is_some() {
            self.send_to(peer, Frame::SoundProblem { problem });
        }
    }

    /// On the Audio holder: this is what went wrong with the sound (`None`:
    /// nothing now). Tells every connected device, and each device that
    /// connects later.
    pub fn report_sound_problem(&self, problem: Option<SoundProblem>) {
        {
            let mut s = lock(&self.sound);
            if s.problem_mine == problem {
                return;
            }
            s.problem_mine = problem.clone();
        }
        let has_peers = !lock(&self.trust).devices().is_empty();
        if !has_peers || !lock(&self.roles).holds(Role::Audio) {
            return;
        }
        let frame = Frame::SoundProblem { problem };
        for s in lock(&self.peers).sessions.values() {
            let _ = s.control.send(super::Control::Frame(frame.clone()));
        }
    }

    /// What the Audio holder, another device, last said went wrong, with its
    /// name. `None` when this device holds Audio, when nobody does, when the
    /// holder is not connected, or when nothing is wrong.
    pub fn sound_problem_elsewhere(&self) -> Option<(String, SoundProblem)> {
        let (holder, name) = self.audio_elsewhere()?;
        if !self.connected(holder) {
            return None;
        }
        match &lock(&self.sound).problem_theirs {
            Some((from, problem)) if *from == holder => Some((name, problem.clone())),
            _ => None,
        }
    }

    /// A device said why a sound did not play. Believed only from the device
    /// that holds Audio.
    pub(crate) fn sound_problem_received(&self, from: DeviceKey, problem: Option<SoundProblem>) {
        let holds = self.audio_elsewhere().map(|(h, _)| h) == Some(from);
        if !holds || !lock(&self.trust).is_paired(from) {
            return;
        }
        if problem.as_ref().is_some_and(|p| !p.believable()) {
            return;
        }
        lock(&self.sound).problem_theirs = problem.map(|p| (from, p));
        self.bump();
    }

    /// A device said what it is playing. Believed only from the device that
    /// holds Audio.
    pub(crate) fn sound_now_received(&self, from: DeviceKey, now: Option<SoundNow>) {
        let holds = self.audio_elsewhere().map(|(h, _)| h) == Some(from);
        if !holds || !lock(&self.trust).is_paired(from) {
            return;
        }
        if now.as_ref().is_some_and(|n| !n.believable()) {
            return;
        }
        lock(&self.sound).theirs = now.map(|n| (from, n, Instant::now()));
        self.bump();
    }
}
