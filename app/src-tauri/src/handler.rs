//! The app's state and its request handling, with no Tauri in it.
//!
//! `App` owns the library, the live engine, the fonts the compositor
//! measures with, the per-output composition cache and the device-local
//! output configuration. The render thread is behind `OutputSink` and the
//! output windows behind `WindowHost`, so tests drive the whole flow with a
//! recording sink and a fake window host.
//!
//! The data flow is the one in ARCHITECTURE.md ("One Sunday-morning
//! click"): a `Request::Live` command goes to `LiveEngine::apply`; when the
//! program changes, every enabled output with a window is composed at its
//! own pixel size and sent `OutputCommand::Show` (or `Animate` on a build
//! step); the UI gets `Event::Live`.
//!
//! Deck slides (T30, T84). The live engine only counts a deck slide's build
//! steps; the scene in its `Content::DeckSlide` is replaced here with the
//! slide at that step (`decks::Slide::scenes`) before composing, so any
//! recompose (a theme edit, a resize, Black) keeps what has built. A click
//! that plays the next build step sends `Animate` to each output showing
//! the step before, never a `Show`, which would cut the animation short.
//! A deck slide arrives with its own transition, not the theme's, and
//! steps that jump (Previous through builds) cut.
//!
//! Browser-source outputs (OBS, T86) go through the same compose step at
//! their configured size, but the scene is published to the [`SceneHub`]
//! the local server reads, instead of a window. Each one's URL key is kept
//! in `browser-sources.json` and given back to the hub at start, so the URL
//! pasted into OBS survives restarts. Browser sources get each deck build
//! step as a cut to the stepped slide (the page has no animations).

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use lampstand_api::{
    BrowserSourceStatus, Event, ItemEstimate, LibraryRequest, LoadedItemView, LoadedService,
    OutputRequest, Request, Response, SectionView, ServiceRequest, ServiceSummary, SettingsRequest,
    SlideView, SongDatabase, SongHit, SoundSlot, SoundView,
};
use lampstand_core::deck::{Deck, DeckId};
use lampstand_core::id::{BlobHash, Timestamp};
use lampstand_core::output::{
    idle_plan, IdlePlan, IdleScreen, OutputConfig, OutputId, OutputRole, OutputTarget,
};
use lampstand_core::program::{Bug, Content, Idle, Logo, Overlay, Program, SlideText};
use lampstand_core::safe_area::SafeArea;
use lampstand_core::scene::{Animation, Scene, Transition};
use lampstand_core::scripture::{
    find_translation, parse_reference_versioned, unrecognised_version_message,
};
use lampstand_core::service::{CountdownTarget, ItemId, ItemKind, Service, ServiceId};
use lampstand_core::text::text_slides;
use lampstand_core::theme::{Theme, ThemeId};
use lampstand_layout::{
    compose_at, paginate_passage, paginate_song, Fonts, OutputSize, PassageSplit,
};
use lampstand_library::Library;
use lampstand_live::{Command, Cursor, LiveEngine, LoadedItem, LoadedSlide, LoopGroup};
use lampstand_render::output::{assign_displays, DisplayInfo};
use lampstand_render::{CalibrationPattern, OutputCommand, OutputSink};
use lampstand_server::SceneHub;
use lampstand_sync::DocChanged;

use crate::audio::{AudioDeck, Parts};
use crate::config::{device_key, KeyStore, OutputStore, SettingsStore};

mod audio;
mod church;
mod sound;

use crate::decks::{self, ParsedSlides};
use crate::import::import_files;
use crate::journal::{Journal, LiveRecord};
use crate::keys::KeyState;
use crate::obs_control::{SceneCue, SceneCues};
use crate::seed;
pub use audio::{problem_words, AUDIO_AWAY, AUDIO_NOBODY, AUDIO_NOT_ALLOWED, FORMATS};
use church::ChurchSync;
pub use church::{AudioRemote, SyncLink};
use sound::SoundSync;

/// Where an output window goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    /// A normal, decorated window of this inner size (physical pixels).
    Window { width: u32, height: u32 },
    /// Borderless and fullscreen on this display.
    Display(DisplayInfo),
}

/// Creates and destroys output windows. The Tauri implementation lives in
/// `shell`; tests use a fake.
pub trait WindowHost: Send {
    /// The monitors connected now.
    fn displays(&self) -> Vec<DisplayInfo>;
    /// Opens a hidden window for `output` and attaches it to the render
    /// thread; it shows itself on its first frame. Opening an output that
    /// is already open moves it to `placement`.
    fn open(&mut self, output: OutputId, title: &str, placement: Placement) -> Result<()>;
    /// Detaches and destroys the window, if there is one.
    fn close(&mut self, output: OutputId);
}

pub type EventSink = Arc<dyn Fn(Event) + Send + Sync>;

/// What an output was last sent, so an unchanged output is not composed
/// again.
#[derive(PartialEq)]
struct Sent {
    program: Program,
    size: (u32, u32),
    theme: Theme,
    config: OutputConfig,
    /// Whole seconds a countdown shows on the slide and in the stage
    /// "next", which the program alone does not change.
    countdown: (Option<i64>, Option<i64>),
}

/// What the UI was last told about the live state.
type LiveState = (Option<Cursor>, Option<Cursor>, bool, Overlay);

/// The whole seconds a countdown shows now, rounded up as it is drawn.
fn countdown_seconds(content: &Option<Content>, now: Timestamp) -> Option<i64> {
    match content {
        Some(Content::Countdown { target, .. }) => Some(((target.0 - now.0).max(0) + 999) / 1000),
        _ => None,
    }
}

struct Loaded {
    view: LoadedService,
    /// Build-step animations by (item, slide), each a list per step.
    builds: HashMap<(ItemId, usize), Vec<Vec<Animation>>>,
    /// The songs in the service, so edits to them reload it.
    songs: Vec<lampstand_core::song::SongId>,
    /// The decks in the service, so an accepted update reloads it.
    decks: Vec<DeckId>,
    /// Each deck slide's scene after each build step by (item, slide),
    /// for thumbnails.
    scenes: HashMap<(ItemId, usize), Vec<Arc<Scene>>>,
    /// The parsed decks the service shows, for stepped scenes, entry
    /// animations and transitions.
    slides: HashMap<DeckId, Arc<ParsedSlides>>,
    /// Each item's OBS scene, for `SceneCue`s (T90).
    obs_scenes: HashMap<ItemId, String>,
}

pub struct App {
    library: Arc<Library>,
    live: LiveEngine,
    fonts: Fonts,
    outputs: OutputStore,
    settings: SettingsStore,
    /// Bookkeeping for the settings shared with the church's other devices.
    church_sync: ChurchSync,
    /// Bookkeeping for sound files that travel by sync.
    sound_sync: SoundSync,
    /// What plays on this device when it holds Audio.
    audio: AudioDeck,
    /// The sound state the screen was last told.
    told_sound: Option<SoundView>,
    /// This device held Audio at the last look (to notice it coming here).
    audio_held: bool,
    sink: Arc<dyn OutputSink>,
    windows: Box<dyn WindowHost>,
    emit: EventSink,
    /// Where each open output window was asked to go. A window is opened
    /// again only when this changes, never because its real size differs.
    placed: HashMap<OutputId, Placement>,
    /// The pixel size each output with a window is composed at: the
    /// placement's size until the window reports its real one.
    sizes: HashMap<OutputId, (u32, u32)>,
    /// Enabled browser-source outputs, each with the size it is composed
    /// at. They have no window; their scenes go to `hub`.
    streams: HashMap<OutputId, (u32, u32)>,
    hub: Arc<SceneHub>,
    /// Whether any enabled output streams, for OBS warnings (T89): read
    /// by `obs_control` without the `App` lock.
    streaming: Arc<AtomicBool>,
    /// Where an item going live sends its OBS scene (T90), once the shell
    /// has started `obs_control`.
    obs_cues: Option<SceneCues>,
    /// Each browser-source output's URL key.
    keys: KeyStore,
    /// `host:port` of the local server, for browser-source URLs; `None`
    /// until the shell has started it.
    server: Option<String>,
    sent: HashMap<OutputId, Sent>,
    told_ui: Option<LiveState>,
    loaded: Option<Loaded>,
    /// Loops changed on air, by section header, for the open service only.
    loop_overrides: HashMap<ItemId, Option<u16>>,
    /// Video lengths already read, by blob, in milliseconds.
    video_lengths: HashMap<BlobHash, Option<u32>>,
    /// Logo shapes already read, by blob: width over height.
    logo_aspects: HashMap<BlobHash, Option<f32>>,
    /// Parsed decks by the blob they were parsed from, for the decks in
    /// the open service.
    parsed: HashMap<BlobHash, Arc<ParsedSlides>>,
    /// What is on air, for crash recovery (T57).
    journal: Journal,
    /// Set from `open` until the journal is restored, so the windows
    /// being placed and the half-restored state do not overwrite it.
    restoring: bool,
    /// This device may drive the screens: it is the presenter, or has no
    /// paired devices. Set by the shell as sync reports it; a Planning
    /// device refuses live control and keeps its screens dark.
    presenting: bool,
    /// The displays the windows were last placed against, so a display that
    /// appears after that, even before the shell's first look, is noticed.
    seen_displays: Vec<DisplayInfo>,
}

/// Why a Planning device refuses live control.
pub const PLANNING_ONLY: &str =
    "This device is planning. Ask the presenting device to hand over, then take the service live.";

