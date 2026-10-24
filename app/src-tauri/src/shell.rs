//! The Tauri side: the `api` command and `api-event` stream, and output
//! windows without a webview, driven by T25's render thread.
//!
//! Threads:
//! - The **main thread** runs Tauri's event loop. It creates and destroys
//!   output windows, creates their surfaces (macOS wants that on the main
//!   thread) and forwards window events. It never locks `App`, so a slow
//!   request cannot stall the event loop, and a request waiting on the main
//!   thread (listing monitors) cannot deadlock.
//! - **`api` requests** run on Tauri's blocking pool and lock `App`.
//! - The **`lampstand-app` thread** locks `App` for everything that is not a
//!   request: window resizes and closes, render-thread events, library
//!   changes from sync, display hot-plug polling, the 100 ms live tick, and
//!   the clicker's global shortcuts (`crate::keys`): it presses them into
//!   the live engine and registers or releases them as clicker mode, the
//!   open service and output-window focus change.
//!   A library change wakes it at once, so a synced edit reloads the open
//!   service without waiting for the tick.
//! - **Sync** (`crate::sync`) runs on Tauri's async runtime: the local
//!   server, the sync sessions and the `Team` requests. It never locks
//!   `App`; remote changes reach it through the library's change feed.
//! - **OBS control** (`crate::obs_control`) runs there too: the OBS
//!   connection and the `Obs` requests. It reads whether any output
//!   streams from an atomic flag, never the `App` lock.
//! - The **render thread** (`lampstand-render`) owns the GPU and every
//!   surface, and reports through a callback that only sends on a channel.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use lampstand_api::{
    Event, LibraryRequest, Request, Response, ServiceRequest, UpdateRequest, UpdateState,
    UpdateStatus,
};
use lampstand_connectors::song_databases::download_worship_leader_app;
use lampstand_core::id::BlobHash;
use lampstand_core::output::OutputId;
use lampstand_layout::Fonts;
use lampstand_library::Library;
use lampstand_render::output::DisplayInfo;
use lampstand_render::{Background, BlobSource, OutputEvent, OutputOptions, Outputs, Renderer};
use lampstand_sync::DocChanged;
use tauri::webview::PageLoadEvent;
use tauri::window::{Color, Window, WindowBuilder};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, RunEvent,
    WindowEvent,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::config::device_key;
use crate::decks;
use crate::display_names;
use crate::handler::{App, EventSink, Placement, WindowHost};
use crate::import::import_files;
use crate::keys::{Key, Keys, ShortcutHost};
use crate::obs_control::{ObsControl, Os, System};
use crate::seed;
use crate::sync::{
    mirroring, public_address_on, AddressWatch, DriveSinks, NodeCell, Obs, Sync, SyncConfig,
};

/// Everything that reaches the app thread.
enum Internal {
    Resized(OutputId, u32, u32),
    Closed(OutputId),
    Render(OutputEvent),
    /// The library changed; the batch is read from the change feed.
    LibraryChanged,
    /// A global shortcut was pressed (T56).
    Key(Key),
    /// An output window gained or lost focus.
    Focused(OutputId, bool),
    /// Sync says whether this device presents (`true`) or only plans.
    Presenting(bool),
    /// The device driving this one sent a request.
    Drive(Request),
    /// A device started driving this one: tell it everything on air.
    Announce,
}

struct Shared {
    app: Arc<Mutex<App>>,
    /// Imports use the library directly, without the `App` lock, so live
    /// control keeps working through a minutes-long song-database import.
    library: Arc<Library>,
    emit: EventSink,
    downloads: PathBuf,
    /// `None` when the local server could not start.
    sync: Option<Arc<Sync>>,
    /// Why `sync` is `None`, for the status bar.
    sync_error: Option<String>,
    /// OBS over its WebSocket (T89). Answers `Obs` requests without the
    /// `App` lock.
    obs: Arc<ObsControl>,
    /// Draws slide thumbnails, apart from the output render thread so a
    /// thumbnail never holds up a frame. `None` if it could not start.
    thumbnails: Option<Arc<Renderer>>,
    /// Fonts for parsing a deck to draw its thumbnail: loaded when the first
    /// one is asked for, and used one at a time.
    thumbnail_fonts: Arc<Mutex<Option<Fonts>>>,
    /// Where updating Lampstand itself stands: what the last check found.
    update: Arc<tokio::sync::Mutex<UpdaterState>>,
}

/// What the last check for a new release found, and the release to install.
struct UpdaterState {
    state: UpdateState,
    pending: Option<tauri_plugin_updater::Update>,
}

fn lock(app: &Mutex<App>) -> MutexGuard<'_, App> {
    app.lock().unwrap_or_else(|e| e.into_inner())
}

