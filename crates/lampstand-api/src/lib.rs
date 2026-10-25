//! The API between the web UI and the Rust core. One set of messages for
//! both transports: Tauri IPC on the desktop (`invoke("api", request)` and
//! an `api-event` event stream) and, in Phase 2, a WebSocket from the
//! browser planner to the relay. TypeScript types are generated from these
//! with `ts-rs` into `app/src/lib/api/` (T45); never edit those by hand.
//!
//! Requests are grouped by area. Each returns `Response::Ok`, a typed
//! payload, or `Response::Error`.

use lampstand_core::brand::{Brand, LogoBug};
use lampstand_core::deck::{Deck, DeckId};
use lampstand_core::id::{BlobHash, Timestamp};
use lampstand_core::output::OutputId;
use lampstand_core::output::{DisplayMatch, IdleScreen, OutputConfig};
use lampstand_core::program::Overlay;
use lampstand_core::safe_area::SafeArea;
use lampstand_core::scripture::{Passage, Translation, TranslationId};
use lampstand_core::service::{ItemId, ItemKind, Service, ServiceId, Sound};
use lampstand_core::song::{Song, SongId};
use lampstand_core::theme::{Theme, ThemeId};
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
    Sound(SoundRequest),
    Update(UpdateRequest),
    About(AboutRequest),
}

/// Playing the sound in a service (decision 0010, the Audio role). Any
/// device may ask; the device that holds Audio plays, and a device that may
/// not control the sound is told so in words.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SoundRequest {
    /// Plays the music-only item `item`, or the backing track of the song
    /// `item` when `backing` is set, from the start.
    Play {
        item: ItemId,
        backing: bool,
    },
    Pause,
    Resume,
    /// Fades out over a few seconds, then stops.
    Stop,
    /// Silence at once. Always works, whoever holds what.
    Panic,
    /// The sound outputs on this computer: `Response::SoundOutputs`.
    Outputs,
    /// What is playing, for a screen that has just opened: `Response::Sound`.
    Status,
}

/// What the screens show of the sound.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoundView {
    /// This device plays the sound (it holds Audio, or it is alone).
    pub here: bool,
    /// Who plays it, when it is another device; empty when nobody does.
    pub holder: String,
    /// Another device holds Audio but is not connected now.
    pub away: bool,
    /// Nobody holds Audio, so a sound would play nowhere.
    pub nobody: bool,
    /// The sound output is open and working (only said where it plays).
    pub output_ok: bool,
    pub playing: Option<SoundPlaying>,
    /// Why the last sound did not play, or why the sound output is not
    /// working, in plain words, if so. Said on every device that shows the
    /// sound, not only the one that plays it.
    pub problem: Option<String>,
    /// The item `problem` belongs to, so a screen can show it next to that
    /// item ("" or `None` when it is about the sound output as a whole).
    pub problem_item: Option<String>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoundPlaying {
    pub item: String,
    pub name: String,
    pub paused: bool,
    /// A fade out is running.
    pub ending: bool,
    /// Where it was, in milliseconds, when this was said.
    pub position_ms: u32,
    pub length_ms: u32,
}

/// The sound outputs on this computer, for Setup > Sound.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoundOutputs {
    pub devices: Vec<String>,
    /// The output used when none is chosen.
    pub default: Option<String>,
    /// The output chosen on this device, if one is.
    pub chosen: Option<String>,
}

/// Telling the project about a problem or an idea, or asking for help. Each
/// opens the right GitHub page in the browser (an issue form, or a new
/// question in Discussions) with the version and the kind of computer
/// filled in; the person submits it themselves.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AboutRequest {
    ReportProblem,
    SuggestIdea,
    /// A question about using Lampstand, which goes in Discussions rather than Issues.
    AskForHelp,
    /// The optional form where a church says who it is and how it plans to use Lampstand.
    TellUs,
    /// The documentation on the project's website.
    ReadDocs,
}

/// Updating Lampstand itself (T59).
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateRequest {
    /// What is known now, without asking the network: `Response::Update`.
    Status,
    /// Asks whether a newer release exists, and answers `Response::Update`.
    Check,
    /// Downloads the release `Check` found, installs it and restarts. Refused
    /// unless nothing is on air and no service starts within the hour, so an
    /// update never interrupts one.
    Install,
}

