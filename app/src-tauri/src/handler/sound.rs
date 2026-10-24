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
//!   files ahead of time), and neither is a past one.
//! - **When.** Only while a paired device is connected. A device on its own
//!   does nothing, so a church that does not sync pays nothing.
//! - **What it reads.** A service is read only when its document changed
//!   (the `DocChanged` id), plus one pass over every service when this
//!   device starts or meets a device. The pass is spread over ticks, eight
//!   services at most per look, and looks are two seconds apart, so the tick
//!   that drives the live engine never waits on it. Nothing is read again
//!   while nothing changes.
//! - **How much.** At most four files are outstanding at once (a document
//!   does not say how big its files are, so the number of files is the
//!   bound, not their size; each travels in 256 KiB chunks and is verified
//!   against its hash on arrival). A file nobody has is asked for again
//!   after 10 s, then 20 s, 40 s and so on up to ten minutes.
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
const PER_LOOK: usize = 8;
/// Files asked for and not yet here, at most.
const OUTSTANDING: usize = 4;
/// The first re-ask delay for a file that did not arrive; it doubles.
const ASK_AGAIN: Duration = Duration::from_secs(10);
const ASK_AGAIN_MAX: Duration = Duration::from_secs(600);
/// A service that started more than this long ago is past; its sound is not
/// fetched ahead.
const PAST: i64 = 36 * 60 * 60 * 1000;

/// Whether a service's sound files are fetched ahead of it.
pub(super) fn fetched_ahead(service: &Service, now: Timestamp) -> bool {
    service.starts_at.is_some_and(|t| t.0 >= now.0 - PAST)
}

/// How a file was asked for.
struct Ask {
    next: Instant,
    tries: u32,
}

/// The app's bookkeeping about sound files.
#[derive(Default)]
pub struct SoundSync {
    /// Services whose documents changed, waiting to be read.
    queue: VecDeque<Uuid>,
    queued: HashSet<Uuid>,
    /// Whether a device was connected at the last look.
    was_connected: bool,
    next_look: Option<Instant>,
    /// Files each service names that this device lacks (fetched-ahead
    /// services only).
    wanted: HashMap<Uuid, Vec<BlobHash>>,
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
        // A missing file is on its way only if somebody is there to send it
        // and this service is one that is fetched ahead.
        let connected = self
            .sync_link()
            .is_some_and(|link| !link.connected().is_empty());
        let coming = connected && fetched_ahead(&service, super::now());
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
                    coming: !on_device && coming,
                });
            }
        }
        Ok(out)
    }

    /// Reads one service's document and notes the sound files it names that
    /// this device lacks.
    fn sound_read(&mut self, id: Uuid, now: Timestamp) {
        self.sound_sync.scans += 1;
        let service = self
            .library
            .service(ServiceId::from_uuid(id))
            .ok()
            .flatten();
        let blobs = self.library.repo().blobs();
        let files: Vec<BlobHash> = match service {
            Some(service) if fetched_ahead(&service, now) => {
                let mut files = Vec::new();
                for item in service.items.values() {
                    for sound in item.sounds() {
                        if blobs.path(&sound.file).is_none() && !files.contains(&sound.file) {
                            files.push(sound.file);
                        }
                    }
                }
                files
            }
            _ => Vec::new(),
        };
        if files.is_empty() {
            self.sound_sync.wanted.remove(&id);
        } else {
            self.sound_sync.wanted.insert(id, files);
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
        let connected = !link.connected().is_empty();
        if connected && !self.sound_sync.was_connected {
            // On starting, and on meeting a device: every service once.
            for id in self.library.repo().list(DocKind::Service) {
                self.sound_sync.service_changed(id);
            }
        }
        self.sound_sync.was_connected = connected;
        let busy = !self.sound_sync.queue.is_empty() || !self.sound_sync.waiting.is_empty();
        if !connected || !busy {
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

        let blobs = self.library.repo().blobs();
        let mut wanted: Vec<BlobHash> = Vec::new();
        for files in self.sound_sync.wanted.values() {
            for file in files {
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

        // Ask for what is due, a few at a time.
        let mut outstanding = self.sound_sync.asks.len();
        let mut ask = Vec::new();
        for blob in &wanted {
            match self.sound_sync.asks.get_mut(blob) {
                Some(a) if at >= a.next => {
                    a.tries += 1;
                    a.next = at + again(a.tries);
                    ask.push(*blob);
                }
                Some(_) => {}
                None if outstanding < OUTSTANDING => {
                    outstanding += 1;
                    self.sound_sync.asks.insert(
                        *blob,
                        Ask {
                            next: at + again(0),
                            tries: 0,
                        },
                    );
                    ask.push(*blob);
                }
                None => {}
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
