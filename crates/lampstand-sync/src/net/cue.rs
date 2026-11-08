//! Scene cues: the Projection holder tells the Stream holder which OBS scene
//! the item that just went live asks for (decision 0010, point 6).
//!
//! - **Where the cue goes.** While another device holds Stream, Projection
//!   never switches OBS itself: [`SyncNode::route_scene_cue`] keeps the cue
//!   and sends it to that device. With nobody holding Stream, or with this
//!   device holding it, the app applies the cue to its own OBS as it always
//!   did.
//! - **A cue is a scene name and nothing else.** No OBS address, password,
//!   file path or device name crosses the wire. A name longer than
//!   [`MAX_SCENE`] bytes is not believed.
//! - **Who may send one.** Only the device that holds Projection, as the
//!   receiving device sees it, and only to the device that holds Stream. From
//!   anyone else (a paired device with no role, a device that held Projection
//!   once, or a device that holds nothing) a cue is ignored, and so is one
//!   that arrives where Stream is not held by this device.
//! - **Waiting.** The newest cue stays on the Projection holder until the
//!   Stream holder says it has it (`ObsCueAck`), and is sent again whenever a
//!   session to that holder opens. A newer cue replaces an older one, so a
//!   holder that returns after several items gets only the latest. A cue kept
//!   for one holder is dropped if Stream moves to another device: passing
//!   Stream leaves OBS on its current scene.
//! - **Stale and repeated cues.** Each cue carries the sender's `run` (random,
//!   new on every start) and a `seq` that grows within a run. The receiver
//!   applies a cue only if it is newer than the last one it took from that
//!   sender, and answers one it has already applied with the ack again but
//!   does not apply it twice. (Frames cannot be replayed from outside: each
//!   session's frames are sealed under keys and a counter.)

use std::sync::Arc;

use super::wire::Frame;
use super::{lock, SyncNode};
use crate::roles::Role;
use crate::team::DeviceKey;

/// The longest scene name believed, in bytes. OBS allows long names, but a
/// frame this size is not a scene.
pub const MAX_SCENE: usize = 1024;

/// What the app does with a cue that arrived on the Stream holder. Called on
/// a sync task: it must not block.
pub type CueHook = Arc<dyn Fn(Option<String>) + Send + Sync>;

/// What to do with a scene cue made on the Projection holder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CueRoute {
    /// This device does the OBS duty itself: apply the cue to its own OBS.
    Local,
    /// Another device holds Stream: the cue is kept for it and sent when it
    /// is connected. Do not touch OBS here.
    Sent,
    /// Neither: this device does not hold Projection and Stream is held
    /// elsewhere, so the cue is not this device's to give.
    Dropped,
}

/// A cue kept until the Stream holder has it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pending {
    /// The Stream holder it is kept for.
    target: DeviceKey,
    seq: u64,
    scene: Option<String>,
}

#[derive(Default)]
pub(super) struct CueState {
    /// Random per start, so a restarted sender's cues are not taken for old
    /// ones.
    pub(super) run: u64,
    seq: u64,
    pending: Option<Pending>,
    /// The newest cue taken, with the device it came from.
    taken: Option<(DeviceKey, u64, u64)>,
    hook: Option<CueHook>,
}

impl CueState {
    pub(super) fn new() -> Self {
        Self {
            run: u64::from_le_bytes(crate::repo::random_bytes::<8>()),
            ..Self::default()
        }
    }
}

impl SyncNode {
    /// Where the app wants a cue that arrives here (this device holds
    /// Stream) to go. Set once at start.
    pub fn set_scene_cue_hook(&self, hook: CueHook) {
        lock(&self.cue).hook = Some(hook);
    }

    /// The device that holds Stream, when it is not this one and this device
    /// has paired devices: the one that owns the OBS connection instead of
    /// this one.
    pub fn stream_elsewhere(&self) -> Option<(DeviceKey, String)> {
        let has_peers = !lock(&self.trust).devices().is_empty();
        let r = lock(&self.roles);
        let record = r.record(Role::Stream);
        let holder = record.holder.filter(|h| has_peers && *h != self.device())?;
        Some((holder, record.name.clone()))
    }

    /// Whether this device owns the OBS connection now: it holds Stream (a
    /// device with nobody paired holds everything), or nobody holds Stream
    /// and it holds Projection, as in 0.7.0.
    pub fn obs_here(&self) -> bool {
        let has_peers = !lock(&self.trust).devices().is_empty();
        if !has_peers {
            return true;
        }
        let r = lock(&self.roles);
        r.holds(Role::Stream)
            || (r.record(Role::Stream).holder.is_none() && r.holds(Role::Projection))
    }