/// Where updating stands.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateStatus {
    /// The version running now.
    pub current: String,
    pub state: UpdateState,
    /// Why `Install` would be refused right now, if it would be.
    pub blocked: Option<String>,
    /// Something to ask the person before installing, which does not refuse
    /// it: the next service's start cannot be known. The screen asks, and
    /// then `Install` goes ahead.
    #[serde(default)]
    pub caution: Option<String>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateState {
    /// No check has been made since the app started.
    NotChecked,
    UpToDate,
    /// A newer release exists. `notes` are its release notes.
    Available {
        version: String,
        notes: Option<String>,
    },
    /// Downloading and installing; the app restarts when it is done.
    Installing,
    /// The last check or install failed.
    Failed {
        message: String,
    },
}

/// Settings. The clicker mode and the default Bible stay on this machine,
/// like the outputs. The church's look, default theme and default idle
/// screen follow the church to its other devices (`ChurchStatus`); they
/// are read and saved here all the same.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SettingsRequest {
    /// Answers `Response::Settings`.
    Get,
    /// Saves them and answers `Response::Settings`.
    Put(DeviceSettings),
    /// Saves the church's brand and makes a theme from it, the quick start
    /// in Get started: the theme made last time is updated, or a new one
    /// added if it is gone, and it becomes the default theme. Answers
    /// `Response::Settings`.
    SaveBrand(Brand),
    /// Turns the logo bug on or off, or changes its corner, size or
    /// opacity, keeping the rest of the brand. Answers `Response::Settings`.
    SetLogoBug(LogoBug),
    /// How the church's shared settings stand: whether they are shared,
    /// whether this device and another disagree, and the files still to
    /// arrive. Answers `Response::Church`.
    Church,
    /// Settles a `ChurchStatus::conflict`, once. Answers `Response::Church`.
    ResolveChurch(ChurchChoice),
}

/// Which of two different church looks to keep.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChurchChoice {
    /// This device's look becomes the church's, on every device.
    ThisDevice,
    /// The look from the other device replaces this device's.
    Shared,
}

/// The church look and idle settings follow the church across its paired
/// devices (`lampstand_core::church`). This is how that stands on this one.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChurchStatus {
    /// The settings live in the synced document. False on a device that
    /// has never met another one with settings of its own, where they
    /// still belong to this device only.
    pub shared: bool,
    /// This device and another each had a look of their own when they met.
    pub conflict: Option<ChurchConflict>,
    /// Files the settings point at that have not reached this device yet.
    pub waiting: Vec<WaitingFile>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChurchConflict {
    /// The device the other look came from: "Mary's laptop".
    pub other_device: String,
    /// What this device has, in a line: "Grace Chapel, logo, Picture: Welcome.png".
    pub mine: String,
    pub theirs: String,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaitingFile {
    /// "idle video", "idle picture" or "logo".
    pub what: String,
    /// The device that set it, or "your other devices".
    pub from: String,
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
    /// Setup › Sound › "Use sound in services": this device offers music-only
    /// items and backing tracks in Plan. Off by default. A service that
    /// already has sound shows it whatever this says. Kept on this device
    /// (not in the synced church settings): a service with sound turns the
    /// controls on for every device that has it anyway.
    #[serde(default)]
    pub use_sound: bool,
    /// Setup › Sound › Output: the sound output this device plays through
    /// when it holds Audio, by name. Unset, or no longer on this computer,
    /// means the computer's own default.
    #[serde(default)]
    pub sound_output: Option<String>,
    /// The Bible version scripture is read from when a reference does not
    /// name one ("James 3:11" rather than "James 3:11 ESV"). Unset, or no
    /// longer on this device, means the first version by name.
    #[serde(default)]
    pub default_translation: Option<TranslationId>,
    /// The church's name, logo, colours and font (the quick start). The
    /// logo is what the Logo overlay shows.
    #[serde(default)]
    pub brand: Option<Brand>,
    /// The theme made from `brand`, so saving the brand again updates it.
    #[serde(default)]
    pub brand_theme: Option<ThemeId>,
    /// The theme used where neither the screen nor the service names one.
    /// Unset means the Starter theme.
    #[serde(default)]
    pub default_theme: Option<ThemeId>,
    /// What screens show when nothing is on air, unless a screen has its
    /// own. Unset means the theme's plain background.
    #[serde(default)]
    pub idle: Option<IdleScreen>,
    /// Set by the app: `brand`, `brand_theme`, `default_theme` and `idle`
    /// are the church's, shared through the synced document, rather than
    /// this device's own. While false they are kept here, as before. A
    /// save from the UI cannot change this.
    #[serde(default)]
    pub church_synced: bool,
}

/// What opening a brand file does to the themes already here.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrandImportMode {
    /// Adds the file's themes beside the ones here, renaming any with the
    /// same name. The saved brand and idle screen are only filled in if
    /// there are none.
    AddAlongside,
    /// A theme replaces the one with the same name (never the Starter
    /// theme) and is added if there is none. The saved brand and idle
    /// screen are replaced.
    Replace,
}