/// The one command the UI calls. Runs on the blocking pool so composing a
/// slide never holds up the async runtime or the event loop. Imports and
/// downloads run without the `App` lock.
#[tauri::command]
async fn api(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, Shared>,
    request: Request,
) -> Result<Response, String> {
    let library = state.library.clone();
    let emit = state.emit.clone();
    let blocking = |f: Box<dyn FnOnce() -> Response + Send>| async move {
        tauri::async_runtime::spawn_blocking(f)
            .await
            .map_err(|e| e.to_string())
    };
    // A device that is driving another sends what the operator does there.
    if let Some(sync) = &state.sync {
        if let Some(response) = sync.forward(&request).await {
            return Ok(response);
        }
    }
    match request {
        Request::Obs(request) => {
            // A scene or mute chosen by hand is carried out by the device
            // that owns OBS (a driver's choice is forwarded above).
            let here = state.sync.as_ref().is_none_or(|s| s.node().obs_here());
            if !here && crate::stream_cues::switches_obs(&request) {
                return Ok(Response::Error {
                    message: crate::stream_cues::NOT_THE_STREAM.into(),
                });
            }
            let response = state.obs.handle(request).await;
            Ok(crate::stream_cues::shape_response(response, here))
        }
        Request::Update(request) => Ok(update_request(&app_handle, &state, request).await),
        Request::About(request) => {
            let url = crate::help::page(
                &request,
                &app_handle.package_info().version.to_string(),
                crate::help::os_name(),
                std::env::consts::ARCH,
            );
            blocking(Box::new(move || {
                match crate::obs_control::open_url(&System, Os::current(), &url) {
                    Ok(()) => Response::Ok,
                    Err(message) => Response::Error { message },
                }
            }))
            .await
        }
        Request::Team(request) => Ok(match &state.sync {
            Some(sync) => sync.handle(request).await,
            None => Response::Error {
                message: format!(
                    "Sync is off: {}",
                    state.sync_error.as_deref().unwrap_or("it did not start")
                ),
            },
        }),
        Request::Library(LibraryRequest::ImportFiles { paths }) => {
            blocking(Box::new(move || {
                Response::Text(import_files(&library, &paths, &*emit))
            }))
            .await
        }
        Request::Service(ServiceRequest::SlideThumbnail {
            item,
            slide,
            step,
            width,
        }) => {
            let Some(renderer) = state.thumbnails.clone() else {
                return Ok(Response::Error {
                    message: "thumbnails are off: no GPU for them".into(),
                });
            };
            let app = state.app.clone();
            blocking(Box::new(move || {
                let Some(scene) = lock(&app).slide_scene(item, slide, step) else {
                    return Response::Error {
                        message: "no picture for this slide".into(),
                    };
                };
                let (w, h) = decks::thumbnail_size(&scene, width);
                renderer
                    .render_offscreen(&scene, w, h)
                    .map_err(|e| format!("{e:?}"))
                    .and_then(|image| decks::png_data_url(&image))
                    .map(Response::Thumbnail)
                    .unwrap_or_else(|message| Response::Error { message })
            }))
            .await
        }
        // Parsing a deck, copying a video and downloading a link all run
        // off the `App` lock, like imports.
        Request::Library(LibraryRequest::AddDeckFile { path }) => {
            blocking(Box::new(move || {
                decks::add_deck_file(&library, &path)
                    .map(Response::Deck)
                    .unwrap_or_else(|message| Response::Error { message })
            }))
            .await
        }
        // A deck's first slide, drawn like a service's slide thumbnails but
        // from the library, off the `App` lock.
        Request::Library(LibraryRequest::DeckThumbnail { deck, width }) => {
            let Some(renderer) = state.thumbnails.clone() else {
                return Ok(Response::Error {
                    message: "thumbnails are off: no GPU for them".into(),
                });
            };
            let fonts = state.thumbnail_fonts.clone();
            blocking(Box::new(move || {
                decks::deck_thumbnail(&library, deck, width, &fonts, &renderer)
                    .map(Response::Thumbnail)
                    .unwrap_or_else(|message| Response::Error { message })
            }))
            .await
        }
        Request::Library(LibraryRequest::DeckSlidePicture { deck, slide, width }) => {
            let Some(renderer) = state.thumbnails.clone() else {
                return Ok(Response::Error {
                    message: "thumbnails are off: no GPU for them".into(),
                });
            };
            let fonts = state.thumbnail_fonts.clone();
            blocking(Box::new(move || {
                decks::deck_slide_picture(&library, deck, slide, width, &fonts, &renderer)
                    .map(|(thumbnail, slides)| Response::DeckSlide { thumbnail, slides })
                    .unwrap_or_else(|message| Response::Error { message })
            }))
            .await
        }
        // A theme drawn with sample text, by the same layout and renderer
        // as the screens.
        Request::Library(LibraryRequest::ThemePreview {
            theme,
            sample,
            width,
            height,
            thumb_width,
        }) => {
            let Some(renderer) = state.thumbnails.clone() else {
                return Ok(Response::Error {
                    message: "previews are off: no GPU for them".into(),
                });
            };
            let fonts = state.thumbnail_fonts.clone();
            blocking(Box::new(move || {
                crate::theme_preview::preview(
                    &theme,
                    sample,
                    (width, height),
                    thumb_width,
                    &fonts,
                    &renderer,
                )
                .map(Response::Thumbnail)
                .unwrap_or_else(|message| Response::Error { message })
            }))
            .await
        }
        // The default idle screen, by the same layout and renderer as the
        // screens.
        Request::Library(LibraryRequest::IdlePreview {
            brand,
            theme,
            width,
        }) => {
            let Some(renderer) = state.thumbnails.clone() else {
                return Ok(Response::Error {
                    message: "previews are off: no GPU for them".into(),
                });
            };
            let fonts = state.thumbnail_fonts.clone();
            blocking(Box::new(move || {
                let brand = crate::theme_preview::brand_for_idle(&library, brand);
                let theme = library.resolve_theme(&[theme]);
                crate::theme_preview::idle_preview(brand.as_ref(), &theme, width, &fonts, &renderer)
                    .map(Response::Thumbnail)
                    .unwrap_or_else(|message| Response::Error { message })
            }))
            .await
        }
        // Decoding a video for its thumbnail runs off the `App` lock.
        Request::Library(LibraryRequest::MediaThumbnail { blob, name, width }) => {
            blocking(Box::new(move || {
                decks::media_thumbnail(&library, &blob, &name, width)
                    .map(Response::Thumbnail)
                    .unwrap_or_else(|message| Response::Error { message })
            }))
            .await
        }
        Request::Library(LibraryRequest::AddMediaFile { path }) => {
            blocking(Box::new(move || {
                decks::add_media_file(&library, &path)
                    .map(Response::Media)
                    .unwrap_or_else(|message| Response::Error { message })
            }))
            .await
        }
        Request::Library(LibraryRequest::AddDeckFromLink { url }) => {
            let link = match decks::classify(&url) {
                Ok(link) => link,
                Err(message) => return Ok(Response::Error { message }),
            };
            emit(Event::ImportProgress {
                done: 0,
                total: 1,
                message: "Downloading the presentation".into(),
            });
            let done = |message: String| {
                emit(Event::ImportProgress {
                    done: 1,
                    total: 1,
                    message,
                })
            };
            let fetched = match decks::fetch_link(&link).await {
                Ok(fetched) => fetched,
                Err(why) => {
                    let message = format!("Could not add the deck: {why}");
                    done(message.clone());
                    return Ok(Response::Error { message });
                }
            };
            let response = blocking(Box::new(move || {
                decks::add_fetched(&library, &link, &url, fetched)
                    .map(Response::Deck)
                    .unwrap_or_else(|message| Response::Error { message })
            }))
            .await;
            done(match &response {
                Ok(Response::Deck(deck)) => format!("Added {}", deck.title),
                Ok(Response::Error { message }) => format!("Could not add the deck: {message}"),
                _ => String::new(),
            });
            response
        }
        Request::Library(LibraryRequest::DownloadSongDatabase { code }) => {
            emit(Event::ImportProgress {
                done: 0,
                total: 1,
                message: format!("Downloading the {code} song database"),
            });
            let path = match download_worship_leader_app(&code, &state.downloads).await {
                Ok(path) => path,
                Err(e) => {
                    use lampstand_connectors::FetchError;
                    let why = match e {
                        FetchError::Network(why) => why,
                        FetchError::Forbidden => "the site refused the download".into(),
                        FetchError::NotFound => format!("there is no {code} song database"),
                        FetchError::NotADeck(why) => why,
                    };
                    return Ok(Response::Error {
                        message: format!("Download failed: {why}"),
                    });
                }
            };
            let paths = vec![path.display().to_string()];
            blocking(Box::new(move || {
                Response::Text(import_files(&library, &paths, &*emit))
            }))
            .await
        }
        request => {
            let app = state.app.clone();
            blocking(Box::new(move || lock(&app).handle(request))).await
        }
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![api])
        .setup(|tauri_app| {
            let handle = tauri_app.handle().clone();
            let data_dir = match std::env::var_os("LAMPSTAND_DATA_DIR") {
                Some(dir) => PathBuf::from(dir),
                None => tauri_app.path().app_data_dir()?,
            };
            eprintln!("lampstand: data in {}", data_dir.display());
            let shared = start(handle, &data_dir)?;
            tauri_app.manage(shared);
            Ok(())
        })
        .on_page_load(|webview, payload| {
            // A service restored after a crash was loaded before the UI
            // was listening; a reloaded webview has forgotten it.
            // This runs on the main thread, which never locks `App`.
            if payload.event() == PageLoadEvent::Finished {
                if let Some(shared) = webview.try_state::<Shared>() {
                    let app = shared.app.clone();
                    tauri::async_runtime::spawn_blocking(move || lock(&app).announce());
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Lampstand")
        .run(|handle, event| {
            if let RunEvent::Exit = event {
                // A clean quit: do not reopen the service next time. The
                // main thread never locks `App` (a request may be waiting
                // on it), so another thread does, and the quit waits for it
                // only briefly; if it times out, the next start restores.
                if let Some(shared) = handle.try_state::<Shared>() {
                    let app = shared.app.clone();
                    let (done, wait) = mpsc::channel();
                    std::thread::spawn(move || {
                        lock(&app).closing();
                        let _ = done.send(());
                    });
                    let _ = wait.recv_timeout(Duration::from_secs(1));
                }
            }
        });
}

/// Starts the render thread, opens the app state and starts the app thread.
fn start(handle: AppHandle, data_dir: &Path) -> Result<Shared, String> {
    let (tx, rx) = mpsc::channel();

    let blobs = Arc::new(Blobs(data_dir.join("library/repo/blobs")));
    let renderer = Renderer::new(blobs.clone()).map_err(|e| format!("{e:?}"))?;
    let info = renderer.adapter_info();
    eprintln!(
        "lampstand: rendering on {} ({}){}",
        info.name,
        info.backend,
        if info.software { ", software" } else { "" }
    );
    let thumbnails = match Renderer::new(blobs) {
        Ok(r) => Some(Arc::new(r)),
        Err(e) => {
            eprintln!("lampstand: no thumbnails: {e:?}");
            None
        }
    };
    let render_tx = Mutex::new(tx.clone());
    let outputs = Arc::new(
        Outputs::spawn(renderer, OutputOptions::default(), move |event| {
            let _ = render_tx
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .send(Internal::Render(event));
        })
        .map_err(|e| format!("{e:?}"))?,
    );

    // Global shortcuts for clickers (T56). A press only goes to the app
    // thread: the handler runs on the main thread, which never locks `App`.
    let key_tx = tx.clone();
    let shortcuts = handle
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |_, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    if let Some(key) = Key::ALL.into_iter().find(|k| shortcut_for(*k) == *shortcut)
                    {
                        let _ = key_tx.send(Internal::Key(key));
                    }
                })
                .build(),
        )
        .inspect_err(|e| eprintln!("lampstand: no clicker shortcuts: {e}"))
        .is_ok();
    let keys = Keys::new(TauriShortcuts {
        handle: handle.clone(),
        enabled: shortcuts,
    });

    let windows = Arc::new(Mutex::new(HashMap::new()));
    let host = TauriWindows {
        handle: handle.clone(),
        outputs: outputs.clone(),
        windows: windows.clone(),
        tx: tx.clone(),
    };
    let emitter = handle.clone();
    // Events also go to a device driving this one, once there is one.
    let node_cell = NodeCell::default();
    let emit = mirroring(
        Arc::new(move |event: Event| {
            let _ = emitter.emit("api-event", event);
        }),
        node_cell.clone(),
    );
    // A folder that has never been seeded is a brand-new install, which
    // starts with a welcome service. Known before `App::open` seeds it.
    let first_run = !data_dir.join("seeded").exists();
    let app = App::open(
        data_dir,
        Fonts::load(),
        outputs,
        Box::new(host.clone()),
        emit,
    )?;
    let changes = app.library().subscribe();
    let wake = app.library().subscribe();
    let library = app.library().clone();
    if let Err(e) = seed::seed_welcome_service(&library, data_dir, first_run) {
        eprintln!("lampstand: cannot add the welcome service: {e}");
    }
    let emit = app.emitter();
    let obs = Obs {
        hub: app.scene_hub().clone(),
        renderer: thumbnails.clone(),
    };
    let streaming = app.streaming();
    let app = Arc::new(Mutex::new(app));
    // OBS belongs to the Stream holder (`crate::stream_cues`): its events
    // are shaped for whether this device owns OBS, and the cue each live item
    // makes goes to this device's OBS or the Stream holder's.
    let obs_control = tauri::async_runtime::block_on(async {
        ObsControl::start(
            data_dir,
            streaming,
            Arc::new(System),
            Os::current(),
            crate::stream_cues::shaping(emit.clone(), node_cell.clone()),
        )
    });
    let obs_view = obs_control.clone();
    let cues = tauri::async_runtime::block_on(async {
        crate::stream_cues::start_router(
            node_cell.clone(),
            obs_control.cues(),
            Arc::new(move || obs_view.view()),
            emit.clone(),
        )
    });
    lock(&app).set_obs_cues(cues);
    // "Set up OBS" asks the app for a picture's address and size, so the
    // window never supplies the address OBS is pointed at.
    let picture_app = app.clone();
    obs_control.set_pictures(Arc::new(move |output| {
        lock(&picture_app).obs_picture(output)
    }));

    let role_tx = tx.clone();
    let drive_tx = tx.clone();
    let announce_tx = tx.clone();
    let drive_obs = obs_control.clone();
    let announce_obs = obs_control.clone();
    let announce_emit = crate::stream_cues::shaping(emit.clone(), node_cell.clone());
    let drive_cell = node_cell.clone();
    let drive = DriveSinks {
        command: Arc::new(move |request| match request {
            // OBS answers on the async runtime, without the `App` lock.
            Request::Obs(request) => {
                if !crate::stream_cues::obs_here(&drive_cell) {
                    eprintln!(
                        "lampstand: ignored an OBS request from a driver: OBS is not run here"
                    );
                    return;
                }
                let obs = drive_obs.clone();
                tauri::async_runtime::spawn(async move {
                    obs.handle(request).await;
                });
            }
            request => {
                let _ = drive_tx.send(Internal::Drive(request));
            }
        }),
        driver: Arc::new(move |on| {
            if on {
                let _ = announce_tx.send(Internal::Announce);
                announce_emit(Event::Obs(Box::new(announce_obs.view())));
            }
        }),
        cell: node_cell,
    };
    let sync = match device_key(data_dir) {
        Ok(key) => tauri::async_runtime::block_on(Sync::start(
            &library,
            key,
            data_dir,
            SyncConfig::from_env(),
            obs,
            emit.clone(),
            Arc::new(move |presenting| {
                let _ = role_tx.send(Internal::Presenting(presenting));
            }),
            drive,
        )),
        Err(e) => Err(e.to_string()),
    };
    let mut address_watch = None;
    let (sync, sync_error) = match sync {
        Ok(sync) => {
            crate::stream_cues::accept(sync.node(), obs_control.cues());
            let address = sync.public_address();
            lock(&app).set_server_address(address.clone());
            lock(&app).set_sync_link(sync.link());
            let sync = Arc::new(sync);
            let port = sync.local_addr().port();
            address_watch = Some(AddressWatch::new(address, move || public_address_on(port)));
            (Some(sync), None)
        }
        Err(e) => {
            eprintln!("lampstand: sync is off: {e}");
            (None, Some(e))
        }
    };
    let wake_tx = tx.clone();
    std::thread::Builder::new()
        .name("lampstand-changes".into())
        .spawn(move || {
            while wake.recv().is_ok() {
                if wake_tx.send(Internal::LibraryChanged).is_err() {
                    return;
                }
            }
        })
        .map_err(|e| e.to_string())?;

    let thread_app = app.clone();
    std::thread::Builder::new()
        .name("lampstand-app".into())
        .spawn(move || app_thread(thread_app, host, keys, rx, changes, address_watch))
        .map_err(|e| e.to_string())?;
    Ok(Shared {
        app,
        library,
        emit,
        downloads: data_dir.join("downloads"),
        sync,
        sync_error,
        obs: obs_control,
        thumbnails,
        thumbnail_fonts: Arc::new(Mutex::new(None)),
        update: Arc::new(tokio::sync::Mutex::new(UpdaterState {
            state: UpdateState::NotChecked,
            pending: None,
        })),
    })
}

/// Looks for a newer release, or installs the one found. An install is
/// refused unless nothing is on air and no service starts within the hour
/// (`update::blocker`), judged now and not from what the screen showed.
async fn update_request(
    app: &tauri::AppHandle,
    state: &Shared,
    request: UpdateRequest,
) -> Response {
    use tauri_plugin_updater::UpdaterExt;

    let status = |u: &UpdaterState| {
        Response::Update(UpdateStatus {
            current: app.package_info().version.to_string(),
            state: u.state.clone(),
            blocked: lock(&state.app).update_blocker(),
        })
    };

    match request {
        UpdateRequest::Status => status(&*state.update.lock().await),
        UpdateRequest::Check => {
            let mut u = state.update.lock().await;
            let found = match app.updater() {
                Err(e) => Err(e.to_string()),
                Ok(updater) => updater.check().await.map_err(|e| e.to_string()),
            };
            match found {
                Ok(Some(update)) => {
                    u.state = UpdateState::Available {
                        version: update.version.clone(),
                        notes: update.body.clone().filter(|n| !n.trim().is_empty()),
                    };
                    u.pending = Some(update);
                }
                Ok(None) => {
                    u.state = UpdateState::UpToDate;
                    u.pending = None;
                }
                Err(message) => u.state = UpdateState::Failed { message },
            }
            status(&u)
        }
        UpdateRequest::Install => {
            if let Some(message) = lock(&state.app).update_blocker() {
                return Response::Error { message };
            }
            let update = {
                let mut u = state.update.lock().await;
                let Some(update) = u.pending.take() else {
                    return Response::Error {
                        message: "There is no update to install. Check for updates first.".into(),
                    };
                };
                u.state = UpdateState::Installing;
                update
            };
            // Not under the lock: it can take a while, and the screen asks
            // for the status as it goes.
            match update.download_and_install(|_, _| {}, || {}).await {
                Ok(()) => app.restart(),
                Err(e) => {
                    let mut u = state.update.lock().await;
                    u.state = UpdateState::Failed {
                        message: e.to_string(),
                    };
                    status(&u)
                }
            }
        }
    }
}

fn app_thread(
    app: Arc<Mutex<App>>,
    host: TauriWindows,
    keys: Keys<TauriShortcuts>,
    rx: Receiver<Internal>,
    changes: Receiver<DocChanged>,
    mut address: Option<AddressWatch>,
) {
    const TICK: Duration = Duration::from_millis(100);
    let mut polled = Instant::now();
    let mut ticked = Instant::now();
    let mut clicker = Clicker {
        keys,
        focused: HashSet::new(),
    };
    loop {
        // Messages must not postpone the tick: wait only for what is left
        // of this one.
        let message = rx.recv_timeout(TICK.saturating_sub(ticked.elapsed()));
        if let Err(RecvTimeoutError::Disconnected) = message {
            return;
        }
        // One bad message must not stop the thread that shows windows on
        // their first frame and keeps sizes current.
        let step = std::panic::AssertUnwindSafe(|| {
            app_step(
                &app,
                &host,
                &mut clicker,
                message,
                &changes,
                &mut polled,
                &mut ticked,
                address.as_mut(),
            )
        });
        if std::panic::catch_unwind(step).is_err() {
            eprintln!("lampstand: the app thread recovered from a panic");
        }
    }
}

/// The global shortcuts the app thread holds, and the output windows with
/// focus.
struct Clicker {
    keys: Keys<TauriShortcuts>,
    focused: HashSet<OutputId>,
}

#[allow(clippy::too_many_arguments)]
fn app_step(
    app: &Mutex<App>,
    host: &TauriWindows,
    clicker: &mut Clicker,
    message: Result<Internal, RecvTimeoutError>,
    changes: &Receiver<DocChanged>,
    polled: &mut Instant,
    ticked: &mut Instant,
    address: Option<&mut AddressWatch>,
) {
    const TICK: Duration = Duration::from_millis(100);
    const DISPLAY_POLL: Duration = Duration::from_secs(1);
    {
        match message {
            Ok(Internal::Resized(id, w, h)) => lock(app).output_resized(id, w, h),
            Ok(Internal::Closed(id)) => {
                clicker.focused.remove(&id);
                lock(app).output_closed(id)
            }
            Ok(Internal::LibraryChanged) => {}
            Ok(Internal::Presenting(presenting)) => lock(app).set_presenting(presenting),
            Ok(Internal::Drive(request)) => {
                if let Response::Error { message } = lock(app).handle(request) {
                    eprintln!("lampstand: a driver's request failed: {message}");
                }
            }
            Ok(Internal::Announce) => lock(app).announce(),
            Ok(Internal::Key(key)) => {
                if let Response::Error { message } = lock(app).handle(Request::Live(key.command()))
                {
                    eprintln!("lampstand: {key:?}: {message}");
                }
            }
            Ok(Internal::Focused(id, true)) => {
                clicker.focused.insert(id);
            }
            Ok(Internal::Focused(id, false)) => {
                clicker.focused.remove(&id);
            }
            Ok(Internal::Render(event)) => match event {
                OutputEvent::FirstFrame(id) => host.first_frame(id),
                OutputEvent::Presented(_) => {}
                OutputEvent::SurfaceLost { output, reason } => {
                    eprintln!(
                        "lampstand: output {} lost its surface: {reason}",
                        output.uuid()
                    )
                }
                OutputEvent::Restarted {
                    output,
                    message,
                    blanked,
                } => eprintln!(
                    "lampstand: render thread recovered{} from: {message}{}",
                    output
                        .map(|o| format!(" output {}", o.uuid()))
                        .unwrap_or_default(),
                    if blanked { " (now black)" } else { "" }
                ),
            },
            Err(_) => {}
        }
        if ticked.elapsed() >= TICK {
            *ticked = Instant::now();
            lock(app).tick();
        }
        let batch: Vec<DocChanged> = changes.try_iter().collect();
        if !batch.is_empty() {
            lock(app).library_changed(&batch);
        }
        if polled.elapsed() >= DISPLAY_POLL {
            *polled = Instant::now();
            // Tauri has no monitor hot-plug event, so poll.
            // Against what the windows were placed for, so a display that
            // appeared while the app was starting is not missed.
            let now = host.displays();
            lock(app).displays_polled(now);
        }
        // A network that came up after the start gives OBS a better
        // address. The probe takes no lock.
        if let Some(found) = address.and_then(|w| w.check(Instant::now())) {
            eprintln!("lampstand: the local server is now at {found}");
            lock(app).set_server_address(found);
        }
        // Without the `App` lock: registering waits on the main thread.
        let mut state = lock(app).key_state();
        state.output_focused = !clicker.focused.is_empty();
        clicker.keys.update(state);
    }
}

/// The global shortcut for a key: the key alone, except the item jumps,
/// which are Shift+Up and Shift+Down as in the operator window.
fn shortcut_for(key: Key) -> Shortcut {
    let (modifiers, code) = match key {
        Key::PageUp => (None, Code::PageUp),
        Key::PageDown => (None, Code::PageDown),
        Key::B => (None, Code::KeyB),
        Key::Period => (None, Code::Period),
        Key::Left => (None, Code::ArrowLeft),
        Key::Right => (None, Code::ArrowRight),
        Key::Space => (None, Code::Space),
        Key::ShiftUp => (Some(Modifiers::SHIFT), Code::ArrowUp),
        Key::ShiftDown => (Some(Modifiers::SHIFT), Code::ArrowDown),
    };
    Shortcut::new(modifiers, code)
}

/// `tauri-plugin-global-shortcut` behind `keys::ShortcutHost`.
struct TauriShortcuts {
    handle: AppHandle,
    /// False if the plugin did not start; every registration then fails.
    enabled: bool,
}

impl ShortcutHost for TauriShortcuts {
    fn register(&mut self, key: Key) -> Result<(), String> {
        if !self.enabled {
            return Err("global shortcuts are not available".into());
        }
        self.handle
            .global_shortcut()
            .register(shortcut_for(key))
            .map_err(|e| e.to_string())
    }

    fn unregister(&mut self, key: Key) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        self.handle
            .global_shortcut()
            .unregister(shortcut_for(key))
            .map_err(|e| e.to_string())
    }
}

