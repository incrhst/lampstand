//! Sound files in the order of the service travel by sync.
//!
//! A service names its music-only items and backing tracks; the audio files
//! are blobs, which never travel inside documents. A device that has the
//! service asks the devices it is connected to for the files it lacks, ahead
//! of the service. Every device does this, not only the one that holds
//! Audio: Audio can be handed to another machine (decision 0010), and that
//! machine then needs the files already, since moving Audio never edits the
//! order.
//!
//! The policy, all of it bounded:
//!
//! - **Which services.** Those that started less than 36 hours ago or start
//!   later. An undated service is not fetched (give it a date to get its
//!   files ahead of time), and neither is a past one. A service that ages
//!   out of the window, or is deleted, stops being asked for.
//! - **When.** Only while a paired device is connected, and not while a
//!   service is on air. A device on its own does nothing, so a church that
//!   does not sync pays nothing.
//! - **What it reads.** A service is read only when its document changed
//!   (the `DocChanged` id), plus one pass over every service when this
//!   device starts or meets a device. The pass is spread over looks: three
//!   services at most per look, looks two seconds apart, so the tick that
//!   drives the live engine never waits on it. Nothing is read again while
//!   nothing changes. (Whether a service is in the window is known only by
//!   reading it; a past service is read once and then forgotten.)
//! - **How much.** At most four files are on their first ask at once. A
//!   document does not say how big its files are, so the number of files is
//!   the bound, not their size; each travels in 256 KiB chunks and is
//!   verified against its hash on arrival. Soonest service first.
//! - **A file nobody has** is asked for again after 10 s, 20 s, 40 s and so
//!   on up to ten minutes, and while it waits it does not hold a slot, so
//!   other files are not starved. When the connected devices change, the wait
//!   starts over.
//!
//! Plan asks `ServiceRequest::SoundStatus` which files are here, and whether
//! a missing one is on its way.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use lampstand_api::{Event, SoundFileStatus};
use lampstand_core::id::{BlobHash, Timestamp};
use lampstand_core::service::{ItemKind, Service, ServiceId};
use lampstand_sync::DocKind;
use uuid::Uuid;

use super::{err, App, Result};

/// How far apart looks are.
const LOOK: Duration = Duration::from_secs(2);
/// Services read per look.
const PER_LOOK: usize = 3;
/// Files on their first ask at once, at most.
const OUTSTANDING: usize = 4;
/// The first re-ask delay for a file that did not arrive; it doubles.
const ASK_AGAIN: Duration = Duration::from_secs(10);
const ASK_AGAIN_MAX: Duration = Duration::from_secs(600);
/// A service that started more than this long ago is past; its sound is not
/// fetched ahead.
const PAST: i64 = 36 * 60 * 60 * 1000;

/// Whether a service's sound files are fetched ahead of it.
pub(super) fn fetched_ahead(starts_at: Option<Timestamp>, now: Timestamp) -> bool {
    starts_at.is_some_and(|t| t.0 >= now.0 - PAST)
}

/// How a file was asked for.
struct Ask {
    next: Instant,
    tries: u32,
}

/// What a service still lacks.
struct Wanted {
    starts_at: Timestamp,
    files: Vec<BlobHash>,
}

/// The app's bookkeeping about sound files.
#[derive(Default)]
pub struct SoundSync {
    /// Services whose documents changed, waiting to be read.
    queue: VecDeque<Uuid>,
    queued: HashSet<Uuid>,
    /// The devices connected at the last look.
    connected: Vec<String>,
    next_look: Option<Instant>,
    conn_next: Option<Instant>,
    /// Files each fetched-ahead service names that this device lacks.
    wanted: HashMap<Uuid, Wanted>,
    asks: HashMap<BlobHash, Ask>,
    /// Files that were awaited at the last look.
    waiting: HashSet<BlobHash>,
    /// Service documents read, for tests.
    scans: usize,
}

impl SoundSync {
    /// A service document changed on some device: read it at the next look.
    pub(super) fn service_changed(&mut self, id: Uuid) {
        if self.queued.insert(id) {
            self.queue.push_back(id);
        }
    }

    /// Whether a first ask is still fresh, so it holds one of the slots.
    fn fresh(&self, at: Instant) -> usize {
        self.asks
            .values()
            .filter(|a| a.tries == 0 && at < a.next)
            .count()
    }
}

impl App {
    /// How many service documents the sound files have caused to be read.
    pub fn sound_scans(&self) -> usize {
        self.sound_sync.scans
    }

    /// Every sound file of `service` and where it stands on this device.
    pub(super) fn sound_status(&self, service: ServiceId) -> Result<Vec<SoundFileStatus>> {
        let service = self
            .library
            .service(service)
            .map_err(err)?
            .ok_or("no such service")?;
        let blobs = self.library.repo().blobs();
        // A missing file is on its way only if somebody is connected, the
        // service is one that is fetched ahead, and the file has been asked
        // for (it is within the slots).
        let connected = self
            .sync_link()
            .is_some_and(|link| !link.connected().is_empty());
        let ahead = connected && fetched_ahead(service.starts_at, super::now());
        let mut out = Vec::new();
        for (id, item) in service.ordered() {
            let mut files = Vec::new();
            if let ItemKind::Sound { sound } = &item.kind {
                files.push((false, sound.file));
            }
            if let Some(sound) = &item.backing {
                files.push((true, sound.file));
            }
            for (backing, file) in files {
                let on_device = blobs.path(&file).is_some();
                out.push(SoundFileStatus {
                    item: id,
                    backing,
                    file,
                    on_device,
                    coming: !on_device && ahead && self.sound_sync.asks.contains_key(&file),
                });
            }
        }
        Ok(out)
    }

