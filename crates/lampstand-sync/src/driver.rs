//! Driving: letting a connected device operate the presenting device.
//!
//! The presenter lease ([`crate::presenter`]) says which device owns the
//! screens, the renderer, OBS and the media. This module is the lighter
//! thing beside it, the *driver*: while one device presents, one other
//! paired device may be its operator, sending the same commands the local
//! operator would and watching the same live state. Nothing about who
//! presents changes. docs/decisions/0009-driving-another-device.md has the
//! reasoning.
//!
//! The rules, all decided by the presenting device:
//!
//! - A device becomes the driver by *asking* ([`Seat::receive_ask`]); the
//!   presenter allows or declines, once, and may remember a device with
//!   *always allow* so later asks are answered at once.
//! - One driver at a time. A second asker is told who is driving
//!   ([`Answer::Busy`]) and is not queued.
//! - The presenter can take control back whenever it likes
//!   ([`Seat::take_back`]); its own keyboard and clicker never depend on
//!   this module at all.
//! - Commands count only from the active driver, and only while this
//!   device still holds the presenter lease ([`Seat::accepts`]).
//! - A driver that disconnects stops being the driver ([`Seat::gone`]); the
//!   presenter keeps showing what is on air.
//!
//! A [`Seat`] holds both halves, because any device can be either: the
//! presenter's side (who drives, who is asking, who is remembered) and the
//! driver's side (whether this device is driving, whether its ask was
//! answered). `net` carries the messages and supplies who holds the lease.

use std::collections::BTreeMap;

use crate::team::DeviceKey;

/// A device, as the prompts name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    pub device: DeviceKey,
    pub name: String,
}

/// The presenter's reply to an ask that was not simply allowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The person said not now.
    Declined,
    /// Another device is driving; `by` is its name.
    Busy { by: String },
}

/// What the presenter does with an ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// Show the prompt: allow or not now.
    Prompt,
    /// The device is remembered (or already driving): it drives now.
    Allowed { changed: bool },
    /// Answer with this and drive nothing.
    Refuse(Answer),
    /// This device is not presenting, so the ask means nothing here.
    Ignored,
}

/// Why the presenter could not allow a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotAllowed {
    /// This device is not presenting.
    NotPresenting,
    /// The device is not asking (it withdrew, or went offline).
    NotAsking,
    /// Another device is driving; take control back first.
    Busy { by: String },
}

impl std::fmt::Display for NotAllowed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotAllowed::NotPresenting => write!(f, "this device is not presenting"),
            NotAllowed::NotAsking => write!(f, "that device is no longer asking"),
            NotAllowed::Busy { by } => write!(f, "{by} is driving: take control back first"),
        }
    }
}

/// This device's own ask, as the asker sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asking {
    Waiting,
    Declined,
    Busy { by: String },
}

/// Something the person should be told once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// This device was driving and the presenter took control back.
    TakenBack { by: String },
}

/// The presenter this device is driving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Granted {
    pub presenter: DeviceKey,
    pub name: String,
}

/// Driving, for one device.
#[derive(Debug, Clone, Default)]
pub struct Seat {
    // The presenter's side.
    active: Option<Named>,
    requests: Vec<Named>,
    always: BTreeMap<DeviceKey, String>,
    ignored: u64,
    // The driver's side.
    granted: Option<Granted>,
    asking: Option<Asking>,
    asked_of: Option<DeviceKey>,
    notice: Option<Notice>,
    // Everyone's: who drives the presenter, as the presenter last said.
    known: Option<Named>,
    known_from: Option<DeviceKey>,
}

impl Seat {
    /// A seat that remembers these devices as always allowed.
    pub fn with_always(always: impl IntoIterator<Item = (DeviceKey, String)>) -> Self {
        Self {
            always: always.into_iter().collect(),
            ..Self::default()
        }
    }

    // ---- The presenter's side -----------------------------------------

    /// The device driving this one, when this one presents.
    pub fn active(&self) -> Option<&Named> {
        self.active.as_ref()
    }