type Result<T> = std::result::Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl App {
    /// Opens (or creates) everything under `data_dir`: the library in
    /// `library/`, the device key, `outputs.json` and
    /// `browser-sources.json`. On first run the seed hymns are imported.
    /// Enabled outputs get their windows, or their browser-source URL, and
    /// a service that was live less than 10 minutes ago (the crash journal,
    /// `journal.json`) is reopened on the same slide.
    pub fn open(
        data_dir: &Path,
        fonts: Fonts,
        sink: Arc<dyn OutputSink>,
        windows: Box<dyn WindowHost>,
        emit: EventSink,
    ) -> Result<Self> {
        std::fs::create_dir_all(data_dir).map_err(err)?;
        let key = device_key(data_dir).map_err(err)?;
        let library = Arc::new(Library::open(&data_dir.join("library"), key).map_err(err)?);
        seed::seed_if_new(&library, data_dir).map_err(err)?;
        let outputs = OutputStore::load(data_dir.join("outputs.json")).map_err(err)?;
        let keys = KeyStore::load(data_dir.join("browser-sources.json")).map_err(err)?;
        let settings = SettingsStore::load(data_dir.join("settings.json")).map_err(err)?;
        let audio = AudioDeck::new(
            Parts::real(library.clone()),
            settings.get().sound_output.clone(),
        );
        let mut app = Self {
            library,
            live: LiveEngine::new(),
            fonts,
            outputs,
            settings,
            church_sync: ChurchSync::default(),
            sound_sync: SoundSync::default(),
            audio,
            told_sound: None,
            audio_held: false,
            sink,
            windows,
            emit,
            placed: HashMap::new(),
            sizes: HashMap::new(),
            streams: HashMap::new(),
            hub: Arc::new(SceneHub::new()),
            streaming: Arc::new(AtomicBool::new(false)),
            obs_cues: None,
            keys,
            server: None,
            sent: HashMap::new(),
            told_ui: None,
            loaded: None,
            loop_overrides: HashMap::new(),
            video_lengths: HashMap::new(),
            logo_aspects: HashMap::new(),
            parsed: HashMap::new(),
            journal: Journal::new(data_dir),
            restoring: true,
            presenting: true,
            seen_displays: Vec::new(),
        };
        app.place_windows();
        app.restore();
        Ok(app)
    }

    /// Shared, so long imports can run without holding the `App` lock.
    pub fn library(&self) -> &Arc<Library> {
        &self.library
    }

    /// Where pushed events go.
    pub fn emitter(&self) -> EventSink {
        self.emit.clone()
    }

    /// Which keys to take from the system (T56): clicker mode, and
    /// whether a service is open for live. The shell adds window focus.
    pub fn key_state(&self) -> KeyState {
        KeyState {
            clicker_mode: self.settings.get().clicker_mode,
            live: self.loaded.is_some(),
            output_focused: false,
        }
    }

    pub fn program(&self) -> &Program {
        self.live.program()
    }

    /// The browser sources' keys and scenes, for the local server's
    /// `Backend` to delegate to.
    pub fn scene_hub(&self) -> &Arc<SceneHub> {
        &self.hub
    }

    /// True while an enabled browser-source output exists. Shared with
    /// `obs_control`, which mentions OBS to the operator only then (or once
    /// OBS settings are saved).
    pub fn streaming(&self) -> Arc<AtomicBool> {
        self.streaming.clone()
    }

    /// Where items going live send their OBS scenes (T90).
    pub fn set_obs_cues(&mut self, cues: SceneCues) {
        self.obs_cues = Some(cues);
    }

    /// Sync says whether this device may drive the screens now. Becoming
    /// Planning takes the service off air and closes the output windows:
    /// they stay dark (the outputs keep their settings) until this device
    /// presents again.
    pub fn set_presenting(&mut self, presenting: bool) {
        if self.presenting == presenting {
            return;
        }
        self.presenting = presenting;
        if !presenting {
            self.loaded = None;
            self.loop_overrides.clear();
            let events = self.live.apply(Command::EndService);
            self.live.load(Vec::new());
            self.after_live(events);
        }
        self.place_windows();
        if !presenting {
            self.journal.clear();
        }
    }

    /// Where the local server listens (`host:port`), for the URLs
    /// `BrowserSourceUrl` gives.
    pub fn set_server_address(&mut self, address: String) {
        self.server = Some(address);
    }

    pub fn handle(&mut self, request: Request) -> Response {
        let result = match request {
            Request::Library(r) => self.library_request(r),
            Request::Service(r) => self.service_request(r),
            Request::Live(command) => {
                // Putting a slide on the preview pane changes nothing on air.
                if !self.presenting && !matches!(command, Command::Preview(_)) {
                    return Response::Error {
                        message: PLANNING_ONLY.into(),
                    };
                }
                let events = self.live.apply(command);
                self.after_live(events);
                Ok(Response::Ok)
            }
            Request::Outputs(r) => self.output_request(r),
            Request::Sound(r) => self.sound_request(r),
            // The shell answers these from `crate::sync` and
            // `crate::obs_control`, without the lock.
            Request::Team(_) => Err("sync is not running".into()),
            Request::Settings(SettingsRequest::Get) => {
                Ok(Response::Settings(self.device_settings()))
            }
            Request::Settings(SettingsRequest::Church) => {
                Ok(Response::Church(self.church_status()))
            }
            Request::Settings(SettingsRequest::ResolveChurch(choice)) => self
                .resolve_church(choice)
                .map(|()| Response::Church(self.church_status())),
            Request::Settings(SettingsRequest::Put(settings)) => self.put_settings(settings),
            Request::Settings(SettingsRequest::SaveBrand(brand)) => self.save_brand(brand),
            Request::Settings(SettingsRequest::SetLogoBug(bug)) => self.set_logo_bug(bug),
            Request::Obs(_) => Err("OBS control is not running".into()),
            // The shell answers updates: it owns the updater.
            Request::Update(_) => Err("updates are not available here".into()),
            // The shell opens the browser.
            Request::About(_) => Err("the browser is opened by the app shell".into()),
        };
        result.unwrap_or_else(|message| Response::Error { message })
    }

    fn library_request(&mut self, request: LibraryRequest) -> Result<Response> {
        let lib = &self.library;
        Ok(match request {
            LibraryRequest::SearchSongs { query, limit } => {
                let hits = if query.trim().is_empty() {
                    // An empty search lists the library, by title.
                    let mut songs = lib.songs().map_err(err)?;
                    songs.sort_by_key(|s| s.title.to_lowercase());
                    songs
                        .into_iter()
                        .take(limit)
                        .map(|s| SongHit {
                            song: s.id,
                            title: s.title,
                            line: None,
                        })
                        .collect()
                } else {
                    lib.search_songs(&query, limit)
                        .map_err(err)?
                        .into_iter()
                        .map(|h| SongHit {
                            song: h.song,
                            title: h.title,
                            line: h.line,
                        })
                        .collect()
                };
                Response::Songs(hits)
            }
            LibraryRequest::GetSong(id) => match lib.song(id).map_err(err)? {
                Some(song) => Response::Song(song),
                None => return Err("no such song".into()),
            },
            LibraryRequest::PutSong(song) => {
                lib.put_song(&song).map_err(err)?;
                Response::Ok
            }
            LibraryRequest::ListThemes => Response::Themes(lib.themes().map_err(err)?),
            LibraryRequest::PutTheme(theme) => {
                lib.put_theme(&theme).map_err(err)?;
                Response::Ok
            }
            LibraryRequest::DeleteTheme(id) => {
                lib.remove_theme(id).map_err(err)?;
                // Anything that named it now falls back.
                self.settings_forget_theme(id)?;
                Response::Ok
            }
            LibraryRequest::RestoreStarterTheme => {
                lib.restore_starter_theme().map_err(err)?;
                Response::Ok
            }
            LibraryRequest::ExportBrand { path } => Response::Text(self.export_brand(&path)?),
            LibraryRequest::InspectBrand { path } => {
                Response::BrandFile(self.inspect_brand(&path)?)
            }
            LibraryRequest::ImportBrand { path, mode } => {
                Response::Text(self.import_brand(&path, mode)?)
            }
            LibraryRequest::ThemePreview { .. } | LibraryRequest::IdlePreview { .. } => {
                // It needs the GPU, so the shell draws it.
                return Err("theme previews are drawn by the app shell".into());
            }
            LibraryRequest::ListTranslations => {
                Response::Translations(lib.translations().map_err(err)?)
            }
            LibraryRequest::LookupPassage {
                reference,
                translation,
            } => {
                let (passage, typed) =
                    parse_reference_versioned(&reference).map_err(|e| format!("{e:?}"))?;
                let translation = match typed {
                    Some(typed) => {
                        let all = lib.translations().map_err(err)?;
                        find_translation(&all, &typed)
                            .map(|t| t.id)
                            .ok_or_else(|| unrecognised_version_message(&typed, &all))?
                    }
                    None => translation,
                };
                let text = lib
                    .bible_text(translation)
                    .map_err(err)?
                    .ok_or("that translation is not on this machine")?;
                let verses = text
                    .verses(&passage)
                    .map_err(|_| format!("{passage} is not in this translation"))?
                    .into_iter()
                    .map(|(cv, t)| (cv.chapter, cv.verse, t.to_string()))
                    .collect();
                Response::Passage {
                    display: passage.to_string(),
                    passage,
                    translation,
                    verses,
                }
            }
            LibraryRequest::ImportFiles { paths } => {
                Response::Text(import_files(lib, &paths, &*self.emit))
            }
            LibraryRequest::ListSongDatabases => Response::SongDatabases(song_databases()),
            LibraryRequest::DownloadSongDatabase { .. } => {
                // It needs the network and the async runtime, so the shell
                // downloads it and then imports as `ImportFiles` does.
                return Err("song databases are downloaded by the app shell".into());
            }
            LibraryRequest::ImportSongSelectDownloads { .. } => {
                // It needs the user's Downloads folder, which the shell knows.
                return Err("SongSelect downloads are imported by the app shell".into());
            }
            LibraryRequest::ListDecks => {
                let mut decks = lib.decks().map_err(err)?;
                decks.sort_by_key(|d| d.title.to_lowercase());
                Response::Decks(decks)
            }
            LibraryRequest::AddDeckFile { path } => {
                Response::Deck(decks::add_deck_file(lib, &path)?)
            }
            LibraryRequest::AddDeckFromLink { .. } => {
                // A download, like a song database: the shell fetches it
                // and then adds it as `AddDeckFile` does.
                return Err("deck links are downloaded by the app shell".into());
            }
            LibraryRequest::AcceptDeckUpdate(_) => {
                return Err("linked deck updates arrive with T41".into())
            }
            LibraryRequest::AddMediaFile { path } => {
                Response::Media(decks::add_media_file(lib, &path)?)
            }
            LibraryRequest::AddSoundFile { path } => {
                Response::SoundFile(decks::add_sound_file(lib, &path)?)
            }
            LibraryRequest::AddSoundFromLink { .. } => {
                // A download, like a deck link: the shell fetches it and
                // then adds it as `AddSoundFile` does.
                return Err("sound links are downloaded by the app shell".into());
            }
            LibraryRequest::DeckThumbnail { .. } => {
                // It needs the GPU, so the shell draws it.
                return Err("deck thumbnails are drawn by the app shell".into());
            }
            LibraryRequest::MediaThumbnail { blob, name, width } => {
                // Decoding a video takes a moment, so the shell runs it off
                // the `App` lock; this is the same work, for callers without one.
                Response::Thumbnail(decks::media_thumbnail(lib, &blob, &name, width)?)
            }
            LibraryRequest::DeckSlidePicture { .. } => {
                return Err("deck pictures are drawn by the app shell".into());
            }
            LibraryRequest::ListMedia => Response::MediaFiles(decks::list_media(lib)?),
        })
    }

    fn service_request(&mut self, request: ServiceRequest) -> Result<Response> {
        let lib = &self.library;
        let get = |id: ServiceId| -> Result<Service> {
            lib.service(id)
                .map_err(err)?
                .ok_or("no such service".into())
        };
        Ok(match request {
            ServiceRequest::List => Response::Services(summaries(lib.services().map_err(err)?)),
            ServiceRequest::ListChecked => {
                let scan = lib.services_checked().map_err(err)?;
                Response::ServiceList(lampstand_api::ServiceList {
                    services: summaries(scan.services),
                    newer: scan.newer as u32,
                    damaged: scan.damaged,
                })
            }
            ServiceRequest::Get(id) => Response::Service(get(id)?),
            ServiceRequest::Create {
                title,
                starts_at,
                time_zone,
            } => {
                let mut service = Service::new(title);
                service.starts_at = starts_at;
                service.time_zone = time_zone;
                lib.put_service(&service).map_err(err)?;
                Response::Service(service)
            }
            ServiceRequest::Duplicate {
                service,
                title,
                starts_at,
                time_zone,
            } => {
                let title = title.trim();
                if title.is_empty() {
                    return Err("a service needs a name".into());
                }
                let mut copy = get(service)?.duplicate(title);
                copy.starts_at = starts_at;
                copy.time_zone = time_zone;
                lib.put_service(&copy).map_err(err)?;
                Response::Service(copy)
            }
            ServiceRequest::CreateSample {
                title,
                starts_at,
                time_zone,
            } => {
                let mut sample = seed::sample_service(lib)?;
                let title = title.trim();
                if !title.is_empty() {
                    sample.title = title.to_string();
                }
                sample.starts_at = starts_at;
                sample.time_zone = time_zone;
                lib.put_service(&sample).map_err(err)?;
                Response::Service(sample)
            }
            ServiceRequest::Reschedule {
                service,
                starts_at,
                time_zone,
            } => {
                let mut s = get(service)?;
                s.starts_at = starts_at;
                s.time_zone = time_zone;
                // The start time is not on any slide, so an open service
                // needs no reload.
                lib.put_service(&s).map_err(err)?;
                Response::Service(s)
            }
            ServiceRequest::Rename { service, title } => {
                let title = title.trim();
                if title.is_empty() {
                    return Err("a service needs a name".into());
                }
                let mut s = get(service)?;
                s.title = title.to_string();
                lib.put_service(&s).map_err(err)?;
                self.reload_if_open(service)?;
                Response::Service(s)
            }
            ServiceRequest::Delete(service) => {
                get(service)?;
                // A service open for live is never deleted from under the
                // operator: it is closed first, on purpose.
                if self.loaded.as_ref().map(|l| l.view.service) == Some(service) {
                    return Err(
                        "This service is open for live. Close it before deleting it.".into(),
                    );
                }
                self.library.delete_service(service).map_err(err)?;
                Response::Ok
            }
            ServiceRequest::Close(service) => {
                self.close_service(service);
                Response::Ok
            }
            ServiceRequest::InsertItem {
                service,
                index,
                kind,
            } => {
                let mut s = get(service)?;
                s.insert(index, kind);
                lib.put_service(&s).map_err(err)?;
                self.reload_if_open(service)?;
                Response::Service(s)
            }
            ServiceRequest::MoveItem {
                service,
                item,
                index,
            } => {
                let mut s = get(service)?;
                if !s.move_item(item, index) {
                    return Err("no such item".into());
                }
                lib.put_service(&s).map_err(err)?;
                self.reload_if_open(service)?;
                Response::Service(s)
            }
            ServiceRequest::RemoveItem { service, item } => {
                let mut s = get(service)?;
                if s.remove(item).is_none() {
                    return Err("no such item".into());
                }
                lib.put_service(&s).map_err(err)?;
                self.reload_if_open(service)?;
                Response::Service(s)
            }
            ServiceRequest::SetTheme { service, theme } => {
                let mut s = get(service)?;
                s.theme = theme;
                lib.put_service(&s).map_err(err)?;
                self.reload_if_open(service)?;
                self.present();
                Response::Service(s)
            }
            ServiceRequest::SetObsScene {
                service,
                item,
                scene,
            } => {
                let mut s = get(service)?;
                let entry = s.items.get_mut(&item).ok_or("no such item")?;
                entry.obs_scene = scene
                    .map(|name| name.trim().to_string())
                    .filter(|name| !name.is_empty());
                lib.put_service(&s).map_err(err)?;
                self.reload_if_open(service)?;
                Response::Service(s)
            }
            ServiceRequest::SetItemArrangement {
                service,
                item,
                arrangement,
            } => {
                let mut s = get(service)?;
                let entry = s.items.get_mut(&item).ok_or("no such item")?;
                let ItemKind::Song {
                    arrangement: slot, ..
                } = &mut entry.kind
                else {
                    return Err("only a song has an order".into());
                };
                *slot = arrangement
                    .map(|labels| {
                        labels
                            .into_iter()
                            .map(|l| l.trim().to_string())
                            .filter(|l| !l.is_empty())
                            .collect::<Vec<_>>()
                    })
                    .filter(|labels| !labels.is_empty());
                lib.put_service(&s).map_err(err)?;
                self.reload_if_open(service)?;
                Response::Service(s)
            }
            ServiceRequest::SetItemText {
                service,
                item,
                title,
                body,
                attribution,
            } => {
                let mut s = get(service)?;
                match s.items.get_mut(&item).map(|i| &mut i.kind) {
                    Some(ItemKind::Text {
                        title: t,
                        body: b,
                        attribution: a,
                    }) => {
                        *t = title;
                        *b = body;
                        *a = attribution
                            .map(|x| x.trim().to_string())
                            .filter(|x| !x.is_empty());
                    }
                    _ => return Err("that item is not a text slide".into()),
                }
                lib.put_service(&s).map_err(err)?;
                self.reload_if_open(service)?;
                Response::Service(s)
            }
            ServiceRequest::SetLoop {
                service,
                item,
                seconds,
            } => {
                let mut s = get(service)?;
                let entry = s.items.get_mut(&item).ok_or("no such item")?;
                entry.loop_seconds = seconds.filter(|n| *n > 0);
                lib.put_service(&s).map_err(err)?;
                self.reload_if_open(service)?;
                Response::Service(s)
            }
            ServiceRequest::Open(_) if !self.presenting => return Err(PLANNING_ONLY.into()),
            ServiceRequest::Open(id) => {
                if self.loaded.as_ref().map(|l| l.view.service) != Some(id) {
                    self.loop_overrides.clear();
                }
                Response::Loaded(self.load_service(id)?)
            }
            ServiceRequest::SetMediaLooped {
                service,
                item,
                looped,
            } => {
                let mut s = get(service)?;
                match s.items.get_mut(&item).map(|i| &mut i.kind) {
                    Some(ItemKind::Media { looped: l, .. }) => *l = looped,
                    _ => return Err("that item is not a media file".into()),
                }
                lib.put_service(&s).map_err(err)?;
                self.reload_if_open(service)?;
                Response::Service(s)
            }
            ServiceRequest::SetSound {
                service,
                item,
                slot,
                sound,
            } => {
                let mut s = get(service)?;
                let entry = s.items.get_mut(&item).ok_or("no such item")?;
                let sound = sound.map(|mut sound| {
                    sound.name = sound.name.trim().to_string();
                    sound
                });
                if sound.as_ref().is_some_and(|sound| sound.name.is_empty()) {
                    return Err("Give the sound a name.".into());
                }
                match (slot, sound) {
                    (SoundSlot::Item, Some(sound)) => match &mut entry.kind {
                        ItemKind::Sound { sound: slot } => *slot = sound,
                        _ => return Err("That item is not a music-only item.".into()),
                    },
                    (SoundSlot::Item, None) => {
                        return Err("A music-only item needs its sound.".into())
                    }
                    (SoundSlot::Backing, sound) => {
                        if !matches!(entry.kind, ItemKind::Song { .. }) {
                            return Err("Only a song can have a backing track.".into());
                        }
                        entry.backing = sound;
                    }
                }
                lib.put_service(&s).map_err(err)?;
                self.reload_if_open(service)?;
                // A new file may need fetching from the other devices.
                self.sound_sync.service_changed(service.uuid());
                Response::Service(s)
            }
            ServiceRequest::SoundStatus(service) => {
                Response::SoundStatus(self.sound_status(service)?)
            }
            ServiceRequest::SetLiveLoop { header, seconds } => {
                let Some(service) = self.loaded.as_ref().map(|l| l.view.service) else {
                    return Err("no service is open for live".into());
                };
                self.loop_overrides.insert(header, seconds);
                self.load_service(service)?;
                Response::Ok
            }
            ServiceRequest::Estimate(service) => Response::Estimate(self.estimate(service)?),
            ServiceRequest::SlideThumbnail { .. } => {
                // It needs a GPU, so the shell draws it from `slide_scene`.
                return Err("thumbnails are drawn by the app shell".into());
            }
            ServiceRequest::Readiness(_) => {
                return Err("the readiness check arrives with T36".into())
            }
            ServiceRequest::ExportPackage { .. } | ServiceRequest::ImportPackage { .. } => {
                return Err("service packages arrive with T37".into())
            }
        })
    }

    fn output_request(&mut self, request: OutputRequest) -> Result<Response> {
        Ok(match request {
            OutputRequest::ListDisplays => Response::Displays(
                self.windows
                    .displays()
                    .iter()
                    .map(DisplayInfo::to_match)
                    .collect(),
            ),
            OutputRequest::List => Response::Outputs(self.outputs.all().to_vec()),
            OutputRequest::Put(config) => {
                let id = config.id;
                self.outputs.put(config).map_err(err)?;
                self.sent.remove(&id);
                self.place_windows();
                self.tell_screens();
                Response::Outputs(self.outputs.all().to_vec())
            }
            OutputRequest::Remove(id) => {
                self.outputs.remove(id).map_err(err)?;
                self.keys.remove(id).map_err(err)?;
                self.place_windows();
                self.tell_screens();
                Response::Outputs(self.outputs.all().to_vec())
            }
            OutputRequest::Calibrate { output, active } => {
                let config = self.outputs.get(output).ok_or("no such output")?;
                let pattern = active.then_some(CalibrationPattern {
                    current: config.safe_area,
                    max_percent: 10,
                    active: None,
                    label: config.name.clone(),
                });
                self.sink.send(output, OutputCommand::Calibrate(pattern));
                Response::Ok
            }
            OutputRequest::SetSafeArea { output, safe_area } => {
                let mut config = self.outputs.get(output).ok_or("no such output")?.clone();
                config.safe_area = safe_area;
                self.outputs.put(config).map_err(err)?;
                self.present();
                Response::Ok
            }
            OutputRequest::BrowserSourceUrl(output) => {
                Response::Text(self.browser_source_address(output)?)
            }
            OutputRequest::BrowserSourceStatus => Response::BrowserSources(
                self.outputs
                    .all()
                    .iter()
                    .filter(|o| matches!(o.target, OutputTarget::BrowserSource { .. }))
                    .map(|o| BrowserSourceStatus {
                        output: o.id,
                        clients: self.hub.watcher_count(o.id) as u32,
                    })
                    .collect(),
            ),
            OutputRequest::RotateBrowserSourceKey(output) => {
                self.browser_source(output)?;
                let key = self.hub.rotate_key(output).unwrap_or_else(fresh_key);
                self.keys.put(output, &key).map_err(err)?;
                Response::Text(self.browser_source_url(output, &key)?)
            }
        })
    }

    /// A browser-source output's URL, making its key if it has none.
    fn browser_source_address(&mut self, output: OutputId) -> Result<String> {
        self.browser_source(output)?;
        let key = match self.keys.get(output) {
            Some(key) => key.to_string(),
            None => {
                let key = fresh_key();
                self.keys.put(output, &key).map_err(err)?;
                key
            }
        };
        self.browser_source_url(output, &key)
    }

    /// What "Set up OBS" puts in OBS for `output`: its URL as the local
    /// server has it now (so it follows the address the shell watches) and
    /// its size. The caller may then swap in a pinned address.
    pub fn obs_picture(&mut self, output: OutputId) -> Result<(String, u32, u32)> {
        let url = self.browser_source_address(output)?;
        match self.outputs.get(output).map(|o| &o.target) {
            Some(OutputTarget::BrowserSource { width, height }) => Ok((url, *width, *height)),
            _ => Err("that output is not an OBS browser source".into()),
        }
    }

    /// Checks that `output` is a browser-source output.
    fn browser_source(&self, output: OutputId) -> Result<()> {
        match self.outputs.get(output).map(|o| &o.target) {
            Some(OutputTarget::BrowserSource { .. }) => Ok(()),
            Some(_) => Err("that output is not an OBS browser source".into()),
            None => Err("no such output".into()),
        }
    }

    fn browser_source_url(&self, output: OutputId, key: &str) -> Result<String> {
        let server = self
            .server
            .as_ref()
            .ok_or("the local server is not running, so OBS cannot connect")?;
        Ok(format!(
            "http://{server}/obs/{}?k={key}",
            output.uuid().hyphenated()
        ))
    }

    // ---- Live --------------------------------------------------------

    /// A video's length in milliseconds, read once from its file.
    fn video_length(&mut self, blob: &BlobHash) -> Option<u32> {
        if let Some(known) = self.video_lengths.get(blob) {
            return *known;
        }
        let length = self
            .library
            .repo()
            .blobs()
            .path(blob)
            .and_then(|path| lampstand_media::duration(&path))
            .map(|seconds| (seconds * 1000.0).round() as u32);
        // A file that has not reached this device may yet; do not remember that.
        if length.is_some() || self.library.repo().blobs().path(blob).is_some() {
            self.video_lengths.insert(*blob, length);
        }
        length
    }

    /// What every item of `service` puts on screen in a loop.
    fn estimate(&mut self, service: ServiceId) -> Result<Vec<ItemEstimate>> {
        let service = self
            .library
            .service(service)
            .map_err(err)?
            .ok_or("no such service")?;
        let primary = self.primary_output();
        let theme = self.pagination_theme(service.theme, &primary);
        let mut out = Vec::new();
        for (id, item) in service.ordered() {
            if matches!(item.kind, ItemKind::Header { .. }) {
                continue;
            }
            let loaded = self.load_item(id, &item.kind, &theme, &primary)?;
            let mut e = ItemEstimate {
                item: id,
                stills: 0,
                click_slides: 0,
                video_ms: None,
                self_loop: false,
                open_ended: false,
                sound_ms: None,
                sound: false,
            };
            if let ItemKind::Sound { sound } = &item.kind {
                // Reported, but not part of a loop's length: nothing plays it.
                e.sound = true;
                e.sound_ms = sound.length_ms;
            }
            for slide in &loaded.slides {
                match &slide.content {
                    Content::Video { blob, looped } => {
                        e.self_loop |= *looped;
                        e.video_ms = Some(self.video_length(blob).unwrap_or(0));
                    }
                    Content::Countdown { .. } => e.open_ended = true,
                    _ => {
                        e.stills += 1;
                        if slide.steps > 0 {
                            e.click_slides += 1;
                        }
                    }
                }
            }
            out.push(e);
        }
        Ok(out)
    }

    /// Takes `service` off air and unloads it, if it is the open one.
    fn close_service(&mut self, service: ServiceId) {
        if self.loaded.as_ref().map(|l| l.view.service) != Some(service) {
            return;
        }
        self.loaded = None;
        self.loop_overrides.clear();
        let events = self.live.apply(Command::EndService);
        self.live.load(Vec::new());
        self.after_live(events);
    }

    fn reload_if_open(&mut self, service: ServiceId) -> Result<()> {
        if self.loaded.as_ref().map(|l| l.view.service) == Some(service) {
            self.load_service(service)?;
        }
        Ok(())
    }

    /// Splits the service into slides and loads it into the live engine.
    /// Reloading the open service keeps what is on air (the engine's rule).
    fn load_service(&mut self, id: ServiceId) -> Result<LoadedService> {
        let service = self
            .library
            .service(id)
            .map_err(err)?
            .ok_or("no such service")?;
        let primary = self.primary_output();
        let theme = self.pagination_theme(service.theme, &primary);
        let mut items = Vec::new();
        let mut views = Vec::new();
        let mut builds = HashMap::new();
        let mut scenes = HashMap::new();
        let mut decks = Vec::new();
        let mut blobs = Vec::new();
        let mut parsed_decks = HashMap::new();
        let mut obs_scenes = HashMap::new();
        // The section a header's loop covers, until the next header.
        let mut section: Option<LoopGroup> = None;
        let mut sections: Vec<SectionView> = Vec::new();
        for (item_id, item) in service.ordered() {
            if let ItemKind::Header { .. } = item.kind {
                let seconds = match self.loop_overrides.get(&item_id) {
                    Some(over) => *over,
                    None => item.loop_seconds,
                };
                section = seconds.filter(|n| *n > 0).map(|n| LoopGroup {
                    id: item_id,
                    interval_ms: u32::from(n) * 1000,
                });
                sections.push(SectionView {
                    header: item_id,
                    title: match &item.kind {
                        ItemKind::Header { title } => title.clone(),
                        _ => String::new(),
                    },
                    seconds: seconds.filter(|n| *n > 0),
                    items: Vec::new(),
                });
            }
            if let Some(scene) = item.obs_scene.as_ref().filter(|s| !s.trim().is_empty()) {
                obs_scenes.insert(item_id, scene.clone());
            }
            let mut loaded = self.load_item(item_id, &item.kind, &theme, &primary)?;
            if !matches!(item.kind, ItemKind::Header { .. }) {
                if let Some(open) = sections.last_mut() {
                    open.items.push(item_id);
                }
                loaded.loop_group = section;
                if section.is_some() {
                    // A video plays through once, then the loop moves on.
                    for slide in &mut loaded.slides {
                        if let Content::Video {
                            blob,
                            looped: false,
                        } = &slide.content
                        {
                            slide.dwell_ms = self.video_length(blob);
                        }
                    }
                }
                if let Some(n) = item.loop_seconds.filter(|n| *n > 0) {
                    loaded.auto_advance_ms = Some(u32::from(n) * 1000);
                }
            }
            if let ItemKind::Deck { deck } = &item.kind {
                decks.push(*deck);
                let parsed = self
                    .library
                    .deck(*deck)
                    .ok()
                    .flatten()
                    .and_then(|d| self.parsed.get(&d.current.blob).cloned().map(|p| (d, p)));
                if let Some((d, parsed)) = parsed {
                    blobs.push(d.current.blob);
                    for (i, slide) in parsed.slides.iter().enumerate() {
                        if !slide.builds.is_empty() {
                            builds.insert((item_id, i), slide.builds.clone());
                        }
                        scenes.insert((item_id, i), slide.scenes.clone());
                    }
                    parsed_decks.insert(*deck, parsed);
                }
            }
            views.push(LoadedItemView {
                id: item_id,
                title: loaded.title.clone(),
                slides: loaded.slides.iter().map(slide_view).collect(),
            });
            items.push(LoadedItem {
                theme: service.theme,
                ..loaded
            });
        }
        let view = LoadedService {
            service: id,
            title: service.title.clone(),
            items: views,
            sections,
        };
        // Parsed decks no service uses any more are let go.
        self.parsed.retain(|blob, _| blobs.contains(blob));
        self.loaded = Some(Loaded {
            view: view.clone(),
            builds,
            songs: items.iter().filter_map(|i| i.song).collect(),
            decks,
            scenes,
            slides: parsed_decks,
            obs_scenes,
        });
        let events = self.live.load(items);
        (self.emit)(Event::Loaded(view.clone()));
        self.after_live(events);
        Ok(view)
    }

    fn load_item(
        &mut self,
        id: ItemId,
        kind: &ItemKind,
        theme: &Theme,
        primary: &OutputConfig,
    ) -> Result<LoadedItem> {
        let item = |title: String, slides: Vec<LoadedSlide>| LoadedItem {
            id,
            title,
            slides,
            background: None,
            theme: None,
            song: None,
            auto_advance_ms: None,
            loop_group: None,
        };
        let text = |content: Content| LoadedSlide {
            content,
            steps: 0,
            dwell_ms: None,
        };
        Ok(match kind {
            ItemKind::Header { title } => item(title.clone(), Vec::new()),
            // Sound has no picture: the screens stay as they are. Playing
            // it is the Audio role's job.
            ItemKind::Sound { sound } => item(sound.name.clone(), Vec::new()),
            ItemKind::Song { song, arrangement } => match self.library.song(*song).map_err(err)? {
                Some(s) => {
                    let slides = paginate_song(&s, arrangement.as_deref(), theme)
                        .into_iter()
                        .map(|slide| text(Content::Lyrics { song: *song, slide }))
                        .collect();
                    LoadedItem {
                        song: Some(*song),
                        ..item(s.title.clone(), slides)
                    }
                }
                None => item("Missing song".into(), Vec::new()),
            },
            ItemKind::Scripture {
                passage,
                translation,
            } => {
                let abbreviation = self
                    .library
                    .translation(*translation)
                    .map_err(err)?
                    .map(|t| t.abbreviation)
                    .unwrap_or_default();
                let reference = format!("{passage} {abbreviation}").trim().to_string();
                let slides = match self.library.bible_text(*translation).map_err(err)? {
                    Some(bible) => match bible.verses(passage) {
                        Ok(verses) => paginate_passage(
                            &verses,
                            &reference,
                            PassageSplit::default(),
                            theme,
                            primary,
                            &mut self.fonts,
                        ),
                        Err(_) => Vec::new(),
                    },
                    None => Vec::new(),
                };
                let slides = slides
                    .into_iter()
                    .map(|slide| text(Content::Scripture { slide }))
                    .collect();
                item(reference, slides)
            }
            ItemKind::Deck { deck } => match self.library.deck(*deck).map_err(err)? {
                Some(d) => match self.parsed_deck(&d) {
                    Ok(parsed) => {
                        let slides = parsed
                            .slides
                            .iter()
                            .map(|slide| LoadedSlide {
                                content: Content::DeckSlide {
                                    deck: *deck,
                                    index: slide.index,
                                    step: 0,
                                    scene: slide.scenes[0].clone(),
                                    notes: slide.notes.clone(),
                                },
                                steps: slide.builds.len(),
                                dwell_ms: None,
                            })
                            .collect();
                        item(d.title.clone(), slides)
                    }
                    Err(why) => item(format!("{} ({why})", d.title), Vec::new()),
                },
                None => item("Missing presentation".into(), Vec::new()),
            },
            ItemKind::Media { blob, name, looped } => {
                let video = [".mp4", ".mov", ".m4v", ".webm", ".mkv"]
                    .iter()
                    .any(|ext| name.to_lowercase().ends_with(ext));
                let content = if video {
                    Content::Video {
                        blob: *blob,
                        looped: *looped,
                    }
                } else {
                    Content::Image { blob: *blob }
                };
                item(name.clone(), vec![text(content)])
            }
            ItemKind::Countdown { target, label } => {
                let target = match target {
                    CountdownTarget::At(t) => *t,
                    // Counts from loading, not from going live, until the
                    // engine reports the time an item went live.
                    CountdownTarget::Seconds(s) => Timestamp(now().0 + i64::from(*s) * 1000),
                };
                let content = Content::Countdown {
                    target,
                    label: label.clone(),
                };
                item(label.clone(), vec![text(content)])
            }
            ItemKind::Text {
                title,
                body,
                attribution,
            } => {
                let slides = text_slides(body, attribution.as_deref())
                    .into_iter()
                    .map(|slide| text(Content::Text { slide }))
                    .collect();
                let title = title.trim();
                item(
                    if title.is_empty() { "Text" } else { title }.to_string(),
                    slides,
                )
            }
            ItemKind::Announcements {
                slides,
                seconds_each,
            } => LoadedItem {
                auto_advance_ms: Some(u32::from(*seconds_each).max(1) * 1000),
                ..item(
                    "Announcements".into(),
                    slides
                        .iter()
                        .map(|blob| text(Content::Image { blob: *blob }))
                        .collect(),
                )
            },
        })
    }

    /// A deck's slides, parsed once per version while its service is open.
    fn parsed_deck(&mut self, deck: &Deck) -> Result<Arc<ParsedSlides>> {
        if let Some(parsed) = self.parsed.get(&deck.current.blob) {
            return Ok(parsed.clone());
        }
        let parsed = Arc::new(decks::parse(&self.library, deck, &self.fonts)?);
        self.parsed.insert(deck.current.blob, parsed.clone());
        Ok(parsed)
    }

    /// Composes and sends the program to every output that changed, and
    /// tells the UI.
    fn after_live(&mut self, events: Vec<lampstand_live::Event>) {
        self.after_live_at(events, now(), false);
    }

    /// `ticked` events come from the clock, not an operator action: the UI
    /// hears about them only if the live state it shows changed.
    fn after_live_at(&mut self, events: Vec<lampstand_live::Event>, now: Timestamp, ticked: bool) {
        let mut program_changed = false;
        let mut cursors_changed = false;
        let mut build = None;
        for event in &events {
            match event {
                lampstand_live::Event::ProgramChanged => {
                    program_changed = true;
                    cursors_changed = true;
                }
                lampstand_live::Event::PreviewChanged => cursors_changed = true,
                lampstand_live::Event::ItemLive { item } => {
                    cursors_changed = true;
                    self.cue_obs(*item);
                    self.cue_sound(*item);
                }
                // Comes with `ProgramChanged`: `present_with` sends it as
                // `Animate` to the outputs showing the step before.
                lampstand_live::Event::BuildStep { cursor } => {
                    cursors_changed = true;
                    let step = self
                        .loaded
                        .as_ref()
                        .and_then(|l| l.builds.get(&(cursor.item, cursor.slide)))
                        .and_then(|steps| steps.get(cursor.step.checked_sub(1)?))
                        .cloned()
                        .unwrap_or_default();
                    build = Some((cursor.step, step));
                }
                // CCLI usage logging arrives with T38.
                lampstand_live::Event::SongShown { .. } => {}
            }
        }
        if program_changed {
            self.present_with(now, build);
        }
        if cursors_changed {
            let state = (
                self.live.live(),
                self.live.preview(),
                self.live.is_idle(),
                self.live.program().overlay,
            );
            if !ticked || self.told_ui != Some(state) {
                (self.emit)(Event::Live {
                    live: state.0,
                    preview: state.1,
                    idle: state.2,
                    overlay: state.3,
                });
            }
            self.told_ui = Some(state);
        }
        self.write_journal(now);
    }

    /// Steps 3 and 4 of the click: compose per output, send `Show`.
    /// Tells `obs_control` which item went live and its scene. Every item
    /// is cued, with or without a scene, because a new item ends a manual
    /// hold either way.
    fn cue_obs(&self, item: ItemId) {
        let Some(cues) = &self.obs_cues else { return };
        let scene = self
            .loaded
            .as_ref()
            .and_then(|l| l.obs_scenes.get(&item))
            .cloned();
        let _ = cues.send(SceneCue { item, scene });
    }

    fn present(&mut self) {
        self.present_at(now());
    }

    /// Composes every output with a window or a browser-source URL whose
    /// program, size, theme or config changed since it was last sent.
    fn present_at(&mut self, now: Timestamp) {
        self.present_with(now, None);
    }

    /// `build` is the build step just clicked (the step it brings the live
    /// slide to, and its animations).
    fn present_with(&mut self, now: Timestamp, build: Option<(usize, Vec<Animation>)>) {
        let program = self.stepped(self.live.program().clone());
        let service_theme = self.loaded_theme();
        let idle = self.live.is_idle();
        let logo = self.logo();
        let ids: Vec<(OutputId, (u32, u32))> = self
            .sizes
            .iter()
            .chain(&self.streams)
            .map(|(id, size)| (*id, *size))
            .collect();
        for (id, size) in ids {
            let Some(config) = self.outputs.get(id).cloned() else {
                continue;
            };
            let theme = self.resolve_theme(config.theme.or(program.theme).or(service_theme));
            // Nothing on air: an audience screen shows its idle screen.
            // Black and Logo are overlays, so `is_idle` is false under
            // them and they cover it. Stage and stream never show it.
            let mut shown_program = program.clone();
            shown_program.logo = logo.clone();
            // Only audience and stream outputs carry the logo bug.
            if config.role == OutputRole::Stage {
                if let Some(l) = shown_program.logo.as_mut() {
                    l.bug = None;
                }
            }
            if idle && matches!(config.role, OutputRole::Audience { .. }) {
                shown_program.idle = self.idle_content(&config);
            }
            let sent = Sent {
                program: shown_program,
                size,
                theme,
                config,
                countdown: (
                    countdown_seconds(&program.content, now),
                    countdown_seconds(&program.next, now),
                ),
            };
            let previous = self.sent.get(&id);
            if previous == Some(&sent) {
                continue;
            }
            if self.streams.contains_key(&id) {
                let composed = compose_at(
                    &sent.program,
                    &sent.config,
                    OutputSize {
                        width: size.0 as f32,
                        height: size.1 as f32,
                    },
                    &sent.theme,
                    &mut self.fonts,
                    now,
                    local_utc_offset_minutes(),
                );
                self.hub.publish(id, Arc::new(composed.scene));
                self.sent.insert(id, sent);
                continue;
            }
            let change = deck_change(
                previous.and_then(|p| p.program.content.as_ref()),
                sent.program.content.as_ref(),
            );
            let same_frame = previous.is_some_and(|p| {
                p.size == sent.size
                    && p.theme == sent.theme
                    && p.config == sent.config
                    && p.program.overlay == sent.program.overlay
                    && p.program.background == sent.program.background
            });
            // The next build step on an output showing the step before:
            // animate what is there.
            if let (DeckChange::NextStep(step), Some((built, animations)), true) =
                (change, &build, same_frame)
            {
                if step == *built {
                    self.sink
                        .send(id, OutputCommand::Animate(animations.clone()));
                    self.sent.insert(id, sent);
                    continue;
                }
            }
            let mut shown = sent.program.clone();
            let mut entry = Vec::new();
            let transition = match change {
                DeckChange::Arrive => {
                    let slide = self.deck_slide(shown.content.as_ref());
                    match (&mut shown.content, slide) {
                        (Some(Content::DeckSlide { step: 0, scene, .. }), Some(slide)) => {
                            // Shown as it arrives; what plays by itself
                            // follows the `Show`.
                            *scene = slide.arrival.clone();
                            entry = slide.entry.clone();
                            slide.transition
                        }
                        (_, Some(slide)) => slide.transition,
                        _ => Transition::CUT,
                    }
                }
                DeckChange::NextStep(_) | DeckChange::OtherStep => Transition::CUT,
                DeckChange::Other => sent.theme.transition,
            };
            let composed = compose_at(
                &shown,
                &sent.config,
                OutputSize {
                    width: size.0 as f32,
                    height: size.1 as f32,
                },
                &sent.theme,
                &mut self.fonts,
                now,
                local_utc_offset_minutes(),
            );
            self.sink.send(
                id,
                OutputCommand::Show {
                    scene: Arc::new(composed.scene),
                    transition,
                },
            );
            if !entry.is_empty() {
                self.sink.send(id, OutputCommand::Animate(entry));
            }
            self.sent.insert(id, sent);
        }
    }

    /// The program with each deck slide's scene at its build step.
    fn stepped(&self, mut program: Program) -> Program {
        for content in [&mut program.content, &mut program.next] {
            let stepped = self.deck_slide(content.as_ref()).zip(match content {
                Some(Content::DeckSlide { step, .. }) => Some(*step),
                _ => None,
            });
            if let (Some((slide, step)), Some(Content::DeckSlide { scene, .. })) =
                (stepped, content)
            {
                *scene = slide.scene_at(step);
            }
        }
        program
    }

    /// The parsed deck slide a `Content::DeckSlide` shows.
    fn deck_slide(&self, content: Option<&Content>) -> Option<&decks::Slide> {
        let Some(Content::DeckSlide { deck, index, .. }) = content else {
            return None;
        };
        self.loaded.as_ref()?.slides.get(deck)?.slide(*index)
    }

    /// The logo screen from the church's brand, if it has set one.
    fn logo(&mut self) -> Option<Logo> {
        let brand = self.church().brand?;
        let bug = match brand.logo {
            Some(image) if brand.bug.enabled => self.logo_aspect(&image).map(|aspect| Bug {
                image,
                aspect,
                corner: brand.bug.corner,
                size: brand.bug.size,
                opacity: brand.bug.alpha(),
            }),
            _ => None,
        };
        Some(Logo {
            image: brand.logo,
            background: brand.primary,
            accent: brand.accent,
            bug,
        })
    }

    /// The logo's width over its height, read once. `None` when the
    /// picture is not on this device or cannot be read, so no bug is drawn
    /// rather than a misshapen one.
    fn logo_aspect(&mut self, blob: &BlobHash) -> Option<f32> {
        if let Some(known) = self.logo_aspects.get(blob) {
            return *known;
        }
        let path = self.library.repo().blobs().path(blob);
        let aspect = path.and_then(|p| {
            let (w, h) = image::ImageReader::open(p)
                .ok()?
                .with_guessed_format()
                .ok()?
                .into_dimensions()
                .ok()?;
            (w > 0 && h > 0).then(|| w as f32 / h as f32)
        });
        if aspect.is_some() {
            self.logo_aspects.insert(*blob, aspect);
        }
        aspect
    }

    /// What `config` shows while nothing is on air: its own idle screen,
    /// else the device default, else the default idle screen (the church's
    /// logo and name, or Lampstand's lamp). A file or deck that has not
    /// reached this device yet, or that is gone, shows the plain theme
    /// background rather than a hole.
    fn idle_content(&mut self, config: &OutputConfig) -> Option<Idle> {
        let church = self.church();
        let present = |lib: &Library, blob: &BlobHash| lib.repo().blobs().path(blob).is_some();
        match idle_plan(config, church.idle.as_ref()) {
            IdlePlan::Plain => None,
            IdlePlan::Mark => {
                let brand = crate::theme_preview::brand_for_idle(&self.library, church.brand);
                Some(Idle::mark(brand.as_ref()))
            }
            IdlePlan::Chosen(IdleScreen::Blank | IdleScreen::Default) => None,
            IdlePlan::Chosen(IdleScreen::Image { blob, .. }) => {
                present(&self.library, blob).then_some(Idle::Image { blob: *blob })
            }
            IdlePlan::Chosen(IdleScreen::Video { blob, .. }) => {
                present(&self.library, blob).then_some(Idle::Video { blob: *blob })
            }
            IdlePlan::Chosen(IdleScreen::DeckSlide { deck, slide }) => {
                let deck = self.library.deck(*deck).ok().flatten()?;
                let parsed = self.parsed_deck(&deck).ok()?;
                let scene = parsed.slides.get(*slide)?.scenes.last()?.clone();
                Some(Idle::Deck { scene })
            }
        }
    }

    fn loaded_theme(&self) -> Option<ThemeId> {
        let id = self.loaded.as_ref()?.view.service;
        self.library.service(id).ok().flatten()?.theme
    }

    /// The theme with this ID, else the starter theme, else any theme. If
    /// a synced delete removed them all, the built-in starter theme, so a
    /// delete on another device never takes the output down.
    fn resolve_theme(&self, id: Option<ThemeId>) -> Theme {
        self.library
            .resolve_theme(&[id, self.church().default_theme])
    }

    /// The theme songs and scripture are split with: the one the main
    /// screen draws them in, else the service's, else the default, so what
    /// is paginated is what is shown.
    fn pagination_theme(&self, service: Option<ThemeId>, primary: &OutputConfig) -> Theme {
        self.library
            .resolve_theme(&[primary.theme, service, self.church().default_theme])
    }

    fn put_settings(&mut self, settings: lampstand_api::DeviceSettings) -> Result<Response> {
        self.put_device_settings(settings)?;
        // The default theme, the idle screen and the logo change what the
        // outputs show.
        self.sent.clear();
        self.present();
        Ok(Response::Settings(self.device_settings()))
    }

    /// Takes a deleted theme out of the settings that name it, on every
    /// device (the church settings sync).
    fn settings_forget_theme(&mut self, id: ThemeId) -> Result<()> {
        self.church_forget_theme(id)?;
        self.present();
        Ok(())
    }

    /// `ExportBrand`: the brand, its themes and its idle screen in one file.
    fn export_brand(&self, path: &str) -> Result<String> {
        use lampstand_library::brand_package::{export, BrandSource};
        let church = self.church();
        let source = BrandSource {
            brand: church.brand.clone(),
            idle: church.idle.clone(),
            brand_theme: church.brand_theme,
        };
        let file = std::fs::File::create(path).map_err(err)?;
        let report = export(&self.library, &source, std::io::BufWriter::new(file)).map_err(err)?;
        let mut message = format!(
            "Saved the brand: {} theme{} and {} picture or video file{}.",
            report.themes,
            if report.themes == 1 { "" } else { "s" },
            report.files,
            if report.files == 1 { "" } else { "s" },
        );
        if church.brand.is_none() {
            message
                .push_str(" No church name or logo has been set up yet, so only themes went in.");
        }
        for skipped in report.skipped {
            message.push(' ');
            message.push_str(&skipped);
        }
        Ok(message)
    }

    /// `InspectBrand`: what a file holds, and which of its fonts are missing here.
    fn inspect_brand(&self, path: &str) -> Result<lampstand_api::BrandContents> {
        use lampstand_core::FontLookup;
        let file = std::fs::File::open(path).map_err(err)?;
        let file = lampstand_library::brand_package::inspect(std::io::BufReader::new(file))
            .map_err(err)?;
        let installed = |family: &str| {
            family.trim().is_empty()
                || self.fonts.lookup(family) == lampstand_core::FontMatch::Exact
        };
        let mut missing: Vec<String> = Vec::new();
        for family in file
            .brand
            .iter()
            .map(|b| b.font_family.clone())
            .chain(file.themes.iter().map(|t| t.text.font_family.clone()))
        {
            if !installed(&family) && !missing.contains(&family) {
                missing.push(family);
            }
        }
        let font = file
            .brand
            .as_ref()
            .map(|b| b.font_family.clone())
            .filter(|f| !f.trim().is_empty());
        Ok(lampstand_api::BrandContents {
            name: file.brand.as_ref().map(|b| b.name.clone()),
            has_logo: file.brand.as_ref().is_some_and(|b| b.logo.is_some()),
            font_installed: font.as_deref().is_none_or(installed),
            font_family: font,
            missing_fonts: missing,
            themes: file.themes.iter().map(|t| t.name.clone()).collect(),
            idle: match &file.idle {
                Some(IdleScreen::Image { name, .. }) => Some(format!("Picture: {name}")),
                Some(IdleScreen::Video { name, .. }) => Some(format!("Looping video: {name}")),
                Some(IdleScreen::Blank) => Some("Plain background".into()),
                _ => None,
            },
            has_brand: file.brand.is_some(),
        })
    }

    /// `ImportBrand`: puts the files and themes in the library and the
    /// brand, default theme and idle screen in the settings.
    fn import_brand(&mut self, path: &str, mode: lampstand_api::BrandImportMode) -> Result<String> {
        use lampstand_api::BrandImportMode;
        use lampstand_library::brand_package::{import, ImportMode};
        let file = std::fs::File::open(path).map_err(err)?;
        let got = import(
            &self.library,
            std::io::BufReader::new(file),
            match mode {
                BrandImportMode::AddAlongside => ImportMode::AddAlongside,
                BrandImportMode::Replace => ImportMode::Replace,
            },
        )
        .map_err(err)?;
        let replace = mode == BrandImportMode::Replace;
        self.save_church(|church| {
            if got.brand.is_some() && (replace || church.brand.is_none()) {
                church.brand = got.brand.clone();
                if let Some(id) = got.brand_theme {
                    church.brand_theme = Some(id);
                    church.default_theme = Some(id);
                }
            }
            if got.idle.is_some() && (replace || church.idle.is_none()) {
                church.idle = got.idle.clone();
            }
        })?;
        self.sent.clear();
        self.present();
        Ok(format!(
            "Imported the brand: {} theme{} added, {} replaced.",
            got.themes_added,
            if got.themes_added == 1 { "" } else { "s" },
            got.themes_replaced
        ))
    }

    /// The quick start: keeps the brand, makes (or updates) its theme and
    /// makes that the default theme.
    fn save_brand(&mut self, brand: lampstand_core::brand::Brand) -> Result<Response> {
        let existing = self
            .church()
            .brand_theme
            .filter(|id| self.library.theme(*id).ok().flatten().is_some());
        let id = existing.unwrap_or_else(ThemeId::generate);
        self.library
            .put_theme(&lampstand_library::theme_from_brand(&brand, id))
            .map_err(err)?;
        self.save_church(|church| {
            church.brand = Some(brand);
            church.brand_theme = Some(id);
            church.default_theme = Some(id);
        })?;
        self.sent.clear();
        self.present();
        Ok(Response::Settings(self.device_settings()))
    }

    /// Turns the logo bug on or off, or moves or resizes it, without
    /// touching the rest of the brand or its theme. A church that has no
    /// brand yet has nothing to put it on.
    fn set_logo_bug(&mut self, bug: lampstand_core::brand::LogoBug) -> Result<Response> {
        if self.church().brand.is_none() {
            return Err(err("Add your church's logo in Setup first."));
        }
        self.save_church(|church| {
            if let Some(brand) = church.brand.as_mut() {
                brand.bug = lampstand_core::brand::LogoBug {
                    opacity: bug.opacity.clamp(10, 100),
                    ..bug
                };
            }
        })?;
        self.present();
        Ok(Response::Settings(self.device_settings()))
    }

    /// The audience output scripture is split against: the primary one,
    /// else any audience output, else a 1080p screen with no margins.
    fn primary_output(&self) -> OutputConfig {
        let all = self.outputs.all();
        all.iter()
            .find(|o| o.role == OutputRole::Audience { primary: true })
            .or_else(|| {
                all.iter()
                    .find(|o| matches!(o.role, OutputRole::Audience { .. }))
            })
            .cloned()
            .unwrap_or_else(|| OutputConfig {
                id: OutputId::generate(),
                name: "Default".into(),
                role: OutputRole::Audience { primary: true },
                target: OutputTarget::Window {
                    width: 1920,
                    height: 1080,
                },
                safe_area: SafeArea::default(),
                background: lampstand_core::output::BackgroundMode::Bleed,
                theme: None,
                idle: None,
                enabled: true,
            })
    }

    // ---- Windows -----------------------------------------------------

    /// Opens a window for every enabled output that has somewhere to go,
    /// closes the rest, then shows the program on the new ones.
    fn place_windows(&mut self) {
        let displays = self.windows.displays();
        self.seen_displays = displays.clone();
        let wanted: Vec<(OutputId, lampstand_core::output::DisplayMatch)> = self
            .outputs
            .all()
            .iter()
            .filter(|o| o.enabled && self.presenting)
            .filter_map(|o| match &o.target {
                OutputTarget::Display(m) => Some((o.id, m.clone())),
                _ => None,
            })
            .collect();
        let assigned = assign_displays(&wanted, &displays);
        let mut placements = HashMap::new();
        for o in self
            .outputs
            .all()
            .iter()
            .filter(|o| o.enabled && self.presenting)
        {
            let placement = match &o.target {
                OutputTarget::Window { width, height } => Placement::Window {
                    width: *width,
                    height: *height,
                },
                OutputTarget::Display(_) => match assigned.get(&o.id) {
                    Some(&index) => Placement::Display(displays[index].clone()),
                    // Its display is not connected: wait, hidden.
                    None => continue,
                },
                OutputTarget::BrowserSource { .. } => continue,
            };
            placements.insert(o.id, (o.name.clone(), placement));
        }

        let stale: Vec<OutputId> = self
            .placed
            .keys()
            .filter(|id| !placements.contains_key(id))
            .copied()
            .collect();
        for id in stale {
            self.windows.close(id);
            self.placed.remove(&id);
            self.sizes.remove(&id);
            self.sent.remove(&id);
        }
        for (id, (name, placement)) in placements {
            let size = match &placement {
                Placement::Window { width, height } => (*width, *height),
                Placement::Display(d) => (d.width, d.height),
            };
            if self.placed.get(&id) == Some(&placement) {
                continue;
            }
            match self.windows.open(id, &name, placement.clone()) {
                Ok(()) => {
                    self.placed.insert(id, placement);
                    self.sizes.insert(id, size);
                    self.sent.remove(&id);
                }
                Err(e) => eprintln!("lampstand: cannot open output {name}: {e}"),
            }
        }
        self.place_streams();
        self.present();
        self.write_journal(now());
    }

    /// Gives every enabled browser-source output its URL in the hub, with
    /// its saved key, and takes the URL away from the rest (their pages
    /// disconnect). Keys stay saved while an output is off.
    fn place_streams(&mut self) {
        let wanted: HashMap<OutputId, (u32, u32)> = self
            .outputs
            .all()
            .iter()
            .filter(|o| o.enabled && self.presenting)
            .filter_map(|o| match o.target {
                OutputTarget::BrowserSource { width, height } => {
                    Some((o.id, (width.max(1), height.max(1))))
                }
                _ => None,
            })
            .collect();
        self.streaming.store(!wanted.is_empty(), Ordering::Relaxed);
        let stale: Vec<OutputId> = self
            .streams
            .keys()
            .filter(|id| !wanted.contains_key(id))
            .copied()
            .collect();
        for id in stale {
            self.hub.remove_output(id);
            self.streams.remove(&id);
            self.sent.remove(&id);
        }
        for (id, size) in wanted {
            if !self.streams.contains_key(&id) {
                let key = self
                    .hub
                    .add_output_with_key(id, self.keys.get(id).unwrap_or_default());
                if let Err(e) = self.keys.put(id, &key) {
                    eprintln!("lampstand: cannot save the browser-source key: {e}");
                }
                self.sent.remove(&id);
            }
            self.streams.insert(id, size);
        }
    }

    /// The window system reports an output window's real inner size
    /// (physical pixels), on opening and on every resize.
    pub fn output_resized(&mut self, id: OutputId, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        if let Some(size) = self.sizes.get_mut(&id) {
            *size = (width, height);
            self.present();
        }
    }

    /// The operator closed an output window: turn that output off.
    pub fn output_closed(&mut self, id: OutputId) {
        self.windows.close(id);
        self.placed.remove(&id);
        self.sizes.remove(&id);
        self.sent.remove(&id);
        if let Some(mut config) = self.outputs.get(id).cloned() {
            config.enabled = false;
            if self.outputs.put(config).is_ok() {
                self.tell_screens();
                (self.emit)(Event::Changed {
                    kind: "outputs".into(),
                    id: id.uuid().to_string(),
                });
            }
        }
        self.write_journal(now());
    }

    /// The shell's look at the displays: place the outputs again only if
    /// they differ from those the windows were last placed against.
    pub fn displays_polled(&mut self, now: Vec<DisplayInfo>) {
        if now != self.seen_displays {
            self.displays_changed();
        }
    }

    /// Displays came or went: place display outputs again.
    pub fn displays_changed(&mut self) {
        self.place_windows();
        // The Screens page and the no-screen prompt list the displays.
        (self.emit)(Event::DisplaysChanged);
    }

    /// Why an update must not be installed now, if it must not: something is
    /// on air, or a service is about to start. Checked again by the shell
    /// at the moment of installing, however the screen looked a moment ago.
    pub fn update_blocker(&self) -> Option<String> {
        self.update_blocker_at(now())
    }

    /// `update_blocker` with the time given, for tests.
    pub fn update_blocker_at(&self, now: Timestamp) -> Option<String> {
        let next_service = self
            .library
            .services()
            .ok()?
            .iter()
            .filter_map(|s| s.starts_at)
            .filter(|t| *t > now)
            .min();
        crate::update::blocker(self.live.is_idle(), next_service, now)
    }

    /// Something to ask before installing an update, which does not refuse it:
    /// some services cannot be read here (a newer Lampstand saved them), so
    /// when the next one starts is not known. Refusing would trap the person,
    /// since installing is how they become readable.
    pub fn update_caution(&self) -> Option<String> {
        let scan = self.library.services_checked().ok()?;
        (scan.newer > 0).then(|| {
            "Some services were saved by a newer Lampstand, so this one cannot tell when the next \
             service starts. Make sure none is about to begin."
                .to_string()
        })
    }

    /// Drives announcement loops and countdowns. Call every 100 ms or so;
    /// the engine reports events only when something on air changed.
    pub fn tick(&mut self) {
        self.tick_at(now());
    }

    /// `tick` with the time given, for tests.
    pub fn tick_at(&mut self, now: Timestamp) {
        let events = self.live.tick(now);
        if !events.is_empty() {
            self.after_live_at(events, now, true);
        }
        // The heartbeat while live, and an fsync a burst of clicks put off.
        self.write_journal(now);
        self.church_tick();
        self.sound_tick(now);
        self.audio_tick();
    }

    // ---- Crash recovery (T57) ----------------------------------------

    /// What is on air now, as the journal keeps it.
    fn live_record(&self) -> LiveRecord {
        let program = self.live.program();
        let mut outputs: Vec<OutputId> = self.sizes.keys().copied().collect();
        outputs.sort();
        LiveRecord {
            service: self.loaded.as_ref().map(|l| l.view.service),
            live: self.live.live(),
            preview: self.live.preview(),
            overlay: program.overlay,
            text: program.content.is_some(),
            background: program.background.is_some(),
            outputs,
        }
    }

    fn write_journal(&mut self, now: Timestamp) {
        if !self.restoring {
            let record = self.live_record();
            self.journal.record(&record, now);
        }
    }

    /// Reopens the service the journal says was live, on the same slide,
    /// with the same overlay, preview and cleared text or background.
    fn restore(&mut self) {
        let record = self.journal.recover(now());
        let loaded = match record.as_ref().and_then(|r| r.service) {
            Some(service) => self.load_service(service).map(|_| ()),
            None => Ok(()),
        };
        self.restoring = false;
        let Some(record) = record else {
            return;
        };
        if let Err(e) = loaded {
            eprintln!("lampstand: cannot restore the live service: {e}");
            return;
        }
        let mut commands = Vec::new();
        commands.extend(record.live.map(Command::Show));
        match record.overlay {
            Overlay::Black => commands.push(Command::ToggleBlack),
            Overlay::Logo => commands.push(Command::ToggleLogo),
            Overlay::None => {}
        }
        if !record.text {
            commands.push(Command::ClearText);
        }
        if !record.background {
            commands.push(Command::ClearBackground);
        }
        commands.extend(record.preview.map(Command::Preview));
        let events = commands
            .into_iter()
            .flat_map(|c| self.live.apply(c))
            .collect();
        self.after_live(events);
        let missing = record
            .outputs
            .iter()
            .filter(|id| !self.sizes.contains_key(id))
            .count();
        eprintln!(
            "lampstand: restored the live service after a crash{}",
            if missing > 0 {
                format!(" ({missing} of its outputs are not connected)")
            } else {
                String::new()
            }
        );
    }

    /// A clean quit: the journal goes, so the next start does not reopen
    /// the service.
    pub fn closing(&mut self) {
        self.journal.clear();
    }

    /// Tells the UI what is loaded and on air, for a webview that was not
    /// listening when it happened (a restore at start, or a reload).
    pub fn announce(&self) {
        if let Some(loaded) = &self.loaded {
            (self.emit)(Event::Loaded(loaded.view.clone()));
        }
        (self.emit)(Event::Live {
            live: self.live.live(),
            preview: self.live.preview(),
            idle: self.live.is_idle(),
            overlay: self.live.program().overlay,
        });
    }

    /// Documents changed here or on another device: tell the UI, and
    /// reload the open service if it, one of its songs or decks or a theme
    /// changed.
    /// A large batch (an import) becomes one `Changed` per kind.
    pub fn library_changed(&mut self, changes: &[DocChanged]) {
        const MANY: usize = 20;
        if changes.len() > MANY {
            let mut kinds: Vec<String> = changes
                .iter()
                .map(|c| format!("{:?}", c.kind).to_lowercase())
                .collect();
            kinds.sort();
            kinds.dedup();
            for kind in kinds {
                (self.emit)(Event::Changed {
                    kind,
                    id: String::new(),
                });
            }
        } else {
            for change in changes {
                (self.emit)(Event::Changed {
                    kind: format!("{:?}", change.kind).to_lowercase(),
                    id: change.id.to_string(),
                });
            }
        }
        if changes
            .iter()
            .any(|c| c.kind == lampstand_sync::DocKind::Theme)
        {
            self.present();
        }
        // A service changed: it may name sound files this device lacks. Only
        // that service is read.
        for change in changes
            .iter()
            .filter(|c| c.kind == lampstand_sync::DocKind::Service)
        {
            self.sound_sync.service_changed(change.id);
        }
        if changes
            .iter()
            .any(|c| c.kind == lampstand_sync::DocKind::ChurchSettings)
        {
            // The church look or idle screen changed on another device (or
            // this one): redraw, and look for files it points at.
            self.sent.clear();
            self.present();
            self.church_sync_look_now();
        }
        let Some(loaded) = &self.loaded else {
            return;
        };
        let service = loaded.view.service;
        let affects = changes.iter().any(|c| {
            c.id == service.uuid()
                || c.kind == lampstand_sync::DocKind::Theme
                || (c.kind == lampstand_sync::DocKind::Song
                    && loaded
                        .songs
                        .contains(&lampstand_core::song::SongId::from_uuid(c.id)))
                || (c.kind == lampstand_sync::DocKind::Deck
                    && loaded.decks.contains(&DeckId::from_uuid(c.id)))
        });
        if affects {
            if let Err(e) = self.load_service(service) {
                eprintln!("lampstand: reloading the service failed: {e}");
            }
        }
    }

    /// The scene of a deck slide in the open service after `step` build
    /// steps (past the last, the last), for a thumbnail.
    pub fn slide_scene(&self, item: ItemId, slide: usize, step: usize) -> Option<Arc<Scene>> {
        let scenes = self.loaded.as_ref()?.scenes.get(&(item, slide))?;
        scenes.get(step.min(scenes.len().checked_sub(1)?)).cloned()
    }

    /// For the live view: what is on air, as text.
    pub fn live_text(&self) -> Option<SlideText> {
        match &self.live.program().content {
            Some(Content::Lyrics { slide, .. })
            | Some(Content::Scripture { slide })
            | Some(Content::Text { slide }) => Some(slide.clone()),
            _ => None,
        }
    }
}