/// What a brand file holds, for the "Import brand" dialog.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrandContents {
    /// The church's name, if the file has a brand.
    pub name: Option<String>,
    pub has_logo: bool,
    /// The brand's font, by name only.
    pub font_family: Option<String>,
    /// That font is installed or bundled on this computer.
    pub font_installed: bool,
    /// Every font the themes use that this computer lacks, by name.
    pub missing_fonts: Vec<String>,
    pub themes: Vec<String>,
    /// "Picture: Welcome.png" or "Looping video: Loop.mp4"; none if unset.
    pub idle: Option<String>,
    /// There is a brand here, so saving it replaces the one saved now.
    pub has_brand: bool,
}

/// The sample text a theme preview is drawn with.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeSample {
    Lyrics,
    Verse,
    /// A text slide: a heading and a few lines, as announcements use.
    Text,
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
    /// Deletes a theme. Refused for the last one and for the Starter
    /// theme, with a message for people. Answers `Response::Ok`.
    DeleteTheme(ThemeId),
    /// Puts the Starter theme back as it was made. Answers `Response::Ok`.
    RestoreStarterTheme,
    /// Saves the church's brand, its themes and its idle screen, with the
    /// logo and every picture and video they use, as one file at `path`
    /// (`.lampstand-brand`). Answers `Response::Text` saying what went in
    /// and what was left out (a video that is too large).
    ExportBrand {
        path: String,
    },
    /// Reads a brand file without changing anything, to show what it
    /// holds before asking. Answers `Response::BrandFile`; a file from a
    /// newer version is an error that says so.
    InspectBrand {
        path: String,
    },
    /// Adds or replaces what a brand file holds. Every theme gets a new ID.
    /// Answers `Response::Text` with a summary.
    ImportBrand {
        path: String,
        mode: BrandImportMode,
    },
    /// A picture of `theme` as `sample` looks on a screen `width` by
    /// `height` pixels in shape, `thumb_width` pixels wide, drawn by the
    /// same layout and renderer as the screens. Answers `Response::Thumbnail`.
    ThemePreview {
        theme: Theme,
        sample: ThemeSample,
        width: u32,
        height: u32,
        thumb_width: u32,
    },
    /// A picture of the default idle screen (`IdleScreen::Default`) for a
    /// church with this `brand` (none: Lampstand's own), `width` pixels
    /// wide, drawn by the same layout and renderer as the screens in the
    /// theme `theme` (the starter theme if none). Answers
    /// `Response::Thumbnail`.
    IdlePreview {
        brand: Option<Brand>,
        theme: Option<ThemeId>,
        width: u32,
    },
    ListTranslations,
    /// "Jn 3:16" in a translation, for the scripture panel (BI-1). A version
    /// typed after the reference ("Jn 3:16 ESV") wins over `translation`,
    /// which is the default for a reference that names none. A version
    /// that matches no translation is an error that lists the ones there
    /// are.
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
    /// Imports SongSelect files (`.usr`, or the `.txt` lyrics export) that
    /// landed in the user's Downloads folder at or after `since` (Unix
    /// seconds), as `ImportFiles` would. Answers `Response::Text` with a
    /// summary, or an empty text when there was nothing new.
    ImportSongSelectDownloads {
        since: u64,
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
    /// An audio file picked from disk, copied into the blob store for a
    /// sound item or a song's backing track. Answers `Response::SoundFile`
    /// with the blob, a name to start from and the length the file gives;
    /// the caller inserts the item.
    AddSoundFile {
        path: String,
    },
    /// A small picture of an image or video in the blob store, `width`
    /// pixels wide, for the library and the service order. For a video it
    /// is the first frame that is not blank (a fade-in from black is
    /// skipped), not simply the first. `name` is the file's name, which says
    /// which kind it is. Answers `Response::Thumbnail`.
    MediaThumbnail {
        blob: BlobHash,
        name: String,
        width: u32,
    },
    /// A picture of a deck's first slide (as it looks once its builds have
    /// played), `width` pixels wide, for the library and the service order.
    /// Answers `Response::Thumbnail`.
    DeckThumbnail {
        deck: DeckId,
        width: u32,
    },
    /// One slide of a deck (counted from 0, hidden slides left out) as it
    /// looks once its builds have played, `width` pixels wide, and how many
    /// slides the deck has, for choosing the slide a screen shows when
    /// nothing is on air. Answers `Response::DeckSlide`.
    DeckSlidePicture {
        deck: DeckId,
        slide: usize,
        width: u32,
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
    /// A new service started from an existing one: the same items in the
    /// same order with their notes, OBS scenes, loops and arrangements, all
    /// with fresh IDs, under `title` and the given time. Answers
    /// `Response::Service` with the copy.
    Duplicate {
        service: ServiceId,
        title: String,
        starts_at: Option<Timestamp>,
        time_zone: Option<String>,
    },
    /// A new copy of the welcome service a new install starts with (two
    /// hymns, a verse and a walk-in music), for trying things out on an
    /// install that never had it or deleted it. Puts the hymns back if they
    /// were deleted. Answers `Response::Service`.
    CreateSample {
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
    /// Renames a service. Answers `Response::Service`.
    Rename {
        service: ServiceId,
        title: String,
    },
    /// The theme this service is shown in; `None` uses the default theme.
    /// A screen's own theme still wins. Answers `Response::Service`.
    SetTheme {
        service: ServiceId,
        theme: Option<ThemeId>,
    },
    /// Deletes a service for good. One that is open for live goes off air
    /// first. Answers `Response::Ok`.
    Delete(ServiceId),
    /// Takes the open service off air: a clean screen and nothing loaded.
    /// Answers `Response::Ok`; a service that is not open is left alone.
    Close(ServiceId),
    /// Sets the sound of an item: the `sound` of a sound item (`Item`, which
    /// needs `Some`) or a song's backing track (`Backing`, where `None`
    /// takes it off). Answers `Response::Service`.
    SetSound {
        service: ServiceId,
        item: ItemId,
        slot: SoundSlot,
        sound: Option<Sound>,
    },
    /// Where each sound file of a service stands on this device, as
    /// `Response::SoundStatus`: Plan says which are not here yet.
    SoundStatus(ServiceId),
    /// Like `List`, and also says which services cannot be shown, as
    /// `Response::ServiceList`: one read of every service for both.
    ListChecked,
    /// Plays a media item once or on repeat. A video that repeats by itself
    /// holds a looping section on it for good. Answers `Response::Service`.
    SetMediaLooped {
        service: ServiceId,
        item: ItemId,
        looped: bool,
    },
    /// Changes how long a looping section leaves each still slide up, for
    /// the service that is open for live and only until it is closed or
    /// another is opened; `None` turns the section's loop off. `header` is
    /// the section's header. The saved service is not touched. Answers
    /// `Response::Ok`.
    SetLiveLoop {
        header: ItemId,
        seconds: Option<u16>,
    },
    /// What each item of a service puts on screen when it loops, for
    /// working out how long a pass of a section takes. Answers
    /// `Response::Estimate`.
    Estimate(ServiceId),
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
    /// The OBS scene an item switches to when it goes live (T90); `None`
    /// (or blank) leaves OBS as it is. Answers `Response::Service`.
    SetObsScene {
        service: ServiceId,
        item: ItemId,
        scene: Option<String>,
    },
    /// The order a song item sings its sections in for this service,
    /// overriding the song's own; `None` (or an empty list) goes back to the
    /// song's order. Labels match ignoring case. Answers `Response::Service`.
    SetItemArrangement {
        service: ServiceId,
        item: ItemId,
        arrangement: Option<Vec<String>>,
    },
    /// Rewrites a text slide item: its title, body and attribution (blank
    /// clears it). Answers `Response::Service`.
    SetItemText {
        service: ServiceId,
        item: ItemId,
        title: String,
        body: String,
        attribution: Option<String>,
    },
    /// Loop an item or section (pre- and post-service): seconds per slide,
    /// or `None` for no loop. On a header it loops the items after it up to
    /// the next header. Answers `Response::Service`.
    SetLoop {
        service: ServiceId,
        item: ItemId,
        seconds: Option<u16>,
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
    /// How many OBS pages are showing each browser-source output now, as
    /// `Response::BrowserSources`: "OBS is showing this" or "waiting".
    BrowserSourceStatus,
}

/// One browser-source output and the OBS pages showing it.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserSourceStatus {
    pub output: OutputId,
    /// Pages watching it now; 0 means OBS has not loaded the URL (or has
    /// stopped).
    pub clients: u32,
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
    /// Becomes the presenting device when nobody needs to allow it: nobody
    /// holds the role, or the presenter has been offline past the takeover
    /// time. Answers `Response::Team`, or an error saying to ask.
    PresenterClaim,
    /// Asks the presenting device to hand over; it shows a prompt.
    PresenterAsk,
    /// Withdraws this device's ask, or clears the "not now" it was shown.
    PresenterCancel,
    /// On the presenting device: hands the role to a device that asked
    /// (its key in hex). This device becomes Planning and its screens clear.
    PresenterAllow {
        device: String,
    },
    /// On the presenting device: says not now to a device that asked.
    PresenterDecline {
        device: String,
    },
    /// Clears the notice about having been replaced as presenter.
    PresenterDismiss,
    /// How long the presenter may be offline before another device may take
    /// over without asking, on this device.
    SetTakeover {
        seconds: u32,
    },
    /// On a device that is not presenting: asks the presenting device to let
    /// this one drive it (operate it from here). The presenter sees a prompt
    /// unless it remembers this device.
    DriveAsk,
    /// Stops driving, or withdraws the ask to drive.
    DriveCancel,
    /// Clears a "not now", "busy" or "taken back" shown to the driver.
    DriveDismiss,
    /// On the presenting device: lets a device that asked drive (its key in
    /// hex); with `always`, remembers it so it is not asked again.
    DriveAllow {
        device: String,
        always: bool,
    },
    /// On the presenting device: says not now to a device that asked.
    DriveDecline {
        device: String,
    },
    /// On the presenting device: takes control back from the driver at once.
    DriveTakeBack,
    /// Remembers a paired device as always allowed to drive this one, or
    /// stops (its key in hex).
    DriveAlways {
        device: String,
        always: bool,
    },
    /// Takes a role when nobody needs to allow it: nobody holds it, or its
    /// holder has been offline past the takeover time (the screens say so
    /// and ask first). Answers `Response::Team`, or an error in the
    /// volunteer's words.
    RoleClaim {
        role: RoleName,
    },
    /// Asks the holder of a role to hand it over; it shows a prompt.
    RoleAsk {
        role: RoleName,
    },
    /// Withdraws this device's ask for a role, or clears the "not now" or
    /// "already asking" it was shown.
    RoleCancel {
        role: RoleName,
    },
    /// On the holder: hands the role to a device that asked (its key in
    /// hex). With `always`, remembers the device; only Operator and Stream
    /// offer that. Passing Projection clears the screens and passing Audio
    /// stops the music.
    RoleAllow {
        role: RoleName,
        device: String,
        always: bool,
    },
    /// On the holder: says not now to a device that asked.
    RoleDecline {
        role: RoleName,
        device: String,
    },
    /// Clears the notice about having lost a role.
    RoleDismiss {
        role: RoleName,
    },
    /// On the holder: remembers a paired device as always allowed to take
    /// the role, or stops (its key in hex). Operator and Stream only.
    RoleAlways {
        role: RoleName,
        device: String,
        always: bool,
    },
}

/// Running OBS from Lampstand (T89; docs/decisions/0005). Each answers
/// `Response::Obs`, except `Screenshot`.
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
    /// Opens OBS's download page in the default browser (the "Get OBS"
    /// button). The address is fixed in the core, so the UI can't open
    /// any other page.
    Download,
    /// The operator picks a scene by hand (T91). It holds until the next
    /// item goes live: the live item's own scene isn't applied again.
    SetScene { scene: String },
    /// Ends a hold and switches to the live item's scene, if it has one.
    Resume,
    /// A JPEG of what OBS shows now, `width` pixels wide, for the live
    /// preview in Present's OBS drawer (T91). `scene: None` is the program
    /// scene; a name previews that scene before switching to it. Answers
    /// `Response::Thumbnail`.
    Screenshot { scene: Option<String>, width: u32 },
    /// Mutes or unmutes one of OBS's audio inputs (T91).
    SetMute { input: String, muted: bool },
    /// "Set up OBS" (T93, #133): makes a "Lampstand" scene collection in
    /// OBS with starter scenes, each showing the picture `output` (a
    /// browser-source output). The core makes the picture's address itself;
    /// the window sends no address. Run again, it changes only that source.
    /// When OBS is on another computer the address is this computer's
    /// network address (the pinned one, if there is one), and never
    /// `localhost`. Answers `Response::Text` with what was done.
    SetUp { output: OutputId },
    /// Pins the address OBS on another computer uses to reach this one, for
    /// a computer that has several ("Use this address"). `None` goes back
    /// to the address Lampstand picks.
    UseAddress { address: Option<String> },
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
    /// OBS's scenes, top of its list first; empty until connected. For
    /// the scene picker in Plan and the buttons in Present.
    pub scenes: Vec<String>,
    /// The scene on air in OBS, while connected.
    pub program_scene: Option<String>,
    /// The operator chose a scene by hand (here or in OBS itself), so the
    /// live item's scene isn't applied until the next item goes live.
    pub held: bool,
    /// The live item's scene couldn't be switched to; cleared by the next
    /// switch that works.
    pub scene_problem: Option<ObsSceneProblem>,
    /// OBS's audio inputs (microphones, desktop audio, media with sound)
    /// in OBS's order, while connected, for the mute toggles (T91).
    pub audio: Vec<ObsAudioInput>,
    /// OBS is on another computer (its host isn't this one). Everything
    /// below is empty or false for a church with OBS on this computer.
    pub remote: bool,
    /// This computer's network addresses, when `remote`, for the "Use this
    /// address" picker.
    pub addresses: Vec<String>,
    /// The address pinned for OBS to reach this computer, when `remote`.
    pub pinned_address: Option<String>,
}

/// One of OBS's audio inputs and whether it is muted.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObsAudioInput {
    pub name: String,
    pub muted: bool,
}