    /// Reads one service's document and notes the sound files it names that
    /// this device lacks.
    fn sound_read(&mut self, id: Uuid, now: Timestamp) {
        self.sound_sync.scans += 1;
        let service: Option<Service> = self
            .library
            .service(ServiceId::from_uuid(id))
            .ok()
            .flatten();
        let blobs = self.library.repo().blobs();
        let wanted = service.and_then(|service| {
            let starts_at = service
                .starts_at
                .filter(|_| fetched_ahead(service.starts_at, now))?;
            let mut files = Vec::new();
            for item in service.items.values() {
                for sound in item.sounds() {
                    if blobs.path(&sound.file).is_none() && !files.contains(&sound.file) {
                        files.push(sound.file);
                    }
                }
            }
            (!files.is_empty()).then_some(Wanted { starts_at, files })
        });
        match wanted {
            Some(w) => {
                self.sound_sync.wanted.insert(id, w);
            }
            None => {
                self.sound_sync.wanted.remove(&id);
            }
        }
    }

    /// Asks the connected devices for sound files that have not arrived,
    /// and tells Plan when some do. Does nothing when nothing changed and
    /// nothing is awaited, so the shell calls it with every tick.
    pub(super) fn sound_tick(&mut self, now: Timestamp) {
        self.sound_tick_at(now, Instant::now());
    }

    /// `sound_tick` with the clock given, for tests.
    pub fn sound_tick_at(&mut self, now: Timestamp, at: Instant) {
        let Some(link) = self.sync_link() else {
            return;
        };
        // Who is connected is asked about once a second, not on every tick.
        if self.sound_sync.conn_next.is_none_or(|t| at >= t) {
            self.sound_sync.conn_next = Some(at + Duration::from_secs(1));
            let mut connected = link.connected();
            connected.sort();
            if connected != self.sound_sync.connected {
                if !connected.is_empty() {
                    // Meeting a device: every service once, and every wait over.
                    for id in self.library.repo().list(DocKind::Service) {
                        self.sound_sync.service_changed(id);
                    }
                    for ask in self.sound_sync.asks.values_mut() {
                        ask.next = at;
                        ask.tries = 0;
                    }
                    self.sound_sync.next_look = None;
                }
                self.sound_sync.connected = connected;
            }
        }
        let busy = !self.sound_sync.queue.is_empty() || !self.sound_sync.waiting.is_empty();
        if self.sound_sync.connected.is_empty() || !busy {
            return;
        }
        // Never while a service is on air: the live engine comes first.
        if !self.live.is_idle() {
            return;
        }
        if self.sound_sync.next_look.is_some_and(|t| at < t) {
            return;
        }
        self.sound_sync.next_look = Some(at + LOOK);

        for _ in 0..PER_LOOK {
            let Some(id) = self.sound_sync.queue.pop_front() else {
                break;
            };
            self.sound_sync.queued.remove(&id);
            self.sound_read(id, now);
        }

        // A service that has aged out of the window stops being asked for.
        let blobs = self.library.repo().blobs();
        self.sound_sync
            .wanted
            .retain(|_, w| fetched_ahead(Some(w.starts_at), now));

        // What is still missing, soonest service first.
        let mut services: Vec<&Wanted> = self.sound_sync.wanted.values().collect();
        services.sort_by_key(|w| {
            if w.starts_at.0 >= now.0 {
                (0, w.starts_at.0)
            } else {
                (1, -w.starts_at.0)
            }
        });
        let mut wanted: Vec<BlobHash> = Vec::new();
        for w in services {
            for file in &w.files {
                if blobs.path(file).is_none() && !wanted.contains(file) {
                    wanted.push(*file);
                }
            }
        }
        let wanted_set: HashSet<BlobHash> = wanted.iter().copied().collect();
        let arrived = self
            .sound_sync
            .waiting
            .iter()
            .any(|blob| !wanted_set.contains(blob));
        self.sound_sync
            .asks
            .retain(|blob, _| wanted_set.contains(blob));

        // New files first, then the ones whose wait is over, within the
        // slots that are free. A file in its wait holds no slot.
        let mut budget = OUTSTANDING.saturating_sub(self.sound_sync.fresh(at));
        let mut ask = Vec::new();
        for blob in &wanted {
            if budget > 0 && !self.sound_sync.asks.contains_key(blob) {
                budget -= 1;
                self.sound_sync.asks.insert(
                    *blob,
                    Ask {
                        next: at + again(0),
                        tries: 0,
                    },
                );
                ask.push(*blob);
            }
        }
        for blob in &wanted {
            if budget == 0 {
                break;
            }
            if let Some(a) = self.sound_sync.asks.get_mut(blob) {
                if at >= a.next {
                    budget -= 1;
                    a.tries += 1;
                    a.next = at + again(a.tries);
                    ask.push(*blob);
                }
            }
        }
        if !ask.is_empty() {
            link.fetch(ask);
        }

        let changed = wanted_set != self.sound_sync.waiting;
        self.sound_sync.waiting = wanted_set;
        if arrived || changed {
            (self.emit)(Event::Changed {
                kind: "sound".into(),
                id: String::new(),
            });
        }
    }
}

/// How long to wait before asking for a file again after `tries` re-asks.
fn again(tries: u32) -> Duration {
    ASK_AGAIN
        .saturating_mul(1u32 << tries.min(10))
        .min(ASK_AGAIN_MAX)
}
