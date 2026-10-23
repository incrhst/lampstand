//! Who may present: one paired device at a time holds the *presenter
//! lease* for a library. It is the device that drives the screens and takes
//! a service live. Every device still holds and edits a full copy of the
//! library and merges peer to peer; only presenting is single-holder.
//! docs/decisions/0008-presenter-lease.md has the reasoning.
//!
//! The lease is a [`Lease`]: the holder's device key and name and an epoch.
//! Leases merge as a maximum register ordered by `(epoch, holder)`, so
//! devices that claim at the same moment agree on one winner without
//! talking, and a lease never goes back to an older holder. This module is
//! the pure rules; `net` carries the lease between devices and supplies
//! what it knows of the holder's connection as a [`Holder`].
//!
//! - A device with no paired devices is always its own presenter.
//! - Another device becomes presenter by *asking* ([`Presenter::ask`]); the
//!   presenter answers [`Presenter::allow`] or [`Presenter::decline`].
//! - It may *take over* without asking only when the holder has been
//!   offline for [`Presenter::takeover_after`] (default two minutes), or
//!   when nobody holds the lease (a library paired before leases existed,
//!   or the holder was unpaired).
//! - A holder that comes back finds a newer lease and becomes Planning,
//!   with a [`Notice`]. It never resumes by itself.
//!
//! Teams (T18) will gate who may hold the lease by [`crate::team::Role`]:
//! [`Presenter::claim`] and [`Presenter::allow`] are the two places that
//! grant it.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::team::DeviceKey;

/// How long the presenter must be offline before another device may take
/// over without asking.
pub const DEFAULT_TAKEOVER: Duration = Duration::from_secs(120);

/// The shortest time that can be set: a brief network blip must never
/// hand the screens to another device.
pub const MIN_TAKEOVER: Duration = Duration::from_secs(1);

/// The longest time that can be set.
pub const MAX_TAKEOVER: Duration = Duration::from_secs(24 * 3600);

/// The right to present, held by one device.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lease {
    /// `None` until a device first claims.
    #[serde(with = "opt_hex_key")]
    pub holder: Option<DeviceKey>,
    /// The holder's name when it claimed: "Sanctuary".
    pub name: String,
    /// Starts at 0 (unclaimed); every claim or transfer adds one.
    pub epoch: u64,
}

impl Lease {
    /// Whether this lease replaces `other` when they meet: the higher
    /// epoch, and for claims made at the same moment the higher device key.
    pub fn beats(&self, other: &Lease) -> bool {
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

/// What this device may do on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Drives the screens and may take a service live.
    Presenter,
    /// Edits and syncs freely; its screens stay dark.
    Planning,
}

/// What the node knows of the lease holder's connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    /// This device holds the lease.
    Me,
    /// Nobody holds it, or the holder is no longer paired with this device.
    Gone,
    /// Connected now.
    Online,
    /// Not connected, and not heard from for this long.
    Offline(Duration),
}

/// How this device could become the presenter right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// It is the presenter (or has nobody to share with).
    Presenting,
    /// Nobody holds the lease: present with one click, no permission.
    Open,
    /// The presenter is connected: ask it.
    Ask,
    /// The presenter has been offline long enough to take over.
    TakeOver { offline: Duration },
    /// The presenter is offline but may be back: not yet.
    Wait {
        offline: Duration,
        remaining: Duration,
    },
}

/// Why a claim or ask was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denied {
    /// The presenter is connected: ask it instead.
    MustAsk,
    /// The presenter is offline but only for `offline`; `remaining` to go.
    TooSoon {
        offline: Duration,
        remaining: Duration,
    },
    /// Already the presenter.
    Already,
    /// The presenter is offline, so there is nobody to ask.
    NobodyToAsk,
    /// Nothing to ask: take the role directly.
    NotNeeded,
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
        }
    }
}

/// A device asking to present, as the presenter sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub device: DeviceKey,
    pub name: String,
}