/// Blob files by hash, straight from the repo's blob folder, so the render
/// thread never touches `App`.
struct Blobs(PathBuf);

impl BlobSource for Blobs {
    fn path(&self, blob: &BlobHash) -> Option<PathBuf> {
        let path = self.0.join(blob.to_string());
        path.is_file().then_some(path)
    }
}

struct OutputWindow {
    window: Window,
    /// Fullscreen on a display, rather than a normal window.
    display: bool,
}

/// Output windows as Tauri windows with no webview (`tauri::window`,
/// behind Tauri's `unstable` feature). A `tauri::Window` has raw window
/// and display handles, so it goes straight to `Outputs::attach`.
#[derive(Clone)]
struct TauriWindows {
    handle: AppHandle,
    outputs: Arc<Outputs>,
    windows: Arc<Mutex<HashMap<OutputId, OutputWindow>>>,
    tx: Sender<Internal>,
}

impl TauriWindows {
    /// The scale factor of the monitor `display` describes, matched by
    /// position and size as `displays()` reports them.
    fn scale_of(&self, display: &DisplayInfo) -> Option<f64> {
        self.handle
            .available_monitors()
            .ok()?
            .iter()
            .find(|m| {
                (
                    m.position().x,
                    m.position().y,
                    m.size().width,
                    m.size().height,
                ) == (display.x, display.y, display.width, display.height)
            })
            .map(|m| m.scale_factor())
    }

