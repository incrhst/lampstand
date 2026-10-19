//! CCLI usage logging and reports (RP-1).
//!
//! Each device appends to its own `UsageLog` document (so logs never
//! conflict), and every device can build the report from all of them.
//! A song counts once per service per kind, however often it is shown.

use lampstand_core::id::Timestamp;
use lampstand_core::service::ServiceId;
use lampstand_core::song::SongId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UsageKind {
    /// Projected in the room.
    Shown,
    /// Sent to the stream output.
    Streamed,
    /// Printed as a lyric or chord sheet (Phase 2).
    Printed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageEntry {
    pub at: Timestamp,
    pub service: ServiceId,
    pub song: SongId,
    pub kind: UsageKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportLine {
    pub title: String,
    pub ccli_number: Option<u32>,
    pub shown: u32,
    pub streamed: u32,
    pub printed: u32,
}

/// Collapses entries to one count per (service, song, kind), then totals
/// per song. Sorted by title.
pub fn report(
    entries: &[UsageEntry],
    songs: &dyn Fn(SongId) -> Option<(String, Option<u32>)>,
) -> Vec<ReportLine> {
    let _ = (entries, songs);
    todo!("T38")
}

/// CSV with the columns CCLI's reporting form asks for: song title, CCLI
/// song number, and counts for display, streaming and print.
pub fn to_csv(lines: &[ReportLine]) -> String {
    let _ = lines;
    todo!("T38")
}