/// Why the live item's OBS scene isn't on air.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObsSceneProblem {
    pub scene: String,
    pub reason: ObsSceneFailure,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObsSceneFailure {
    /// OBS has no scene of that name (renamed or deleted).
    Missing,
    /// OBS isn't connected; it switches when OBS connects.
    NotConnected,
    /// OBS refused or didn't answer; `message` is for people.
    Failed { message: String },
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
    /// OBS is on another machine and doesn't answer: wrong name or address,
    /// another network, or a firewall.
    Unreachable,
    /// OBS is on another machine that answers, but nothing is listening on
    /// OBS's port: OBS isn't open there, its WebSocket server is off, or the
    /// port differs.
    RemoteNotListening,
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
    /// Who may present, and what this device can do about it.
    pub presenter: PresenterView,
    /// The four roles in order (Projection, Operator, Stream, Audio): who
    /// holds each, how this device could take it, and any prompt or notice.
    pub roles: Vec<RoleView>,
    /// A scene change is waiting for the device that runs the stream, which
    /// is offline (docs/roles.md, "Where OBS lives"). Drives the status bar
    /// line "{Name} runs the stream and is offline. OBS will catch up when it
    /// is back." Always false until the OBS cue work (#132, PR #150) sets it
    /// from `SyncNode::scene_cue_waiting()`.
    pub scene_cue_waiting: bool,
    pub sync: SyncSummary,
}