    /// The display the operator window is on, if it has one.
    fn operator_display(&self) -> Option<DisplayInfo> {
        let monitor = self
            .handle
            .get_webview_window("main")?
            .current_monitor()
            .ok()??;
        Some(DisplayInfo {
            name: String::new(),
            width: monitor.size().width,
            height: monitor.size().height,
            x: monitor.position().x,
            y: monitor.position().y,
        })
    }

    /// What the OS calls each display. AppKit only answers on the main
    /// thread; if it does not in time, the generic names stand.
    fn native_displays(&self) -> Vec<display_names::Native> {
        let (tx, rx) = mpsc::channel();
        let sent = self.handle.run_on_main_thread(move || {
            let _ = tx.send(display_names::native_displays());
        });
        if sent.is_err() {
            return Vec::new();
        }
        rx.recv_timeout(Duration::from_secs(1)).unwrap_or_default()
    }

    fn label(id: OutputId) -> String {
        format!("output-{}", id.uuid().simple())
    }

    /// The first frame is drawn: show the window, go fullscreen on its
    /// display (simple fullscreen on macOS: no new Space, no animation)
    /// and hide the cursor over it.
    fn first_frame(&self, id: OutputId) {
        let windows = self.windows.clone();
        let _ = self.handle.run_on_main_thread(move || {
            let windows = windows.lock().unwrap_or_else(|e| e.into_inner());
            let Some(w) = windows.get(&id) else {
                return;
            };
            let _ = w.window.show();
            if w.display {
                let _ = w.window.set_simple_fullscreen(true);
                let _ = w.window.set_cursor_visible(false);
            }
        });
    }
}

