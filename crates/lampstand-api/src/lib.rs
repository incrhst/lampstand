//! The API between the web UI and the Rust core. One set of messages for
//! both transports: Tauri IPC on the desktop (`invoke("api", request)` and
//! an `api-event` event stream) and, in Phase 2, a WebSocket from the
//! browser planner to the relay. TypeScript types are generated from these
//! with `ts-rs` into `app/src/lib/api/` (T45); never edit those by hand.
//!
//! Requests are grouped by area. Each returns `Response::Ok`, a typed
//! payload, or `Response::Error`.

use lampstand_core::deck::{Deck, DeckId};
use lampstand_core::id::{BlobHash, Timestamp};
use lampstand_core::output::OutputId;
use lampstand_core::output::{DisplayMatch, OutputConfig};
use lampstand_core::program::Overlay;
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
    Obs(ObsRequest),
    Settings(SettingsRequest),
}

/// This machine's settings that are not about one screen. They stay on the
/// device, like the outputs.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SettingsRequest {
    /// Answers `Response::Settings`.
    Get,
    /// Saves them and answers `Response::Settings`.
    Put(DeviceSettings),
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceSettings {
    /// Clicker mode (T56, PS-9): while a service is open for live, a
    /// presenter clicker's Page Up, Page Down, B and full stop work
    /// whichever window has focus, and B means Black, not Bridge. Off by
    /// default, because while it is on those keys never reach other apps.
    #[serde(default)]
    pub clicker_mode: bool,
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
    /// Files or folders dropped or picked: PPTX, images, video, song or
    /// Bible files, an OpenSong data or `Songs` folder, an OpenSongApp
    /// backup (`.osb`). Songs already in the library are not added again.
    /// Answers `Response::Text` with a summary; progress comes as
    /// `Event::ImportProgress`.
    ImportFiles {
        paths: Vec<String>,
    },
    /// The free song databases this app can download, as
    /// `Response::SongDatabases`.
    ListSongDatabases,
    /// Downloads one language's database (a `SongDatabase::code`) and
    /// imports it as `ImportFiles` would.
    DownloadSongDatabase {
        code: String,
    },
    /// The decks in the library, by title, as `Response::Decks`.
    ListDecks,
    /// A PPTX picked from disk (PS-1). It is parsed once to check it,
    /// stored as a blob and added as a `Deck` with a `File` source, titled
    /// by its file name. Answers `Response::Deck`.
    AddDeckFile {
        path: String,
    },
    /// A pasted "anyone with the link" Google Slides, Google Drive or
    /// OneDrive link: downloaded, checked and added as `AddDeckFile` does,
    /// with the link as its source. Answers `Response::Deck`.
    AddDeckFromLink {
        url: String,
    },
    AcceptDeckUpdate(DeckId),
    /// An image or video picked from disk, copied into the blob store for
    /// a `Media` item. Answers `Response::Media` with the blob and the
    /// file's name; the caller inserts the item.
    AddMediaFile {
        path: String,
    },
    /// Every image and video used in a service, once each, by name, as
    /// `Response::MediaFiles`. Media has no document of its own; the
    /// services are its library.
    ListMedia,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServiceRequest {
    List,
    Get(ServiceId),
    /// A new, empty service. `starts_at` is when it is scheduled, if the
    /// planner picked a time (the UI defaults to the coming Sunday), and
    /// `time_zone` the IANA zone it was picked in (`Service::time_zone`).
    Create {
        title: String,
        starts_at: Option<Timestamp>,
        time_zone: Option<String>,
    },
    /// Moves a service to another time and zone, or clears its date with
    /// `starts_at: None`. Answers `Response::Service`.
    Reschedule {
        service: ServiceId,
        starts_at: Option<Timestamp>,
        time_zone: Option<String>,
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
    /// Load into the live engine (and start pre-service caching). Answers
    /// `Response::Loaded` with the slides the operator can show.
    Open(ServiceId),
    /// A picture of one slide of the open service after `step` build
    /// steps (0: as it arrives, with anything that plays by itself done),
    /// `width` pixels wide, for the operator view: `Response::Thumbnail`.
    /// Deck slides only for now; text slides are drawn from their lines
    /// (T47 draws the rest).
    SlideThumbnail {
        item: ItemId,
        slide: usize,
        #[serde(default)]
        step: usize,
        width: u32,
    },
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
    /// The monitors this machine has now, as `Response::Displays`.
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
    /// The URL to paste into OBS for a browser-source output, as
    /// `Response::Text`: `http://<host>:<port>/obs/<output>?k=<key>`.
    BrowserSourceUrl(OutputId),
    /// Gives a browser-source output a new key, so a leaked URL stops
    /// working; answers the new URL as `Response::Text`. Pages already
    /// open keep going until they reload.
    RotateBrowserSourceKey(OutputId),
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TeamRequest {
    /// This device, its paired devices and the devices nearby, as
    /// `Response::Team`.
    Status,
    CreateInvite {
        role: lampstand_sync::team::Role,
    },
    /// Until T18, `invite` is the six-digit pairing code another device is
    /// showing. With `address` (`host` or `host:port`), pairs with the
    /// device there; otherwise with whichever device nearby (mDNS) is
    /// showing a code and accepts it. Answers `Response::Team`.
    Join {
        invite: String,
        address: Option<String>,
    },
    SetRelay {
        url: Option<String>,
    },
    /// Shows a pairing code on this device (the presenting machine) for
    /// five minutes: `Response::Team`, with `pairing` set.
    StartPairing,
    StopPairing,
    /// Connects to an already paired device by address, for networks that
    /// block mDNS. Remembered for reconnecting.
    Connect {
        address: String,
    },
    /// Stops syncing with a paired device (its key in hex).
    Unpair {
        device: String,
    },
}

/// Running OBS from Lampstand (T89; docs/decisions/0005). Each answers
/// `Response::Obs`.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ObsRequest {
    /// The settings (never the password), the connection and, when OBS
    /// isn't connected, why.
    Status,
    /// Saves where OBS is and reconnects, then answers once the attempt has
    /// an outcome (connected, refused, or no answer within a few seconds):
    /// the "Save and test" button. `password: None` keeps the saved one;
    /// an empty string clears it.
    Configure {
        host: String,
        port: u16,
        password: Option<String>,
    },
    /// Starts OBS on this machine (the "Open OBS" button). Never changes
    /// OBS's own settings.
    Launch,
}

/// OBS as the operator and Setup see it. Also pushed as `Event::Obs`
/// whenever it changes.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObsView {
    pub host: String,
    pub port: u16,
    /// A password is saved. The password itself never leaves the core.
    pub has_password: bool,
    pub connection: ObsConnection,
    /// Why OBS isn't connected, when the core can tell; `None` while
    /// connected or connecting.
    pub problem: Option<ObsProblem>,
    /// Whether the operator view should mention OBS at all: true when an
    /// enabled output streams (a browser source) or OBS settings were
    /// saved. Churches that don't stream never see OBS warnings.
    pub relevant: bool,
    /// `None` until connected.
    pub stream: Option<ObsOutputView>,
    pub record: Option<ObsOutputView>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ObsConnection {
    Connecting,
    Connected {
        obs_version: String,
        websocket_version: String,
    },
    /// Retrying; `reason` is for people.
    Disconnected {
        reason: String,
    },
    PasswordRequired,
    WrongPassword,
}

/// What to tell people when OBS isn't connected.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObsProblem {
    /// Installed (or can't tell) and not running: offer "Open OBS".
    NotRunning,
    /// Offer "Get OBS" (obsproject.com).
    NotInstalled,
    /// OBS is running but nothing answers on the port: its WebSocket server
    /// is off (Tools › WebSocket Server Settings) or on another port.
    ServerOff,
    PasswordRequired,
    WrongPassword,
    /// OBS is on another machine and doesn't answer.
    Unreachable,
}

/// Streaming or recording, for the status pill.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObsOutputView {
    pub active: bool,
    /// The stream dropped and OBS is reconnecting it (streaming only).
    pub reconnecting: bool,
    /// "HH:MM:SS", empty when inactive.
    pub timecode: String,
    /// Frames OBS skipped or dropped (streaming only), and out of how many.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub skipped_frames: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub total_frames: u64,
}