/// This device's own ask, as the requester sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asking {
    /// The lease epoch it was asked at.
    pub epoch: u64,
    /// The presenter said not now.
    pub declined: bool,
}

/// Something the person should be told once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// This device was presenting and another device now holds the lease.
    Replaced { by: String },
}

/// What a [`Presenter::merge`] changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Merged {
    pub changed: bool,
    pub lost_role: bool,
}

/// The lease and the asking around it, for one device.
#[derive(Debug, Clone)]
pub struct Presenter {
    me: DeviceKey,
    name: String,
    lease: Lease,
    takeover_after: Duration,
    requests: Vec<Request>,
    asking: Option<Asking>,
    notice: Option<Notice>,
}

impl Presenter {
    pub fn new(me: DeviceKey, name: String, lease: Lease, takeover_after: Duration) -> Self {
        Self {
            me,
            name,
            lease,
            takeover_after: takeover_after.clamp(MIN_TAKEOVER, MAX_TAKEOVER),
            requests: Vec::new(),
            asking: None,
            notice: None,
        }
    }

    pub fn lease(&self) -> &Lease {
        &self.lease
    }

    pub fn takeover_after(&self) -> Duration {
        self.takeover_after
    }

    pub fn set_takeover_after(&mut self, after: Duration) {
        self.takeover_after = after.clamp(MIN_TAKEOVER, MAX_TAKEOVER);
    }

    pub fn requests(&self) -> &[Request] {
        &self.requests
    }

    pub fn asking(&self) -> Option<Asking> {
        self.asking
    }

    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    pub fn dismiss_notice(&mut self) {
        self.notice = None;
    }

    pub fn holds(&self) -> bool {
        self.lease.holder == Some(self.me)
    }

    /// What the node should report for the lease holder here.
    pub fn holder_of(&self, other: impl Fn(DeviceKey) -> Holder) -> Holder {
        match self.lease.holder {
            None => Holder::Gone,
            Some(h) if h == self.me => Holder::Me,
            Some(h) => other(h),
        }
    }

    /// `has_peers`: at least one device is paired with this one.
    pub fn role(&self, has_peers: bool) -> Role {
        if !has_peers || self.holds() {
            Role::Presenter
        } else {
            Role::Planning
        }
    }