impl WindowHost for TauriWindows {
    fn displays(&self) -> Vec<DisplayInfo> {
        let mut displays: Vec<DisplayInfo> = self
            .handle
            .available_monitors()
            .unwrap_or_default()
            .iter()
            .map(|m| DisplayInfo {
                name: m.name().cloned().unwrap_or_default(),
                width: m.size().width,
                height: m.size().height,
                x: m.position().x,
                y: m.position().y,
            })
            .collect();
        display_names::without_operator(&mut displays, self.operator_display());
        display_names::apply(&mut displays, &self.native_displays());
        displays
    }

    fn open(&mut self, output: OutputId, title: &str, placement: Placement) -> Result<(), String> {
        self.close(output);
        let host = self.clone();
        let title = format!("{title} — Lampstand output");
        self.handle
            .run_on_main_thread(move || {
                if let Err(e) = host.create(output, &title, &placement) {
                    eprintln!("lampstand: cannot open output window: {e}");
                }
            })
            .map_err(|e| e.to_string())
    }

    fn close(&mut self, output: OutputId) {
        let Some(w) = self
            .windows
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&output)
        else {
            return;
        };
        // The render thread lets go of the surface before the window goes.
        self.outputs.detach(output);
        let _ = self.handle.run_on_main_thread(move || {
            let _ = w.window.destroy();
        });
    }
}