    /// Devices waiting for an answer.
    pub fn requests(&self) -> &[Named] {
        &self.requests
    }

    /// Devices allowed without asking, with the name they had when
    /// remembered.
    pub fn always(&self) -> Vec<Named> {
        self.always
            .iter()
            .map(|(device, name)| Named {
                device: *device,
                name: name.clone(),
            })
            .collect()
    }

    /// How many commands came from a device that was not the driver.
    pub fn ignored(&self) -> u64 {
        self.ignored
    }

    /// A device asks to drive. `holds`: this device presents.
    pub fn receive_ask(&mut self, holds: bool, device: DeviceKey, name: String) -> Asked {
        if !holds {
            return Asked::Ignored;
        }
        if let Some(active) = &self.active {
            if active.device == device {
                return Asked::Allowed { changed: false };
            }
            return Asked::Refuse(Answer::Busy {
                by: active.name.clone(),
            });
        }
        if let Some(remembered) = self.always.get_mut(&device) {
            remembered.clone_from(&name);
            self.requests.retain(|r| r.device != device);
            self.active = Some(Named { device, name });
            return Asked::Allowed { changed: true };
        }
        if !self.requests.iter().any(|r| r.device == device) {
            self.requests.push(Named { device, name });
        }
        Asked::Prompt
    }

    /// The asking device withdrew. True if it had been asking.
    pub fn receive_cancel(&mut self, device: DeviceKey) -> bool {
        let before = self.requests.len();
        self.requests.retain(|r| r.device != device);
        self.requests.len() != before
    }

    /// Lets a device that asked drive; with `always`, remembers it. Returns
    /// the driver.
    pub fn allow(
        &mut self,
        holds: bool,
        device: DeviceKey,
        always: bool,
    ) -> Result<Named, NotAllowed> {
        if !holds {
            return Err(NotAllowed::NotPresenting);
        }
        if let Some(active) = &self.active {
            if active.device != device {
                return Err(NotAllowed::Busy {
                    by: active.name.clone(),
                });
            }
        }
        let Some(pos) = self.requests.iter().position(|r| r.device == device) else {
            return Err(NotAllowed::NotAsking);
        };
        let asker = self.requests.remove(pos);
        if always {
            self.always.insert(asker.device, asker.name.clone());
        }
        self.active = Some(asker.clone());
        Ok(asker)
    }

    /// Says not now to a device that asked. True if it was asking.
    pub fn decline(&mut self, device: DeviceKey) -> bool {
        self.receive_cancel(device)
    }

    /// The presenter takes control back. True if a device was driving.
    pub fn take_back(&mut self) -> bool {
        self.active.take().is_some()
    }

    /// A device went offline or stopped driving. True if the driver or an
    /// ask went with it.
    pub fn gone(&mut self, device: DeviceKey) -> bool {
        let asking = self.receive_cancel(device);
        let driving = self.active.as_ref().is_some_and(|a| a.device == device);
        if driving {
            self.active = None;
        }
        asking || driving
    }

    /// Forgets that a device is always allowed (or that it is paired).
    pub fn forget(&mut self, device: DeviceKey) -> bool {
        self.always.remove(&device).is_some()
    }

    /// Remembers a device as always allowed, or stops. True if that changed.
    pub fn set_always(&mut self, device: DeviceKey, name: String, on: bool) -> bool {
        if on {
            self.always.insert(device, name.clone()) != Some(name)
        } else {
            self.always.remove(&device).is_some()
        }
    }

    /// Whether a command from `from` is to be carried out: this device
    /// presents and `from` is the driver. Anything else is counted and
    /// ignored.
    pub fn accepts(&mut self, holds: bool, from: DeviceKey) -> bool {
        let ok = holds && self.active.as_ref().is_some_and(|a| a.device == from);
        if !ok {
            self.ignored += 1;
        }
        ok
    }

    // ---- The driver's side --------------------------------------------

