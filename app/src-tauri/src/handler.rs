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

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use lampstand_api::{
    Event, LibraryRequest, LoadedItemView, LoadedService, OutputRequest, Request, Response,
    ServiceRequest, ServiceSummary, SlideView, SongDatabase, SongHit,
};
use lampstand_core::id::Timestamp;
use lampstand_core::output::{OutputConfig, OutputId, OutputRole, OutputTarget};
use lampstand_core::program::{Content, Program, SlideText};
use lampstand_core::safe_area::SafeArea;
use lampstand_core::scene::Animation;
use lampstand_core::scripture::parse_reference;
use lampstand_core::service::{CountdownTarget, ItemId, ItemKind, Service, ServiceId};
use lampstand_core::theme::{Theme, ThemeId};
use lampstand_layout::{compose_at, paginate_passage, paginate_song, Fonts, OutputSize};
use lampstand_library::{starter_theme_id, Library};
use lampstand_live::{LiveEngine, LoadedItem, LoadedSlide};
use lampstand_render::output::{assign_displays, DisplayInfo};
use lampstand_render::{CalibrationPattern, OutputCommand, OutputSink};
use lampstand_sync::DocChanged;

use crate::config::{device_key, OutputStore};
use crate::import::import_files;
use crate::seed;

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
}

struct Loaded {
    view: LoadedService,
    /// Build-step animations by (item, slide), each a list per step.
    /// Empty until decks load (T29 to T31).
    builds: HashMap<(ItemId, usize), Vec<Vec<Animation>>>,
    /// The songs in the service, so edits to them reload it.
    songs: Vec<lampstand_core::song::SongId>,
}

pub struct App {
    library: Arc<Library>,
    live: LiveEngine,
    fonts: Fonts,
    outputs: OutputStore,
    sink: Arc<dyn OutputSink>,
    windows: Box<dyn WindowHost>,
    emit: EventSink,
    /// Where each open output window was asked to go. A window is opened
    /// again only when this changes, never because its real size differs.
    placed: HashMap<OutputId, Placement>,
    /// The pixel size each output with a window is composed at: the
    /// placement's size until the window reports its real one.
    sizes: HashMap<OutputId, (u32, u32)>,
    sent: HashMap<OutputId, Sent>,
    loaded: Option<Loaded>,
}