/// One of the four jobs (docs/roles.md): Projection runs the screens,
/// Operator the service, Stream what goes out online, Audio the sound.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoleName {
    Projection,
    Operator,
    Stream,
    Audio,
}

/// How this device could take a role.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoleStanding {
    /// This device holds it, or is alone.
    Holding,
    /// Nobody holds it: `RoleClaim` takes it with no one's permission.
    Open,
    /// The holder is connected: `RoleAsk`.
    Ask,
    /// The holder has been offline long enough: `RoleClaim` after confirming.
    TakeOver,
    /// The holder is offline but only recently: wait `wait_secs`.
    Wait,
}

/// What keeps this device from taking a role whatever the holder says.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoleBlocked {
    /// Projection needs screens set up on this device.
    NoScreens,
    /// Operator needs somebody to hold Projection.
    NeedsProjection,
}

/// This device's own ask for a role.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RoleAskingView {
    Waiting,
    /// The holder said not now.
    Declined,
    /// Another device is already asking; `by` is its name.
    Busy {
        by: String,
    },
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoleView {
    pub role: RoleName,
    /// This device does the job.
    pub holds: bool,
    pub standing: RoleStanding,
    /// The holder's key in hex; `None` while nobody holds it.
    pub holder: Option<String>,
    /// The holder's name, empty when unknown.
    pub holder_name: String,
    /// How long the holder has been offline, when it is.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub offline_secs: Option<u64>,
    /// Seconds until takeover opens, when the holder is offline.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub wait_secs: Option<u64>,
    pub blocked: Option<RoleBlocked>,
    /// On the holder: the one device asking for the role.
    pub request: Option<PresentRequestView>,
    pub asking: Option<RoleAskingView>,
    /// This device held the role and another now does; shown once.
    pub replaced_by: Option<String>,
    /// On the holder: devices handed the role without asking.
    pub always: Vec<DriverView>,
    /// "Always allow this device" is offered (Operator and Stream only).
    pub offers_always: bool,
    /// The holder's takeover setting in seconds, when it is longer than this
    /// device's own and so decides how long this device waits to take the
    /// role over (the holder judges a claim by its own setting).
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub holder_waits_secs: Option<u64>,
}