    /// Whether this device is driving, and whom.
    pub fn granted(&self) -> Option<&Granted> {
        self.granted.as_ref()
    }

    pub fn asking(&self) -> Option<&Asking> {
        self.asking.as_ref()
    }

    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    pub fn dismiss_notice(&mut self) {
        self.notice = None;
    }

    /// Who drives the presenter, as the presenter last said.
    pub fn known(&self) -> Option<&Named> {
        self.known.as_ref()
    }

    /// This device asks the presenter to let it drive.
    pub fn begin_ask(&mut self, presenter: DeviceKey) {
        self.asking = Some(Asking::Waiting);
        self.asked_of = Some(presenter);
    }

    /// The presenter answered an ask with no.
    pub fn receive_answer(&mut self, answer: Answer) {
        if self.asking.is_some() {
            self.asking = Some(match answer {
                Answer::Declined => Asking::Declined,
                Answer::Busy { by } => Asking::Busy { by },
            });
        }
    }

    /// The presenter says who is driving it. `from` is the lease holder,
    /// `from_name` its name and `me` this device. True if anything changed.
    pub fn receive_state(
        &mut self,
        from: DeviceKey,
        from_name: &str,
        driver: Option<Named>,
        me: DeviceKey,
    ) -> bool {
        let before = (self.granted.clone(), self.known.clone(), self.asking.clone());
        let mine = driver.as_ref().is_some_and(|d| d.device == me);
        if mine {
            self.granted = Some(Granted {
                presenter: from,
                name: from_name.to_string(),
            });
            self.asking = None;
            self.notice = None;
        } else if let Some(was) = self.granted.take() {
            // Control moved away from this device.
            if was.presenter == from {
                self.notice = Some(Notice::TakenBack {
                    by: from_name.to_string(),
                });
            }
        }
        self.known = driver;
        self.known_from = Some(from);
        before != (self.granted.clone(), self.known.clone(), self.asking.clone())
    }

    /// This device stops driving, or withdraws its ask. True if it was
    /// either (the presenter should be told).
    pub fn stop(&mut self) -> bool {
        let driving = self.granted.take().is_some();
        let asking = self.asking.take().is_some();
        driving || asking
    }

    /// Clears a "not now" or "busy" answer.
    pub fn clear_answer(&mut self) {
        if matches!(
            self.asking,
            Some(Asking::Declined) | Some(Asking::Busy { .. })
        ) {
            self.asking = None;
        }
    }