impl TauriWindows {
    /// Puts a rehearsal window in the bottom-right corner of the operator
    /// window's screen. macOS never presents to a window that is completely
    /// covered (wgpu reports it occluded), so one opened behind the
    /// operator window would look frozen until uncovered.
    fn place_in_corner(&self, window: &Window) {
        let Some(main) = self.handle.get_webview_window("main") else {
            return;
        };
        let (Ok(Some(monitor)), Ok(size)) = (main.current_monitor(), window.outer_size()) else {
            return;
        };
        let margin = (24.0 * monitor.scale_factor()) as i32;
        let x = monitor.position().x + monitor.size().width as i32 - size.width as i32 - margin;
        let y = monitor.position().y + monitor.size().height as i32 - size.height as i32 - margin;
        let _ = window.set_position(PhysicalPosition::new(
            x.max(monitor.position().x),
            y.max(monitor.position().y),
        ));
    }

    /// On the main thread: builds the window hidden and black, attaches its
    /// surface, and forwards its resizes and close requests.
    fn create(&self, output: OutputId, title: &str, placement: &Placement) -> Result<(), String> {
        let mut builder = WindowBuilder::new(&self.handle, Self::label(output))
            .title(title)
            .visible(false)
            .focused(false)
            .background_color(Color(0, 0, 0, 255));
        builder = match placement {
            Placement::Window { width, height } => {
                builder.inner_size(f64::from(*width), f64::from(*height))
            }
            Placement::Display(_) => builder
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true),
        };
        let window = builder.build().map_err(|e| e.to_string())?;
        if let Placement::Window { .. } = placement {
            self.place_in_corner(&window);
        }
        if let Placement::Display(d) = placement {
            // In logical points, converted with the display's own scale. A
            // physical position would be converted with the scale of the
            // display the window starts on (the operator's), which puts a
            // window for a display of a different scale on the wrong
            // screen, and fullscreen then blacks out the operator's.
            let scale = self
                .scale_of(d)
                .or_else(|| window.scale_factor().ok())
                .unwrap_or(1.0);
            let (x, y, width, height) = d.logical_frame(scale);
            window
                .set_position(LogicalPosition::new(x, y))
                .map_err(|e| e.to_string())?;
            window
                .set_size(LogicalSize::new(width, height))
                .map_err(|e| e.to_string())?;
        }
        let size = window.inner_size().map_err(|e| e.to_string())?;
        self.outputs
            .attach(
                output,
                window.clone(),
                size.width,
                size.height,
                Background::Opaque,
            )
            .map_err(|e| format!("{e:?}"))?;
        self.outputs
            .set_audible(output, matches!(placement, Placement::Window { .. }));
        let _ = self
            .tx
            .send(Internal::Resized(output, size.width, size.height));

        let outputs = self.outputs.clone();
        let tx = self.tx.clone();
        window.on_window_event(move |event| match event {
            WindowEvent::Resized(size) => {
                outputs.resize(output, size.width, size.height);
                let _ = tx.send(Internal::Resized(output, size.width, size.height));
            }
            WindowEvent::CloseRequested { api, .. } => {
                // The app closes it, after the render thread lets go.
                api.prevent_close();
                let _ = tx.send(Internal::Closed(output));
            }
            // It has no webview to see keys, so the app thread takes the
            // operator keys for it while it has focus (T56).
            WindowEvent::Focused(focused) => {
                let _ = tx.send(Internal::Focused(output, *focused));
            }
            _ => {}
        });
        self.windows
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                output,
                OutputWindow {
                    window,
                    display: matches!(placement, Placement::Display(_)),
                },
            );
        Ok(())
    }
}