/// Where sync with the paired devices stands, in one value for the status
/// bar.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncSummary {
    pub state: SyncState,
    /// Devices this one is paired with.
    pub paired: usize,
    /// Of those, how many are connected now.
    pub connected: usize,
    /// For `Error`: what went wrong, in words for the operator.
    pub reason: Option<String>,
    /// When changes last arrived from, and went out to, another device
    /// (unix ms); `None` since this app started.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub last_in_ms: Option<i64>,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub last_out_ms: Option<i64>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncState {
    /// Nothing is paired.
    DeviceOnly,
    /// Every paired device is connected and no changes are moving.
    Synced,
    /// Changes are moving to or from a paired device now.
    Syncing,
    /// A paired device is not connected (off, asleep or not found).
    Offline,
    /// Sync is not working; `reason` says why.
    Error,
}

/// What this device may do on screen: the presenting device drives the
/// screens and takes a service live; the others are Planning.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PresenterRole {
    Presenting,
    Planning,
}

/// How a Planning device can become the presenter.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PresenterStanding {
    /// Already presenting, or alone.
    Presenting,
    /// Nobody holds the role: `PresenterClaim` takes it with no one's
    /// permission.
    Open,
    /// The presenter is connected: `PresenterAsk`.
    Ask,
    /// The presenter has been offline long enough: `PresenterClaim` takes
    /// over without asking; say so first ("offline for 5 minutes").
    TakeOver,
    /// The presenter is offline but only recently: wait `wait_secs`.
    Wait,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresenterView {
    pub role: PresenterRole,
    pub standing: PresenterStanding,
    /// The presenting device's key in hex; `None` while nobody holds it.
    pub holder: Option<String>,
    /// The presenter's name: "Mary's laptop". This device's own when it
    /// presents.
    pub holder_name: String,
    /// Counts every claim or hand-over.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub epoch: u64,
    /// How long the presenter has been offline, when it is.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub offline_secs: Option<u64>,
    /// Seconds until takeover opens, when the presenter is offline.
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub wait_secs: Option<u64>,
    /// The takeover time on this device.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub takeover_secs: u64,
    /// Devices asking this one, the presenter, to hand over.
    pub requests: Vec<PresentRequestView>,
    /// This device's own ask, while it is open or was declined.
    pub asking: Option<AskingView>,
    /// This device was presenting and another took over; shown once.
    pub replaced_by: Option<String>,
    /// Driving: which device operates the presenter, and what this device
    /// may do about it. The presenter is still the only device that owns
    /// the screens.
    pub drive: Box<DriveView>,
}

