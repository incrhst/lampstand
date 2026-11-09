//! Crash recovery (T57, NFR Reliability): a small journal of what is on
//! air, so a crash mid-service comes back on the same slide.
//!
//! `App` records the live state after every change: the open service, the
//! live and preview cursors, the overlay, whether the operator cleared the
//! text or background, and which outputs had windows. Each record replaces
//! `journal.json` through a temporary file and a rename, so a process
//! killed mid-write leaves the previous record whole. The file is fsynced
//! at most every 100 ms: a record that lands sooner is synced by a later
//! record or by `tick`, so a burst of clicks costs one fsync. A process
//! that dies (even with `kill -9`) loses nothing that was written, because
//! the operating system still holds it; the fsync is for a power cut.
//!
//! On start, `App::open` reads the journal and, if a service was live less
//! than 10 minutes ago, reopens it on the same slide. A long sermon on one
//! slide would age the record, so while a service is live the record is
//! rewritten every minute even when nothing changed. A clean quit removes
//! the journal: the operator chose to stop.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use lampstand_core::id::Timestamp;
use lampstand_core::output::OutputId;
use lampstand_core::program::Overlay;
use lampstand_core::service::ServiceId;
use lampstand_live::Cursor;
use serde::{Deserialize, Serialize};

/// A journal older than this is not restored: the service is over, or the
/// operator has long since moved on.
pub const MAX_AGE_MS: i64 = 10 * 60 * 1000;

/// The least time between two fsyncs.
pub const SYNC_EVERY_MS: i64 = 100;

/// While a service is live, the record is rewritten this often even when
/// nothing changed, so its age says when the service was last live.
pub const HEARTBEAT_MS: i64 = 60 * 1000;

/// What was on air.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveRecord {
    /// The service open in the live engine.
    pub service: Option<ServiceId>,
    pub live: Option<Cursor>,
    /// The slide on the screens, when a music-only item is live (it has no
    /// picture). Missing from a journal written before 0.9.0.
    #[serde(default)]
    pub shown: Option<Cursor>,
    pub preview: Option<Cursor>,
    /// A journal from a 0.9.0 preview build may hold `"Logo"`, the overlay
    /// Idle replaced: it comes back as no overlay.
    #[serde(deserialize_with = "overlay_from_journal")]
    pub overlay: Overlay,
    /// False when the operator cleared the text (`ClearText`, `ClearAll`).
    pub text: bool,
    /// False when the background was cleared, or the slide has none.
    pub background: bool,
    /// The outputs that had windows.
    pub outputs: Vec<OutputId>,
}

/// Reads the overlay, taking the retired `"Logo"` as `Overlay::None`.
fn overlay_from_journal<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Overlay, D::Error> {
    #[derive(Deserialize)]
    enum Written {
        None,
        Black,
        Idle,
        Logo,
    }
    Ok(match Written::deserialize(d)? {
        Written::None | Written::Logo => Overlay::None,
        Written::Black => Overlay::Black,
        Written::Idle => Overlay::Idle,
    })
}