/// This device on the network, for the devices screen.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamStatus {
    pub name: String,
    /// Two groups of four hex digits, for people to compare: "3fa9 c201".
    pub fingerprint: String,
    /// Where other devices can reach this one: `"192.168.1.20:7340"`.
    pub addresses: Vec<String>,
    /// The pairing code shown now, if any.
    pub pairing: Option<PairingView>,
    pub paired: Vec<DeviceView>,
    /// Lampstand devices seen on the network that are not paired.
    pub nearby: Vec<NearbyView>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairingView {
    pub code: String,
    /// Unix milliseconds.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub expires_ms: i64,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceView {
    /// The device key in hex, for `Unpair`.
    pub device: String,
    pub fingerprint: String,
    pub name: String,
    pub address: Option<String>,
    pub connected: bool,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NearbyView {
    pub device: String,
    pub fingerprint: String,
    pub name: String,
    pub address: String,
    /// It is showing a pairing code now.
    pub pairing: bool,
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
    Decks(Vec<Deck>),
    /// A file added to the blob store by `AddMediaFile`.
    Media(MediaFile),
    MediaFiles(Vec<MediaFile>),
    Outputs(Vec<OutputConfig>),
    /// The monitors connected now, as a `Display` target would match them.
    Displays(Vec<DisplayMatch>),
    /// The service now in the live engine, split into slides.
    Loaded(LoadedService),
    SongDatabases(Vec<SongDatabase>),
    Text(String),
    Team(TeamStatus),
    /// A PNG as a `data:` URL, for an `<img>`.
    Thumbnail(String),
    Obs(ObsView),
    Settings(DeviceSettings),
}

/// An image or video in the blob store, as a `Media` item names it.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaFile {
    pub blob: BlobHash,
    /// The file name it was added with: "Welcome loop.mp4".
    pub name: String,
    /// Whether its bytes are on this device (a synced service can name a
    /// blob that has not arrived yet).
    pub on_device: bool,
}