    /// The presenter lease now names `holder` (`me` is this device). What
    /// was learned from or asked of any other presenter ends, and a device
    /// that no longer presents drops its driver and prompts. True if
    /// anything changed.
    pub fn lease_moved(&mut self, me: DeviceKey, holder: Option<DeviceKey>) -> bool {
        let mut changed = false;
        if self.granted.as_ref().is_some_and(|g| Some(g.presenter) != holder) {
            self.granted = None;
            changed = true;
        }
        if self.known_from != holder && self.known.take().is_some() {
            changed = true;
        }
        if self.asked_of != holder && self.asking.take().is_some() {
            changed = true;
        }
        if holder != Some(me) {
            if self.active.take().is_some() {
                changed = true;
            }
            if !self.requests.is_empty() {
                self.requests.clear();
                changed = true;
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u8) -> DeviceKey {
        DeviceKey([n; 32])
    }

    const PRESENTER: u8 = 1;
    const MARY: u8 = 2;
    const SAM: u8 = 3;

    fn ask(seat: &mut Seat, who: u8, name: &str) -> Asked {
        seat.receive_ask(true, key(who), name.into())
    }

    #[test]
    fn an_ask_is_a_prompt_until_the_presenter_answers() {
        let mut p = Seat::default();
        assert_eq!(ask(&mut p, MARY, "Mary's laptop"), Asked::Prompt);
        // Asking again does not stack a second prompt.
        assert_eq!(ask(&mut p, MARY, "Mary's laptop"), Asked::Prompt);
        assert_eq!(p.requests().len(), 1);
        assert_eq!(p.active(), None);
        let driver = p.allow(true, key(MARY), false).unwrap();
        assert_eq!(driver.name, "Mary's laptop");
        assert_eq!(p.active().map(|a| a.device), Some(key(MARY)));
        assert!(p.requests().is_empty());
        assert!(p.always().is_empty(), "allowed once, not remembered");
    }

    #[test]
    fn declining_clears_the_prompt_and_drives_nothing() {
        let mut p = Seat::default();
        ask(&mut p, MARY, "Mary's laptop");
        assert!(p.decline(key(MARY)));
        assert!(!p.decline(key(MARY)));
        assert_eq!(p.active(), None);
        assert_eq!(
            p.allow(true, key(MARY), false),
            Err(NotAllowed::NotAsking),
            "a declined ask cannot be allowed later"
        );
    }

    #[test]
    fn always_allow_is_remembered_and_answers_the_next_ask_at_once() {
        let mut p = Seat::default();
        ask(&mut p, MARY, "Mary's laptop");
        p.allow(true, key(MARY), true).unwrap();
        assert_eq!(p.always().len(), 1);
        // She stops; the next ask needs no prompt.
        assert!(p.gone(key(MARY)));
        assert_eq!(
            ask(&mut p, MARY, "Mary's MacBook"),
            Asked::Allowed { changed: true }
        );
        assert!(p.requests().is_empty());
        assert_eq!(p.active().map(|a| a.name.as_str()), Some("Mary's MacBook"));
        // Someone who was never remembered is still asked.
        assert_eq!(ask(&mut p, SAM, "Sam's phone"), Asked::Refuse(Answer::Busy { by: "Mary's MacBook".into() }));
        assert!(p.forget(key(MARY)));
        p.take_back();
        assert_eq!(ask(&mut p, MARY, "Mary's laptop"), Asked::Prompt);
    }

    #[test]
    fn a_second_driver_is_refused_and_told_who_drives() {
        let mut p = Seat::default();
        ask(&mut p, MARY, "Mary's laptop");
        p.allow(true, key(MARY), false).unwrap();
        assert_eq!(
            ask(&mut p, SAM, "Sam's tablet"),
            Asked::Refuse(Answer::Busy {
                by: "Mary's laptop".into()
            })
        );
        assert!(p.requests().is_empty(), "no queue");
        // Even a remembered device does not displace the driver.
        p.set_always(key(SAM), "Sam's tablet".into(), true);
        assert!(matches!(
            ask(&mut p, SAM, "Sam's tablet"),
            Asked::Refuse(Answer::Busy { .. })
        ));
        assert_eq!(p.active().map(|a| a.device), Some(key(MARY)));
        // A prompt shown before Mary was allowed cannot displace her either.
        let mut q = Seat::default();
        ask(&mut q, MARY, "Mary's laptop");
        ask(&mut q, SAM, "Sam's tablet");
        q.allow(true, key(MARY), false).unwrap();
        assert_eq!(
            q.allow(true, key(SAM), false),
            Err(NotAllowed::Busy {
                by: "Mary's laptop".into()
            })
        );
    }

    #[test]
    fn the_presenter_takes_control_back_and_commands_stop_counting() {
        let mut p = Seat::default();
        ask(&mut p, MARY, "Mary's laptop");
        p.allow(true, key(MARY), false).unwrap();
        assert!(p.accepts(true, key(MARY)));
        assert!(p.take_back());
        assert!(!p.take_back());
        assert!(!p.accepts(true, key(MARY)));
        assert_eq!(p.ignored(), 1);
    }

    #[test]
    fn commands_from_anyone_but_the_driver_are_ignored() {
        let mut p = Seat::default();
        assert!(!p.accepts(true, key(MARY)), "nobody drives yet");
        ask(&mut p, MARY, "Mary's laptop");
        assert!(!p.accepts(true, key(MARY)), "asking is not driving");
        p.allow(true, key(MARY), false).unwrap();
        assert!(!p.accepts(true, key(SAM)));
        assert!(!p.accepts(true, key(PRESENTER)));
        assert!(p.accepts(true, key(MARY)));
        assert_eq!(p.ignored(), 4);
        // A device that has lost the presenter lease carries out nothing.
        assert!(!p.accepts(false, key(MARY)));
    }

    #[test]
    fn a_device_that_is_not_presenting_ignores_asks_and_allows_nothing() {
        let mut p = Seat::default();
        assert_eq!(
            p.receive_ask(false, key(MARY), "Mary's laptop".into()),
            Asked::Ignored
        );
        assert_eq!(
            p.allow(false, key(MARY), false),
            Err(NotAllowed::NotPresenting)
        );
    }

    #[test]
    fn a_driver_that_goes_offline_hands_control_back() {
        let mut p = Seat::default();
        ask(&mut p, MARY, "Mary's laptop");
        p.allow(true, key(MARY), false).unwrap();
        assert!(p.gone(key(MARY)));
        assert_eq!(p.active(), None);
        assert!(!p.accepts(true, key(MARY)));
        // Somebody who was only asking is forgotten too.
        ask(&mut p, SAM, "Sam's tablet");
        assert!(p.gone(key(SAM)));
        assert!(p.requests().is_empty());
    }

    #[test]
    fn losing_the_presenter_lease_ends_driving_and_prompts() {
        let mut p = Seat::default();
        ask(&mut p, MARY, "Mary's laptop");
        ask(&mut p, SAM, "Sam's tablet");
        p.allow(true, key(MARY), false).unwrap();
        assert!(p.lease_moved(key(PRESENTER), Some(key(9))));
        assert_eq!(p.active(), None);
        assert!(p.requests().is_empty());
    }

    #[test]
    fn the_driver_learns_it_is_driving_from_the_presenter() {
        let me = key(MARY);
        let mut d = Seat::default();
        d.begin_ask(key(PRESENTER));
        assert_eq!(d.asking(), Some(&Asking::Waiting));
        let named = Named {
            device: me,
            name: "Mary's laptop".into(),
        };
        assert!(d.receive_state(key(PRESENTER), "Sanctuary PC", Some(named), me));
        assert_eq!(d.granted().map(|g| g.name.as_str()), Some("Sanctuary PC"));
        assert_eq!(d.asking(), None, "the ask is answered");
        // The presenter takes control back.
        assert!(d.receive_state(key(PRESENTER), "Sanctuary PC", None, me));
        assert_eq!(d.granted(), None);
        assert_eq!(
            d.notice(),
            Some(&Notice::TakenBack {
                by: "Sanctuary PC".into()
            })
        );
    }

    #[test]
    fn a_refused_ask_says_why() {
        let mut d = Seat::default();
        d.begin_ask(key(PRESENTER));
        d.receive_answer(Answer::Busy {
            by: "Sam's tablet".into(),
        });
        assert_eq!(
            d.asking(),
            Some(&Asking::Busy {
                by: "Sam's tablet".into()
            })
        );
        d.clear_answer();
        assert_eq!(d.asking(), None);
        d.begin_ask(key(PRESENTER));
        d.receive_answer(Answer::Declined);
        assert_eq!(d.asking(), Some(&Asking::Declined));
    }

    #[test]
    fn stopping_ends_driving_and_a_new_presenter_ends_it_too() {
        let me = key(MARY);
        let mut d = Seat::default();
        let named = Named {
            device: me,
            name: "Mary's laptop".into(),
        };
        d.receive_state(key(PRESENTER), "Sanctuary PC", Some(named.clone()), me);
        assert!(d.stop());
        assert!(!d.stop());
        d.receive_state(key(PRESENTER), "Sanctuary PC", Some(named), me);
        assert!(d.lease_moved(me, Some(key(7))));
        assert_eq!(d.granted(), None);
        assert_eq!(d.known(), None);
    }
}
