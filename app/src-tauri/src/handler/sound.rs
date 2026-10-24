//! Sound files in the order of the service travel by sync.
//!
//! A service names its music-only items and backing tracks; the audio files
//! are blobs, which never travel inside documents. A device that has the
//! service asks the devices it is connected to for the files it lacks, ahead
//! of the service, and asks again every ten seconds until they are here.
//! Every device does this, not only the one that holds Audio: Audio can be
//! handed to another machine (decision 0010), and that machine then needs
//! the files already, since moving Audio never edits the order.
//!
//! A device on its own, with nobody connected, does nothing here, so a
//! church that does not use sound (or does not sync) pays nothing.
//!
//! Plan asks `ServiceRequest::SoundStatus` which files are here.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use lampstand_api::{Event, SoundFileStatus};
use lampstand_core::id::{BlobHash, Timestamp};
use lampstand_core::service::{ItemKind, ServiceId};

use super::{err, App, Result};

/// How long before a file that did not arrive is asked for again.
const ASK_AGAIN: Duration = Duration::from_secs(10);
/// A service that started more than this long ago is past; its sound is not
/// fetched ahead.
const PAST: i64 = 36 * 60 * 60 * 1000;

/// The app's bookkeeping about sound files.
#[derive(Default)]
pub struct SoundSync {
    /// A service document changed: look at what it names.
    look: bool,
    /// While a file is awaited: when to look again.
    retry_at: Option<Instant>,
    asked: HashSet<BlobHash>,
    asked_at: Option<Instant>,
    /// Files that were awaited at the last look.
    waiting: HashSet<BlobHash>,
}

impl SoundSync {
    /// A service document changed on some device: look at what it names.
    pub(super) fn service_changed(&mut self) {
        self.look = true;
    }
}

impl App {
    /// Every sound file of `service` and whether it is on this device.
    pub(super) fn sound_status(&self, service: ServiceId) -> Result<Vec<SoundFileStatus>> {
        let service = self
            .library
            .service(service)
            .map_err(err)?
            .ok_or("no such service")?;
        let blobs = self.library.repo().blobs();
        let mut out = Vec::new();
        for (id, item) in service.ordered() {
            if let ItemKind::Sound { sound } = &item.kind {
                out.push(SoundFileStatus {
                    item: id,
                    backing: false,
                    file: sound.file,
                    on_device: blobs.path(&sound.file).is_some(),
                });
            }
            if let Some(sound) = &item.backing {
                out.push(SoundFileStatus {
                    item: id,
                    backing: true,
                    file: sound.file,
                    on_device: blobs.path(&sound.file).is_some(),
                });
            }
        }
        Ok(out)
    }

    /// The sound files that upcoming services name and this device lacks.
    fn sound_files_wanted(&self, now: Timestamp) -> Vec<BlobHash> {
        let blobs = self.library.repo().blobs();
        let mut wanted = Vec::new();
        for service in self.library.services().unwrap_or_default() {
            if service.starts_at.is_some_and(|t| t.0 < now.0 - PAST) {
                continue;
            }
            for item in service.items.values() {
                for sound in item.sounds() {
                    if blobs.path(&sound.file).is_none() && !wanted.contains(&sound.file) {
                        wanted.push(sound.file);
                    }
                }
            }
        }
        wanted
    }

    /// Asks the connected devices for sound files that have not arrived,
    /// and tells Plan when some do. Cheap when nothing is awaited and when
    /// nobody is connected, so the shell calls it with every tick.
    pub(super) fn sound_tick(&mut self, now: Timestamp) {
        let Some(link) = self.sync_link() else {
            return;
        };
        let at = Instant::now();
        let due = self.sound_sync.look || self.sound_sync.retry_at.is_some_and(|t| at >= t);
        if !due || link.connected().is_empty() {
            return;
        }
        self.sound_sync.look = false;
        self.sound_sync.retry_at = None;
        if self
            .sound_sync
            .asked_at
            .is_some_and(|t| at.duration_since(t) >= ASK_AGAIN)
        {
            self.sound_sync.asked.clear();
            self.sound_sync.asked_at = None;
        }
        let wanted = self.sound_files_wanted(now);
        let wanted_set: HashSet<BlobHash> = wanted.iter().copied().collect();
        let arrived = self
            .sound_sync
            .waiting
            .iter()
            .any(|blob| !wanted_set.contains(blob));
        let ask: Vec<BlobHash> = wanted
            .iter()
            .copied()
            .filter(|blob| self.sound_sync.asked.insert(*blob))
            .collect();
        if !ask.is_empty() {
            self.sound_sync.asked_at.get_or_insert(at);
            link.fetch(ask);
        }
        // Still awaited: look again soon, and ask again after a while.
        if !wanted.is_empty() {
            self.sound_sync.retry_at = Some(at + Duration::from_secs(2));
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