impl LiveRecord {
    /// A service is open and a slide has gone live.
    pub fn is_live(&self) -> bool {
        self.service.is_some() && self.live.is_some()
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Stored {
    /// When this was written, in milliseconds since the Unix epoch.
    at: Timestamp,
    #[serde(flatten)]
    record: LiveRecord,
}

pub struct Journal {
    path: PathBuf,
    /// The last record written, and when.
    last: Option<(LiveRecord, Timestamp)>,
    synced_at: Option<Timestamp>,
    /// Written since the last fsync.
    dirty: bool,
    /// Fsyncs done, for tests.
    syncs: usize,
}

impl Journal {
    /// The journal in `data_dir`. Nothing is read or written yet.
    pub fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join("journal.json"),
            last: None,
            synced_at: None,
            dirty: false,
            syncs: 0,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The record to restore at `now`: one that was live less than
    /// `MAX_AGE_MS` ago. Unreadable or stale journals give `None`.
    pub fn recover(&self, now: Timestamp) -> Option<LiveRecord> {
        let bytes = fs::read(&self.path).ok()?;
        let stored: Stored = match serde_json::from_slice(&bytes) {
            Ok(stored) => stored,
            Err(e) => {
                eprintln!("lampstand: ignoring an unreadable crash journal: {e}");
                return None;
            }
        };
        let age = now.0 - stored.at.0;
        // A clock that went backwards a little still counts as recent.
        let fresh = (-MAX_AGE_MS..MAX_AGE_MS).contains(&age);
        (fresh && stored.record.is_live()).then_some(stored.record)
    }

    /// Records the live state at `now`. Writes only when the record
    /// changed, or for the heartbeat while live; fsyncs when the last
    /// fsync was at least `SYNC_EVERY_MS` ago.
    pub fn record(&mut self, record: &LiveRecord, now: Timestamp) {
        let due = match &self.last {
            Some((last, at)) => {
                last != record || (record.is_live() && now.0 - at.0 >= HEARTBEAT_MS)
            }
            None => true,
        };
        if due {
            if let Err(e) = self.write(record, now) {
                eprintln!("lampstand: cannot write the crash journal: {e}");
                return;
            }
            self.last = Some((record.clone(), now));
            self.dirty = true;
        }
        self.sync_if_due(now);
    }

    /// Fsyncs a record that was written too soon after the last fsync.
    /// Called from the 100 ms tick.
    pub fn sync_if_due(&mut self, now: Timestamp) {
        let due = match self.synced_at {
            // A clock that went backwards syncs at once.
            Some(at) => !(0..SYNC_EVERY_MS).contains(&(now.0 - at.0)),
            None => true,
        };
        if self.dirty && due {
            if let Err(e) = self.sync() {
                eprintln!("lampstand: cannot sync the crash journal: {e}");
            }
            self.dirty = false;
            self.synced_at = Some(now);
            self.syncs += 1;
        }
    }

    /// A clean quit: nothing to recover.
    pub fn clear(&mut self) {
        match fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => eprintln!("lampstand: cannot remove the crash journal: {e}"),
        }
        self.last = None;
        self.dirty = false;
    }

    #[cfg(test)]
    fn syncs(&self) -> usize {
        self.syncs
    }

    fn write(&self, record: &LiveRecord, now: Timestamp) -> io::Result<()> {
        let stored = Stored {
            at: now,
            record: record.clone(),
        };
        let bytes = serde_json::to_vec(&stored).map_err(io::Error::other)?;
        let temp = self.path.with_extension("json.tmp");
        File::create(&temp)?.write_all(&bytes)?;
        // Atomic on every platform we ship: a reader sees the old record or
        // the new one, never half of one.
        fs::rename(&temp, &self.path)
    }

    fn sync(&self) -> io::Result<()> {
        // Windows flushes only through a handle that can write.
        OpenOptions::new()
            .write(true)
            .open(&self.path)?
            .sync_all()?;
        // The rename lives in the directory; sync it too where that is
        // possible (Windows cannot open a directory as a file).
        #[cfg(unix)]
        if let Some(dir) = self.path.parent() {
            File::open(dir)?.sync_all()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lampstand_core::service::ItemId;

    fn live_record() -> LiveRecord {
        let item = ItemId::generate();
        LiveRecord {
            service: Some(ServiceId::generate()),
            live: Some(Cursor {
                item,
                slide: 2,
                step: 0,
            }),
            shown: None,
            preview: Some(Cursor {
                item,
                slide: 3,
                step: 0,
            }),
            overlay: Overlay::Idle,
            text: true,
            background: false,
            outputs: vec![OutputId::generate()],
        }
    }

    #[test]
    fn a_record_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = Journal::new(dir.path());
        let record = live_record();
        journal.record(&record, Timestamp(1_000));
        assert_eq!(
            Journal::new(dir.path()).recover(Timestamp(2_000)),
            Some(record)
        );
    }

    #[test]
    fn only_a_live_record_less_than_ten_minutes_old_is_restored() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = Journal::new(dir.path());
        let record = live_record();
        journal.record(&record, Timestamp(0));
        assert!(journal.recover(Timestamp(MAX_AGE_MS - 1)).is_some());
        assert!(journal.recover(Timestamp(MAX_AGE_MS)).is_none());

        let idle = LiveRecord {
            live: None,
            ..record
        };
        journal.record(&idle, Timestamp(1));
        assert!(journal.recover(Timestamp(2)).is_none());
    }

    #[test]
    fn a_preview_journal_with_the_logo_overlay_comes_back_with_none() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = Journal::new(dir.path());
        let record = live_record();
        journal.record(&record, Timestamp(1_000));
        let written = fs::read_to_string(journal.path()).unwrap();
        assert!(written.contains("\"overlay\":\"Idle\""), "{written}");
        fs::write(
            journal.path(),
            written.replace("\"overlay\":\"Idle\"", "\"overlay\":\"Logo\""),
        )
        .unwrap();
        let back = Journal::new(dir.path()).recover(Timestamp(2_000)).unwrap();
        assert_eq!(back.overlay, Overlay::None);
        assert_eq!(
            back,
            LiveRecord {
                overlay: Overlay::None,
                ..record
            }
        );
    }

    #[test]
    fn missing_or_unreadable_journals_restore_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::new(dir.path());
        assert!(journal.recover(Timestamp(0)).is_none());
        fs::write(journal.path(), b"{\"at\":").unwrap();
        assert!(journal.recover(Timestamp(0)).is_none());
    }

    #[test]
    fn fsyncs_at_most_every_100_ms_and_catches_up_on_the_tick() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = Journal::new(dir.path());
        let mut record = live_record();
        journal.record(&record, Timestamp(0));
        assert_eq!(journal.syncs(), 1);

        // A burst of clicks: every one is written, none is synced yet.
        for (slide, at) in [(3, 10), (4, 40), (5, 90)] {
            record.live.as_mut().unwrap().slide = slide;
            journal.record(&record, Timestamp(at));
            assert_eq!(
                Journal::new(dir.path()).recover(Timestamp(at)),
                Some(record.clone())
            );
        }
        assert_eq!(journal.syncs(), 1);

        // The tick syncs the last of them once 100 ms have passed.
        journal.sync_if_due(Timestamp(99));
        assert_eq!(journal.syncs(), 1);
        journal.sync_if_due(Timestamp(100));
        assert_eq!(journal.syncs(), 2);
        journal.sync_if_due(Timestamp(500));
        assert_eq!(journal.syncs(), 2, "nothing new to sync");
    }

    #[test]
    fn an_unchanged_record_is_rewritten_only_for_the_heartbeat() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = Journal::new(dir.path());
        let record = live_record();
        journal.record(&record, Timestamp(0));
        journal.record(&record, Timestamp(HEARTBEAT_MS - 1));
        assert!(journal.recover(Timestamp(MAX_AGE_MS + 1)).is_none());
        journal.record(&record, Timestamp(HEARTBEAT_MS));
        assert!(journal.recover(Timestamp(MAX_AGE_MS + 1)).is_some());
    }

    #[test]
    fn a_clean_quit_leaves_nothing_to_restore() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = Journal::new(dir.path());
        journal.record(&live_record(), Timestamp(0));
        journal.clear();
        assert!(!journal.path().exists());
        assert!(journal.recover(Timestamp(1)).is_none());
    }
}