/// A device that drives, or may, named for people.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DriverView {
    /// The device's key in hex.
    pub device: String,
    pub name: String,
}

/// Driving the presenting device from another one.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DriveView {
    /// This device is driving the presenter: its Present screen mirrors the
    /// presenter and its commands go there.
    pub driving: Option<DrivingView>,
    /// Who operates the presenting device now. On the presenter this is the
    /// truth; elsewhere it is what the presenter last said.
    pub active: Option<DriverView>,
    /// On the presenter: devices asking to drive, waiting for an answer.
    pub requests: Vec<PresentRequestView>,
    /// This device's own ask to drive, while it is open or was refused.
    pub asking: Option<DriveAskingView>,
    /// `DriveAsk` would work now: the presenter is connected and nobody
    /// has let this device drive yet.
    pub can_drive: bool,
    /// On the presenter: devices allowed without asking.
    pub always: Vec<DriverView>,
    /// The presenter took control back from this device; shown once.
    pub taken_back_by: Option<String>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrivingView {
    /// The presenter's key in hex.
    pub presenter: String,
    /// "Sanctuary PC".
    pub name: String,
    /// The presenter is connected. While it is not, the controls wait.
    pub online: bool,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DriveAskingView {
    /// Waiting for the presenter to answer.
    Waiting,
    /// The presenter said not now.
    Declined,
    /// Another device is driving; `by` is its name.
    Busy { by: String },
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresentRequestView {
    /// The asking device's key in hex.
    pub device: String,
    pub name: String,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AskingView {
    /// The presenter said not now.
    pub declined: bool,
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
    /// It holds the presenter role.
    pub presenting: bool,
    /// Why the last attempt to reach it failed, when it is not connected.
    pub problem: Option<String>,
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
        /// The translation the verses are from: the one typed after the
        /// reference, or else the one asked for.
        translation: TranslationId,
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
    /// A file added to the blob store by `AddSoundFile`.
    SoundFile(SoundFile),
    SoundStatus(Vec<SoundFileStatus>),
    Sound(SoundView),
    SoundOutputs(SoundOutputs),
    ServiceList(ServiceList),
    Outputs(Vec<OutputConfig>),
    /// The monitors connected now, as a `Display` target would match them.
    Displays(Vec<DisplayMatch>),
    /// Each browser-source output and how many OBS pages are showing it.
    BrowserSources(Vec<BrowserSourceStatus>),
    /// The service now in the live engine, split into slides.
    Loaded(LoadedService),
    Estimate(Vec<ItemEstimate>),
    SongDatabases(Vec<SongDatabase>),
    Text(String),
    Team(TeamStatus),
    /// A PNG as a `data:` URL, for an `<img>`.
    Thumbnail(String),
    DeckSlide {
        thumbnail: String,
        slides: usize,
    },
    BrandFile(BrandContents),
    Obs(ObsView),
    Settings(DeviceSettings),
    Church(ChurchStatus),
    Update(UpdateStatus),
}

/// The services, and the ones that cannot be shown.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceList {
    pub services: Vec<ServiceSummary>,
    /// Saved by a newer Lampstand than this one.
    pub newer: u32,
    /// Files that could not be loaded. `ServiceRequest::Delete` reaches them.
    pub damaged: Vec<ServiceId>,
}

/// Which sound of an item `SetSound` changes.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SoundSlot {
    /// The sound of a music-only item.
    Item,
    /// A song's backing track.
    Backing,
}

/// An audio file added to the blob store by `AddSoundFile`.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SoundFile {
    pub file: BlobHash,
    /// The file's name without its extension: "Walk-in music".
    pub name: String,
    /// How long it plays, if the file says.
    pub length_ms: Option<u32>,
}