type Result<T> = std::result::Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl App {
    /// Opens (or creates) everything under `data_dir`: the library in
    /// `library/`, the device key and `outputs.json`. On first run the
    /// seed hymns are imported. Enabled outputs get their windows.
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
        let mut app = Self {
            library,
            live: LiveEngine::new(),
            fonts,
            outputs,
            sink,
            windows,
            emit,
            placed: HashMap::new(),
            sizes: HashMap::new(),
            sent: HashMap::new(),
            loaded: None,
        };
        app.place_windows();
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

    pub fn program(&self) -> &Program {
        self.live.program()
    }

    pub fn handle(&mut self, request: Request) -> Response {
        let result = match request {
            Request::Library(r) => self.library_request(r),
            Request::Service(r) => self.service_request(r),
            Request::Live(command) => {
                let events = self.live.apply(command);
                self.after_live(events);
                Ok(Response::Ok)
            }
            Request::Outputs(r) => self.output_request(r),
            // The shell answers these from `crate::sync`, without the lock.
            Request::Team(_) => Err("sync is not running".into()),
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
            LibraryRequest::ListTranslations => {
                Response::Translations(lib.translations().map_err(err)?)
            }
            LibraryRequest::LookupPassage {
                reference,
                translation,
            } => {
                let passage = parse_reference(&reference).map_err(|e| format!("{e:?}"))?;
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
            LibraryRequest::AddDeckFromLink { .. } | LibraryRequest::AcceptDeckUpdate(_) => {
                return Err("decks arrive with T29 to T41".into())
            }
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
            ServiceRequest::List => {
                let mut services: Vec<ServiceSummary> = lib
                    .services()
                    .map_err(err)?
                    .into_iter()
                    .map(|s| ServiceSummary {
                        id: s.id,
                        title: s.title,
                        starts_at: s.starts_at,
                        time_zone: s.time_zone,
                    })
                    .collect();
                sort_services(&mut services, now());
                Response::Services(services)
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
            ServiceRequest::Open(id) => Response::Loaded(self.load_service(id)?),
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
                if let OutputTarget::BrowserSource { .. } = config.target {
                    return Err(
                        "OBS browser sources arrive with the local server (T42, T43)".into(),
                    );
                }
                let id = config.id;
                self.outputs.put(config).map_err(err)?;
                self.sent.remove(&id);
                self.place_windows();
                Response::Outputs(self.outputs.all().to_vec())
            }
            OutputRequest::Remove(id) => {
                self.outputs.remove(id).map_err(err)?;
                self.place_windows();
                Response::Outputs(self.outputs.all().to_vec())
            }
            OutputRequest::Calibrate { output, active } => {
                let config = self.outputs.get(output).ok_or("no such output")?;
                let pattern = active.then_some(CalibrationPattern {
                    current: config.safe_area,
                    max_percent: 10,
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
            OutputRequest::BrowserSourceUrl(_) => {
                return Err("OBS browser sources arrive with the local server (T42, T43)".into())
            }
        })
    }

    // ---- Live --------------------------------------------------------

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
        let theme = self.theme(service.theme);
        let primary = self.primary_output();
        let mut items = Vec::new();
        let mut views = Vec::new();
        for (item_id, item) in service.ordered() {
            let loaded = self.load_item(item_id, &item.kind, &theme, &primary)?;
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
        };
        self.loaded = Some(Loaded {
            view: view.clone(),
            builds: HashMap::new(),
            songs: items.iter().filter_map(|i| i.song).collect(),
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
        };
        let text = |content: Content| LoadedSlide { content, steps: 0 };
        Ok(match kind {
            ItemKind::Header { title } => item(title.clone(), Vec::new()),
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
                        Ok(verses) => {
                            paginate_passage(&verses, &reference, theme, primary, &mut self.fonts)
                        }
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
            ItemKind::Deck { .. } => item(
                "Presentation (decks arrive with T29 to T31)".into(),
                Vec::new(),
            ),
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

    /// Composes and sends the program to every output that changed, and
    /// tells the UI.
    fn after_live(&mut self, events: Vec<lampstand_live::Event>) {
        let mut program_changed = false;
        let mut cursors_changed = false;
        for event in &events {
            match event {
                lampstand_live::Event::ProgramChanged => {
                    program_changed = true;
                    cursors_changed = true;
                }
                lampstand_live::Event::PreviewChanged | lampstand_live::Event::ItemLive { .. } => {
                    cursors_changed = true
                }
                lampstand_live::Event::BuildStep { cursor } => {
                    cursors_changed = true;
                    let step = self
                        .loaded
                        .as_ref()
                        .and_then(|l| l.builds.get(&(cursor.item, cursor.slide)))
                        .and_then(|steps| steps.get(cursor.step.checked_sub(1)?))
                        .cloned()
                        .unwrap_or_default();
                    for id in self.sizes.keys() {
                        self.sink.send(*id, OutputCommand::Animate(step.clone()));
                    }
                }
                // CCLI usage logging arrives with T38.
                lampstand_live::Event::SongShown { .. } => {}
            }
        }
        if program_changed {
            self.present();
        }
        if cursors_changed {
            (self.emit)(Event::Live {
                live: self.live.live(),
                preview: self.live.preview(),
                idle: self.live.is_idle(),
                overlay: self.live.program().overlay,
            });
        }
    }

    /// Steps 3 and 4 of the click: compose per output, send `Show`.
    fn present(&mut self) {
        let program = self.live.program().clone();
        let service_theme = self.loaded_theme();
        let ids: Vec<OutputId> = self.sizes.keys().copied().collect();
        for id in ids {
            let Some(config) = self.outputs.get(id).cloned() else {
                continue;
            };
            let size = self.sizes[&id];
            let theme = self.theme(config.theme.or(program.theme).or(service_theme));
            let sent = Sent {
                program: program.clone(),
                size,
                theme,
                config,
            };
            if self.sent.get(&id) == Some(&sent) {
                continue;
            }
            let composed = compose_at(
                &sent.program,
                &sent.config,
                OutputSize {
                    width: size.0 as f32,
                    height: size.1 as f32,
                },
                &sent.theme,
                &mut self.fonts,
                now(),
                local_utc_offset_minutes(),
            );
            self.sink.send(
                id,
                OutputCommand::Show {
                    scene: Arc::new(composed.scene),
                    transition: sent.theme.transition,
                },
            );
            self.sent.insert(id, sent);
        }
    }

    fn loaded_theme(&self) -> Option<ThemeId> {
        let id = self.loaded.as_ref()?.view.service;
        self.library.service(id).ok().flatten()?.theme
    }

    /// The theme with this ID, or the starter theme.
    fn theme(&self, id: Option<ThemeId>) -> Theme {
        id.and_then(|id| self.library.theme(id).ok().flatten())
            .or_else(|| self.library.theme(starter_theme_id()).ok().flatten())
            .or_else(|| self.library.themes().ok()?.into_iter().next())
            .expect("the library always has the starter theme")
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
                enabled: true,
            })
    }

    // ---- Windows -----------------------------------------------------

    /// Opens a window for every enabled output that has somewhere to go,
    /// closes the rest, then shows the program on the new ones.
    fn place_windows(&mut self) {
        let displays = self.windows.displays();
        let wanted: Vec<(OutputId, lampstand_core::output::DisplayMatch)> = self
            .outputs
            .all()
            .iter()
            .filter(|o| o.enabled)
            .filter_map(|o| match &o.target {
                OutputTarget::Display(m) => Some((o.id, m.clone())),
                _ => None,
            })
            .collect();
        let assigned = assign_displays(&wanted, &displays);
        let mut placements = HashMap::new();
        for o in self.outputs.all().iter().filter(|o| o.enabled) {
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
        self.present();
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
                (self.emit)(Event::Changed {
                    kind: "outputs".into(),
                    id: id.uuid().to_string(),
                });
            }
        }
    }

    /// Displays came or went: place display outputs again.
    pub fn displays_changed(&mut self) {
        self.place_windows();
    }

    /// Drives auto-advancing items. Call every 100 ms or so.
    ///
    /// `LiveEngine::tick` is still `todo!("T34")`, so this does nothing
    /// yet; T34 turns the call on.
    pub fn tick(&mut self) {
        const ENGINE_TICKS: bool = false;
        if ENGINE_TICKS {
            let events = self.live.tick(now());
            if !events.is_empty() {
                self.after_live(events);
            }
        }
    }

    /// Documents changed here or on another device: tell the UI, and
    /// reload the open service if it, one of its songs or a theme changed.
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
        });
        if affects {
            if let Err(e) = self.load_service(service) {
                eprintln!("lampstand: reloading the service failed: {e}");
            }
        }
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

fn slide_view(slide: &LoadedSlide) -> SlideView {
    let (label, lines) = match &slide.content {
        Content::Lyrics { slide, .. } | Content::Scripture { slide } | Content::Text { slide } => {
            (slide.label.clone(), slide.lines.clone())
        }
        Content::DeckSlide { index, .. } => (Some(format!("Slide {}", index + 1)), Vec::new()),
        Content::Image { .. } => (Some("Image".into()), Vec::new()),
        Content::Video { .. } => (Some("Video".into()), Vec::new()),
        Content::Countdown { label, .. } => (Some("Countdown".into()), vec![label.clone()]),
    };
    SlideView {
        label,
        lines,
        steps: slide.steps,
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

fn now() -> Timestamp {
    Timestamp(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64),
    )
}