/// The databases `DownloadSongDatabase` can fetch.
pub fn song_databases() -> Vec<SongDatabase> {
    lampstand_connectors::song_databases::WORSHIP_LEADER_APP
        .iter()
        .map(|d| SongDatabase {
            code: d.code.into(),
            name: d.name.into(),
            source: "worshipleaderapp.com".into(),
        })
        .collect()
}

/// How a deck slide on an output changes, which decides how it is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeckChange {
    /// A deck slide arrives (from another slide, or from anything else).
    Arrive,
    /// The same slide, one build step on: to this step.
    NextStep(usize),
    /// The same slide at another step (back, or several on).
    OtherStep,
    /// Not a deck slide, or the same slide at the same step.
    Other,
}

fn deck_change(before: Option<&Content>, after: Option<&Content>) -> DeckChange {
    let Some(Content::DeckSlide {
        deck, index, step, ..
    }) = after
    else {
        return DeckChange::Other;
    };
    match before {
        Some(Content::DeckSlide {
            deck: d,
            index: i,
            step: s,
            ..
        }) if d == deck && i == index => {
            if *step == s + 1 {
                DeckChange::NextStep(*step)
            } else if step != s {
                DeckChange::OtherStep
            } else {
                DeckChange::Other
            }
        }
        _ => DeckChange::Arrive,
    }
}