/// Where one sound file of a service stands on this device.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoundFileStatus {
    pub item: ItemId,
    /// A song's backing track rather than a sound item.
    pub backing: bool,
    pub file: BlobHash,
    /// Whether its bytes are on this device.
    pub on_device: bool,
    /// Not here, and being fetched from the other devices: somebody is
    /// connected and the service is one that is fetched ahead.
    pub coming: bool,
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
    /// Every header and the items under it, with the loop each has now
    /// (a change made on air included).
    pub sections: Vec<SectionView>,
}

/// A header and the items up to the next header.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectionView {
    pub header: ItemId,
    pub title: String,
    /// Seconds each still slide stays up while the section loops; `None`
    /// when it does not loop.
    pub seconds: Option<u16>,
    pub items: Vec<ItemId>,
}

/// What one item puts on screen in a loop. The interval is not in here,
/// so a change of interval needs no new request.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemEstimate {
    pub item: ItemId,
    /// Slides that stay up for the interval (images, text, deck slides).
    pub stills: u32,
    /// Of those, deck slides that build on click: a loop shows them finished.
    pub click_slides: u32,
    /// The length of a video that plays through; `None` when there is no
    /// video, or its length could not be read.
    pub video_ms: Option<u32>,
    /// A video that repeats by itself, so a loop never leaves it.
    pub self_loop: bool,
    /// Something with no fixed length, such as a countdown.
    pub open_ended: bool,
    /// How long a music-only item plays; `None` for anything else, and for
    /// a sound whose length the file did not give.
    #[serde(default)]
    pub sound_ms: Option<u32>,
    /// A music-only item. The live engine skips an item with no slides, so a
    /// loop does not play it (until Audio exists), and a pass does not count
    /// it.
    #[serde(default)]
    pub sound: bool,
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
    /// The smaller line at the foot: a song's author, copyright and CCLI
    /// number, a verse's reference, or a text slide's attribution.
    #[serde(default)]
    pub footer: Option<String>,
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
    /// Connections, nearby devices or sync traffic changed. Refetch
    /// `TeamRequest::Status` for the devices themselves.
    SyncStatus {
        sync: SyncSummary,
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
    /// The monitors connected changed (one was plugged in or removed, or
    /// the arrangement changed): refetch `OutputRequest::ListDisplays`.
    DisplaysChanged,
    /// OBS's connection, streaming, recording, scenes or mutes changed.
    /// Boxed: the view is much bigger than the other events.
    Obs(Box<ObsView>),
    /// What is playing, or who plays it, changed.
    Sound(Box<SoundView>),
}