/// A song database that can be downloaded to start a library.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SongDatabase {
    /// What `DownloadSongDatabase` takes.
    pub code: String,
    /// "English", "Spanish".
    pub name: String,
    /// Where it comes from, to show the operator: "worshipleaderapp.com".
    pub source: String,
}

/// A service as the live engine holds it: every item with its slides, in
/// order. Cursors in `Event::Live` index into this.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadedService {
    pub service: ServiceId,
    pub title: String,
    pub items: Vec<LoadedItemView>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadedItemView {
    pub id: ItemId,
    pub title: String,
    /// Empty for headers and for items that cannot be shown yet.
    pub slides: Vec<SlideView>,
}

/// One slide as the operator sees it.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlideView {
    /// "V1", "John 3:16", or the slide's kind for non-text slides.
    pub label: Option<String>,
    pub lines: Vec<String>,
    /// Build steps after the slide appears (deck slides).
    pub steps: usize,
    /// Speaker notes (deck slides), one line per paragraph; empty when
    /// there are none.
    #[serde(default)]
    pub notes: String,
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
    pub starts_at: Option<Timestamp>,
    pub time_zone: Option<String>,
    /// Items that put something on screen (headers don't), so Present can
    /// say a service has nothing in it before it is opened.
    pub items: u32,
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
        /// Black or logo, which persist across slides.
        overlay: Overlay,
    },
    /// A service was loaded or reloaded (an edit synced in mid-service):
    /// the operator view should show these slides.
    Loaded(LoadedService),
    /// A document changed (here or on another device); refetch if shown.
    /// When many change at once (an import, a first sync), one event with
    /// an empty `id` stands for all of that kind.
    Changed {
        kind: String,
        id: String,
    },
    /// Connections or nearby devices changed: `peers` paired devices are
    /// connected now. Refetch `TeamRequest::Status` for the details.
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
    /// OBS's connection, streaming or recording changed.
    Obs(ObsView),
}