fn slide_view(slide: &LoadedSlide) -> SlideView {
    let (label, lines, footer) = match &slide.content {
        Content::Lyrics { slide, .. } | Content::Scripture { slide } | Content::Text { slide } => (
            slide.label.clone(),
            slide.lines.clone(),
            slide.footer.clone(),
        ),
        Content::DeckSlide { index, .. } => {
            (Some(format!("Slide {}", index + 1)), Vec::new(), None)
        }
        Content::Image { .. } => (Some("Image".into()), Vec::new(), None),
        Content::Video { .. } => (Some("Video".into()), Vec::new(), None),
        Content::Countdown { label, .. } => (Some("Countdown".into()), vec![label.clone()], None),
    };
    let notes = match &slide.content {
        Content::DeckSlide { notes, .. } => notes.clone(),
        _ => String::new(),
    };
    SlideView {
        label,
        lines,
        footer,
        steps: slide.steps,
        notes,
    }
}

/// Upcoming services first, soonest first; then undated ones, newest
/// first (IDs are time-ordered); then past ones, most recent first. A
/// service counts as upcoming for a few hours after it starts, so the one
/// running now stays at the top.
fn sort_services(services: &mut [ServiceSummary], now: Timestamp) {
    const RUNNING_MS: i64 = 6 * 3_600_000;
    let newest_first = |a: &ServiceSummary, b: &ServiceSummary| b.id.uuid().cmp(&a.id.uuid());
    let group = |s: &ServiceSummary| match s.starts_at {
        Some(t) if t.0 + RUNNING_MS >= now.0 => 0,
        None => 1,
        Some(_) => 2,
    };
    services.sort_by(|a, b| {
        group(a).cmp(&group(b)).then_with(|| match group(a) {
            0 => a
                .starts_at
                .cmp(&b.starts_at)
                .then_with(|| newest_first(a, b)),
            1 => newest_first(a, b),
            _ => b
                .starts_at
                .cmp(&a.starts_at)
                .then_with(|| newest_first(a, b)),
        })
    });
}

/// Local time's offset from UTC, for the stage clock.
fn local_utc_offset_minutes() -> i32 {
    chrono::Local::now().offset().local_minus_utc() / 60
}

/// A new browser-source key: what `SceneHub` makes, 32 hex digits.
fn fresh_key() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// The services as the list shows them, in order.
fn summaries(services: Vec<Service>) -> Vec<ServiceSummary> {
    let mut services: Vec<ServiceSummary> = services
        .into_iter()
        .map(|s| ServiceSummary {
            id: s.id,
            title: s.title,
            starts_at: s.starts_at,
            time_zone: s.time_zone,
            items: s
                .items
                .values()
                .filter(|i| !matches!(i.kind, ItemKind::Header { .. }))
                .count() as u32,
        })
        .collect();
    sort_services(&mut services, now());
    services
}

fn now() -> Timestamp {
    Timestamp(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64),
    )
}
