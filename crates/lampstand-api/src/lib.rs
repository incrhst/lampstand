//! The API between the web UI and the Rust core. One set of messages for
//! both transports: Tauri IPC on the desktop (`invoke("api", request)` and
//! an `api-event` event stream) and, in Phase 2, a WebSocket from the
//! browser planner to the relay. TypeScript types are generated from these
//! with `ts-rs` into `app/src/lib/api/` (T45); never edit those by hand.
//!
//! Requests are grouped by area. Each returns `Response::Ok`, a typed
//! payload, or `Response::Error`.

use lampstand_core::deck::{Deck, DeckId};
use lampstand_core::output::OutputConfig;
use lampstand_core::output::OutputId;
use lampstand_core::safe_area::SafeArea;
use lampstand_core::scripture::{Passage, Translation, TranslationId};
use lampstand_core::service::{ItemId, ItemKind, Service, ServiceId};
use lampstand_core::song::{Song, SongId};
use lampstand_core::theme::Theme;
use lampstand_live::{Command, Cursor};
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "area", content = "request")]
pub enum Request {
    Library(LibraryRequest),
    Service(ServiceRequest),
    Live(Command),
    Outputs(OutputRequest),
    Team(TeamRequest),
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LibraryRequest {
    SearchSongs {
        query: String,
        limit: usize,
    },
    GetSong(SongId),
    PutSong(Song),
    ListThemes,
    PutTheme(Theme),
    ListTranslations,
    /// "Jn 3:16" in a translation, for the scripture panel (BI-1).
    LookupPassage {
        reference: String,
        translation: TranslationId,
    },
    /// Files dropped or picked: PPTX, images, video, song or Bible files.
    ImportFiles {
        paths: Vec<String>,
    },
    AddDeckFromLink {
        url: String,
    },
    AcceptDeckUpdate(DeckId),
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServiceRequest {
    List,
    Get(ServiceId),
    Create {
        title: String,
    },
    InsertItem {
        service: ServiceId,
        index: usize,
        kind: ItemKind,
    },
    MoveItem {
        service: ServiceId,
        item: ItemId,
        index: usize,
    },
    RemoveItem {
        service: ServiceId,
        item: ItemId,
    },
    /// Load into the live engine (and start pre-service caching).
    Open(ServiceId),
    Readiness(ServiceId),
    ExportPackage {
        service: ServiceId,
        path: String,
    },
    ImportPackage {
        path: String,
    },
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OutputRequest {
    ListDisplays,
    List,
    Put(OutputConfig),
    Remove(OutputId),
    /// Show the calibration pattern on an output, or stop.
    Calibrate {
        output: OutputId,
        active: bool,
    },
    SetSafeArea {
        output: OutputId,
        safe_area: SafeArea,
    },
    /// The URL to paste into OBS for a browser-source output.
    BrowserSourceUrl(OutputId),
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TeamRequest {
    Status,
    CreateInvite { role: lampstand_sync::team::Role },
    Join { invite: String },
    SetRelay { url: Option<String> },
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum Response {
    Ok,
    Error {
        message: String,
    },
    Song(Song),
    Songs(Vec<SongHit>),
    Themes(Vec<Theme>),
    Translations(Vec<Translation>),
    Passage {
        passage: Passage,
        display: String,
        verses: Vec<(u16, u16, String)>,
    },
    Service(Service),
    Services(Vec<ServiceSummary>),
    Deck(Deck),
    Outputs(Vec<OutputConfig>),
    Text(String),
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SongHit {
    pub song: SongId,
    pub title: String,
    pub line: Option<String>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceSummary {
    pub id: ServiceId,
    pub title: String,
    pub starts_at: Option<lampstand_core::id::Timestamp>,
}

/// Pushed to the UI.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum Event {
    /// Live and preview cursors changed.
    Live {
        live: Option<Cursor>,
        preview: Option<Cursor>,
        idle: bool,
    },
    /// A document changed (here or on another device); refetch if shown.
    Changed {
        kind: String,
        id: String,
    },
    SyncStatus {
        peers: usize,
        relay_connected: bool,
    },
    Readiness {
        service: ServiceId,
        ready: bool,
    },
    ImportProgress {
        done: usize,
        total: usize,
        message: String,
    },
}