    pub fn standing(&self, has_peers: bool, holder: Holder) -> Standing {
        if self.role(has_peers) == Role::Presenter {
            return Standing::Presenting;
        }
        match holder {
            Holder::Me => Standing::Presenting,
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

    /// Takes the lease without anyone's permission, when the rules allow
    /// it: nobody holds it, or the holder has been offline long enough.
    /// Returns the new lease, to send to every device.
    pub fn claim(&mut self, has_peers: bool, holder: Holder) -> Result<Lease, Denied> {
        match self.standing(has_peers, holder) {
            Standing::Presenting if self.holds() => return Err(Denied::Already),
            // Alone: record the claim anyway, so a device that pairs later
            // starts out as the presenter.
            Standing::Presenting | Standing::Open | Standing::TakeOver { .. } => {}
            Standing::Ask => return Err(Denied::MustAsk),
            Standing::Wait { offline, remaining } => {
                return Err(Denied::TooSoon { offline, remaining })
            }
        }
        Ok(self.take())
    }

    fn take(&mut self) -> Lease {
        self.lease = Lease {
            holder: Some(self.me),
            name: self.name.clone(),
            epoch: self.lease.epoch + 1,
        };
        self.asking = None;
        self.requests.clear();
        self.lease.clone()
    }

    /// A device that has just paired and finds nobody presenting becomes
    /// the presenter: the one that showed the pairing code is the
    /// presenting machine. Returns the lease if it claimed.
    pub fn claim_if_unclaimed(&mut self) -> Option<Lease> {
        (self.lease.holder.is_none()).then(|| self.take())
    }

    /// Asks the presenter to hand over. Only while it is connected.
    pub fn ask(&mut self, has_peers: bool, holder: Holder) -> Result<u64, Denied> {
        match self.standing(has_peers, holder) {
            Standing::Presenting => Err(Denied::Already),
            Standing::Open | Standing::TakeOver { .. } => Err(Denied::NotNeeded),
            Standing::Wait { .. } => Err(Denied::NobodyToAsk),
            Standing::Ask => {
                self.asking = Some(Asking {
                    epoch: self.lease.epoch,
                    declined: false,
                });
                Ok(self.lease.epoch)
            }
        }
    }

    /// Withdraws or dismisses this device's ask. True if one was open and
    /// not yet answered (the presenter should be told).
    pub fn cancel_ask(&mut self) -> bool {
        self.asking.take().is_some_and(|a| !a.declined)
    }

    /// A device asks this one to hand over. Ignored unless this device
    /// holds the lease. True if it is new.
    pub fn receive_ask(&mut self, device: DeviceKey, name: String) -> bool {
        if !self.holds() || device == self.me || self.requests.iter().any(|r| r.device == device) {
            return false;
        }
        self.requests.push(Request { device, name });
        true
    }

    /// The asking device withdrew.
    pub fn receive_cancel(&mut self, device: DeviceKey) -> bool {
        let before = self.requests.len();
        self.requests.retain(|r| r.device != device);
        self.requests.len() != before
    }

    /// Hands the lease to a device that asked. Returns the new lease, to
    /// send to every device; this device becomes Planning. `None` if that
    /// device is not asking (or this device does not hold the lease).
    pub fn allow(&mut self, device: DeviceKey) -> Option<Lease> {
        if !self.holds() {
            return None;
        }
        let request = self.requests.iter().find(|r| r.device == device)?.clone();
        self.lease = Lease {
            holder: Some(request.device),
            name: request.name,
            epoch: self.lease.epoch + 1,
        };
        self.requests.clear();
        Some(self.lease.clone())
    }

    /// Says not now to a device that asked. True if it was asking.
    pub fn decline(&mut self, device: DeviceKey) -> bool {
        let before = self.requests.len();
        self.requests.retain(|r| r.device != device);
        self.requests.len() != before
    }

    /// The presenter declined this device's ask.
    pub fn receive_declined(&mut self, from: DeviceKey, epoch: u64) {
        if self.lease.holder == Some(from) && self.lease.epoch == epoch {
            if let Some(a) = self.asking.as_mut() {
                a.declined = true;
            }
        }
    }

    /// Takes in a lease from another device: the newer one wins. `known`:
    /// the new holder is a device paired with this one (a stranger's lease
    /// is not worth a notice).
    pub fn merge(&mut self, remote: &Lease, known: bool) -> Merged {
        if !remote.beats(&self.lease) {
            return Merged::default();
        }
        let was_holder = self.holds();
        self.lease = remote.clone();
        self.requests.clear();
        let lost_role = was_holder && !self.holds();
        if lost_role && known {
            self.notice = Some(Notice::Replaced {
                by: remote.name.clone(),
            });
        }
        // An ask is settled once the lease has moved on, whoever got it.
        if self.asking.is_some_and(|a| a.epoch < remote.epoch) {
            self.asking = None;
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

    fn device(n: u8, name: &str) -> Presenter {
        Presenter::new(key(n), name.into(), Lease::default(), DEFAULT_TAKEOVER)
    }

    /// Two devices that agree `a` presents.
    fn pair() -> (Presenter, Presenter) {
        let mut a = device(1, "Sanctuary");
        let mut b = device(2, "Office PC");
        let lease = a.claim_if_unclaimed().unwrap();
        assert!(b.merge(&lease, true).changed);
        (a, b)
    }

    #[test]
    fn a_lone_device_is_its_own_presenter() {
        let mut p = device(1, "Sanctuary");
        assert_eq!(p.role(false), Role::Presenter);
        assert_eq!(p.standing(false, Holder::Gone), Standing::Presenting);
        // Even when an old lease names another device.
        p.merge(
            &Lease {
                holder: Some(key(9)),
                name: "Gone".into(),
                epoch: 4,
            },
            false,
        );
        assert_eq!(p.role(false), Role::Presenter);
    }

    #[test]
    fn the_device_that_shows_the_pairing_code_starts_as_presenter() {
        let (a, b) = pair();
        assert_eq!(a.role(true), Role::Presenter);
        assert_eq!(b.role(true), Role::Planning);
        assert_eq!(b.lease().name, "Sanctuary");
        assert_eq!(b.lease().epoch, 1);
    }

    #[test]
    fn a_planning_device_must_ask_while_the_presenter_is_connected() {
        let (_, mut b) = pair();
        assert_eq!(b.standing(true, Holder::Online), Standing::Ask);
        assert_eq!(b.claim(true, Holder::Online), Err(Denied::MustAsk));
        assert_eq!(b.ask(true, Holder::Online), Ok(1));
        assert_eq!(
            b.asking(),
            Some(Asking {
                epoch: 1,
                declined: false
            })
        );
        assert_eq!(b.role(true), Role::Planning);
    }

    #[test]
    fn allowing_hands_the_role_over() {
        let (mut a, mut b) = pair();
        b.ask(true, Holder::Online).unwrap();
        assert!(a.receive_ask(key(2), "Office PC".into()));
        assert_eq!(a.requests().len(), 1);
        let lease = a.allow(key(2)).unwrap();
        assert_eq!(lease.epoch, 2);
        assert_eq!(a.role(true), Role::Planning);
        let merged = b.merge(&lease, true);
        assert!(merged.changed && !merged.lost_role);
        assert_eq!(b.role(true), Role::Presenter);
        assert_eq!(b.asking(), None, "the ask is settled");
        // The old presenter learns of it from its own allow, and from
        // anyone else's copy, without a notice (it chose this).
        assert!(!a.merge(&lease, true).changed);
        assert_eq!(a.notice(), None);
    }

    #[test]
    fn declining_tells_the_requester_and_keeps_the_role() {
        let (mut a, mut b) = pair();
        b.ask(true, Holder::Online).unwrap();
        a.receive_ask(key(2), "Office PC".into());
        assert!(a.decline(key(2)));
        assert!(a.requests().is_empty());
        assert_eq!(a.role(true), Role::Presenter);
        b.receive_declined(key(1), 1);
        assert_eq!(
            b.asking(),
            Some(Asking {
                epoch: 1,
                declined: true
            })
        );
        // It can ask again.
        assert_eq!(b.ask(true, Holder::Online), Ok(1));
        assert!(!b.asking().unwrap().declined);
    }

    #[test]
    fn only_the_presenter_hears_asks_and_only_from_askers() {
        let (mut a, mut b) = pair();
        assert!(
            !b.receive_ask(key(3), "Laptop".into()),
            "b is not presenting"
        );
        assert!(a.allow(key(3)).is_none(), "nobody asked");
        assert!(a.receive_ask(key(2), "Office PC".into()));
        assert!(!a.receive_ask(key(2), "Office PC".into()), "once");
        assert!(a.receive_cancel(key(2)));
        assert!(a.allow(key(2)).is_none(), "withdrawn");
    }

    #[test]
    fn concurrent_claims_resolve_to_the_higher_device_key() {
        let (mut a, b) = pair();
        // The presenter vanished; both take over at the same moment.
        let gone = Holder::Offline(secs(300));
        assert_eq!(a.claim(true, Holder::Me), Err(Denied::Already));
        let mut c = device(3, "Laptop");
        c.merge(a.lease(), true);
        let mut b2 = b.clone();
        let lb = b2.claim(true, gone).unwrap();
        let lc = c.claim(true, gone).unwrap();
        assert_eq!(lb.epoch, lc.epoch, "same epoch");
        // Merging in either order gives the same winner: key 3.
        let mut x = b2.clone();
        let mut y = c.clone();
        let mx = x.merge(&lc, true);
        let my = y.merge(&lb, true);
        assert!(mx.changed && mx.lost_role);
        assert!(!my.changed);
        assert_eq!(x.lease(), y.lease());
        assert_eq!(x.lease().holder, Some(key(3)));
        assert_eq!(
            x.notice(),
            Some(&Notice::Replaced {
                by: "Laptop".into()
            })
        );
        assert_eq!(y.role(true), Role::Presenter);
    }

    #[test]
    fn a_higher_epoch_beats_a_higher_key() {
        let low = Lease {
            holder: Some(key(1)),
            name: "A".into(),
            epoch: 3,
        };
        let high = Lease {
            holder: Some(key(9)),
            name: "B".into(),
            epoch: 2,
        };
        assert!(low.beats(&high));
        assert!(!high.beats(&low));
        assert!(!low.beats(&low));
    }

    #[test]
    fn takeover_only_after_the_timeout() {
        let (_, mut b) = pair();
        assert_eq!(
            b.standing(true, Holder::Offline(secs(30))),
            Standing::Wait {
                offline: secs(30),
                remaining: secs(90)
            }
        );
        assert_eq!(
            b.claim(true, Holder::Offline(secs(119))),
            Err(Denied::TooSoon {
                offline: secs(119),
                remaining: secs(1)
            })
        );
        assert_eq!(
            b.ask(true, Holder::Offline(secs(30))),
            Err(Denied::NobodyToAsk)
        );
        assert_eq!(
            b.role(true),
            Role::Planning,
            "a refused claim changes nothing"
        );
        assert_eq!(
            b.standing(true, Holder::Offline(secs(120))),
            Standing::TakeOver { offline: secs(120) }
        );
        assert_eq!(
            b.ask(true, Holder::Offline(secs(300))),
            Err(Denied::NotNeeded)
        );
        let lease = b.claim(true, Holder::Offline(secs(300))).unwrap();
        assert_eq!((lease.epoch, lease.holder), (2, Some(key(2))));
        assert_eq!(b.role(true), Role::Presenter);
    }

    #[test]
    fn the_timeout_is_configurable() {
        let (_, mut b) = pair();
        b.set_takeover_after(secs(600));
        assert!(b.claim(true, Holder::Offline(secs(300))).is_err());
        b.set_takeover_after(secs(60));
        assert!(b.claim(true, Holder::Offline(secs(61))).is_ok());
        b.set_takeover_after(Duration::ZERO);
        assert_eq!(b.takeover_after(), MIN_TAKEOVER);
    }

    #[test]
    fn a_returning_presenter_yields_and_is_told() {
        let (mut a, mut b) = pair();
        let lease = b.claim(true, Holder::Offline(secs(400))).unwrap();
        // `a` never knew: it still believes it presents until it hears.
        assert_eq!(a.role(true), Role::Presenter);
        let merged = a.merge(&lease, true);
        assert!(merged.lost_role);
        assert_eq!(a.role(true), Role::Planning);
        assert_eq!(
            a.notice(),
            Some(&Notice::Replaced {
                by: "Office PC".into()
            })
        );
        // Its own older lease can never win it back.
        assert!(
            !a.merge(
                &Lease {
                    holder: Some(key(1)),
                    name: "Sanctuary".into(),
                    epoch: 1
                },
                true
            )
            .changed
        );
        assert_eq!(a.role(true), Role::Planning);
        a.dismiss_notice();
        assert_eq!(a.notice(), None);
    }

    #[test]
    fn an_unclaimed_library_can_be_claimed_without_asking() {
        let mut b = device(2, "Office PC");
        assert_eq!(b.role(true), Role::Planning);
        assert_eq!(b.standing(true, Holder::Gone), Standing::Open);
        assert_eq!(b.ask(true, Holder::Gone), Err(Denied::NotNeeded));
        assert!(b.claim(true, Holder::Gone).is_ok());
        assert_eq!(b.role(true), Role::Presenter);
    }

    #[test]
    fn a_holder_that_was_unpaired_leaves_the_seat_open() {
        let (_, b) = pair();
        let holder = b.holder_of(|_| Holder::Gone);
        assert_eq!(holder, Holder::Gone);
        assert_eq!(b.standing(true, holder), Standing::Open);
    }
}