    /// An item went live on this device with `scene` (or none). Decides who
    /// switches OBS; see [`CueRoute`].
    pub fn route_scene_cue(&self, scene: Option<String>) -> CueRoute {
        // A name the other side would not believe is not sent, and not kept
        // to be sent again: the item is still a new item, without a scene.
        let scene = match scene {
            Some(s) if s.len() > MAX_SCENE && !self.obs_here() => {
                eprintln!(
                    "lampstand: an OBS scene name of {} bytes is too long to send; the item goes live without a scene",
                    s.len()
                );
                None
            }
            other => other,
        };
        if self.obs_here() {
            lock(&self.cue).pending = None;
            return CueRoute::Local;
        }
        let Some((target, _)) = self.stream_elsewhere() else {
            return CueRoute::Dropped;
        };
        if !lock(&self.roles).holds(Role::Projection) {
            return CueRoute::Dropped;
        }
        {
            let mut cue = lock(&self.cue);
            cue.seq += 1;
            let seq = cue.seq;
            cue.pending = Some(Pending { target, seq, scene });
        }
        self.flush_scene_cue();
        CueRoute::Sent
    }

    /// Whether a cue is kept for the Stream holder, which has not got it
    /// yet (it is offline, or has not answered).
    pub fn scene_cue_waiting(&self) -> bool {
        lock(&self.cue).pending.is_some()
    }

    /// Sends the cue kept for the Stream holder if it is connected and still
    /// the holder; drops one kept for a device that no longer holds Stream,
    /// or kept by a device that no longer holds Projection.
    pub(super) fn flush_scene_cue(&self) {
        let (pending, run) = {
            let cue = lock(&self.cue);
            (cue.pending.clone(), cue.run)
        };
        let Some(pending) = pending else { return };
        let still = self.stream_elsewhere().map(|(h, _)| h) == Some(pending.target)
            && lock(&self.roles).holds(Role::Projection);
        if !still {
            let mut cue = lock(&self.cue);
            if cue.pending.as_ref() == Some(&pending) {
                cue.pending = None;
            }
            return;
        }
        if self.connected(pending.target) {
            self.send_to(
                pending.target,
                Frame::ObsCue {
                    run,
                    seq: pending.seq,
                    scene: pending.scene,
                },
            );
        }
    }

    /// On the Stream holder: a device sent a cue.
    pub(crate) fn scene_cue_received(
        &self,
        from: DeviceKey,
        run: u64,
        seq: u64,
        scene: Option<String>,
    ) {
        let has_peers = !lock(&self.trust).devices().is_empty();
        let allowed = has_peers && lock(&self.trust).is_paired(from) && {
            let r = lock(&self.roles);
            r.holds(Role::Stream) && r.record(Role::Projection).holder == Some(from)
        };
        if !allowed {
            eprintln!(
                "lampstand: ignored a scene cue from {}: it is not the Projection holder, or Stream is not held here",
                self.paired_name(from)
            );
            return;
        }
        if scene.as_ref().is_some_and(|s| s.len() > MAX_SCENE) {
            eprintln!("lampstand: ignored a scene cue with an unreasonable name");
            return;
        }
        let hook = {
            let mut cue = lock(&self.cue);
            // Nothing to hand the cue to yet (the app has not said where
            // cues go): do not take it or answer, so the sender keeps it and
            // sends it again at the next connection.
            if cue.hook.is_none() {
                eprintln!("lampstand: a scene cue arrived before anything was ready for it");
                return;
            }
            let fresh = match cue.taken {
                Some((device, r, s)) if device == from && r == run => seq > s,
                _ => true,
            };
            if fresh {
                cue.taken = Some((from, run, seq));
                cue.hook.clone()
            } else {
                None
            }
        };
        // Say it has been taken even if it was a repeat: the first ack may
        // have been lost with a connection.
        self.send_to(from, Frame::ObsCueAck { run, seq });
        if let Some(hook) = hook {
            hook(scene);
        }
    }

    /// On the Projection holder: the Stream holder has the cue.
    pub(crate) fn scene_cue_ack_received(&self, from: DeviceKey, run: u64, seq: u64) {
        let mut cue = lock(&self.cue);
        let theirs = cue.run == run
            && cue
                .pending
                .as_ref()
                .is_some_and(|p| p.target == from && p.seq <= seq);
        if theirs {
            cue.pending = None;
        }
    }
}
