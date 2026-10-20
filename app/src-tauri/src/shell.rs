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
//!   changes from sync, display hot-plug polling and the 100 ms live tick.
//!   A library change wakes it at once, so a synced edit reloads the open
//!   service without waiting for the tick.
//! - **Sync** (`crate::sync`) runs on Tauri's async runtime: the local
//!   server, the sync sessions and the `Team` requests. It never locks
//!   `App`; remote changes reach it through the library's change feed.
//! - The **render thread** (`lampstand-render`) owns the GPU and every
//!   surface, and reports through a callback that only sends on a channel.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use lampstand_api::{Event, LibraryRequest, Request, Response, ServiceRequest};
use lampstand_connectors::song_databases::download_worship_leader_app;
use lampstand_core::id::BlobHash;
use lampstand_core::output::OutputId;
use lampstand_layout::Fonts;
use lampstand_library::Library;
use lampstand_render::output::DisplayInfo;
use lampstand_render::{Background, BlobSource, OutputEvent, OutputOptions, Outputs, Renderer};
use lampstand_sync::DocChanged;
use tauri::window::{Color, Window, WindowBuilder};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WindowEvent};

use crate::config::device_key;
use crate::decks;
use crate::handler::{App, EventSink, Placement, WindowHost};
use crate::import::import_files;
use crate::sync::{Sync, SyncConfig};

/// Everything that reaches the app thread.
enum Internal {
    Resized(OutputId, u32, u32),
    Closed(OutputId),
    Render(OutputEvent),
    /// The library changed; the batch is read from the change feed.
    LibraryChanged,
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
    /// Draws slide thumbnails, apart from the output render thread so a
    /// thumbnail never holds up a frame. `None` if it could not start.
    thumbnails: Option<Arc<Renderer>>,
}

fn lock(app: &Mutex<App>) -> MutexGuard<'_, App> {
    app.lock().unwrap_or_else(|e| e.into_inner())
}

/// The one command the UI calls. Runs on the blocking pool so composing a
/// slide never holds up the async runtime or the event loop. Imports and
/// downloads run without the `App` lock.
#[tauri::command]
async fn api(state: tauri::State<'_, Shared>, request: Request) -> Result<Response, String> {
    let library = state.library.clone();
    let emit = state.emit.clone();
    let blocking = |f: Box<dyn FnOnce() -> Response + Send>| async move {
        tauri::async_runtime::spawn_blocking(f)
            .await
            .map_err(|e| e.to_string())
    };
    match request {
        Request::Team(request) => Ok(match &state.sync {
            Some(sync) => sync.handle(request).await,
            None => Response::Error {
                message: "sync is off: the local server could not start".into(),
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
        .run(tauri::generate_context!())
        .expect("error while running Lampstand");
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

    let windows = Arc::new(Mutex::new(HashMap::new()));
    let host = TauriWindows {
        handle: handle.clone(),
        outputs: outputs.clone(),
        windows: windows.clone(),
        tx: tx.clone(),
    };
    let emitter = handle.clone();
    let emit = Arc::new(move |event: Event| {
        let _ = emitter.emit("api-event", event);
    });
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
    let emit = app.emitter();
    let app = Arc::new(Mutex::new(app));

    let sync = match device_key(data_dir) {
        Ok(key) => tauri::async_runtime::block_on(Sync::start(
            &library,
            key,
            data_dir,
            SyncConfig::from_env(),
            emit.clone(),
        )),
        Err(e) => Err(e.to_string()),
    };
    let sync = match sync {
        Ok(sync) => Some(Arc::new(sync)),
        Err(e) => {
            eprintln!("lampstand: sync is off: {e}");
            None
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
        .spawn(move || app_thread(thread_app, host, rx, changes))
        .map_err(|e| e.to_string())?;
    Ok(Shared {
        app,
        library,
        emit,
        downloads: data_dir.join("downloads"),
        sync,
        thumbnails,
    })
}

fn app_thread(
    app: Arc<Mutex<App>>,
    host: TauriWindows,
    rx: Receiver<Internal>,
    changes: Receiver<DocChanged>,
) {
    const TICK: Duration = Duration::from_millis(100);
    let mut displays = host.displays();
    let mut polled = Instant::now();
    let mut ticked = Instant::now();
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
                message,
                &changes,
                &mut displays,
                &mut polled,
                &mut ticked,
            )
        });
        if std::panic::catch_unwind(step).is_err() {
            eprintln!("lampstand: the app thread recovered from a panic");
        }
    }
}

fn app_step(
    app: &Mutex<App>,
    host: &TauriWindows,
    message: Result<Internal, RecvTimeoutError>,
    changes: &Receiver<DocChanged>,
    displays: &mut Vec<DisplayInfo>,
    polled: &mut Instant,
    ticked: &mut Instant,
) {
    const TICK: Duration = Duration::from_millis(100);
    const DISPLAY_POLL: Duration = Duration::from_secs(1);
    {
        match message {
            Ok(Internal::Resized(id, w, h)) => lock(app).output_resized(id, w, h),
            Ok(Internal::Closed(id)) => lock(app).output_closed(id),
            Ok(Internal::LibraryChanged) => {}
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
            let now = host.displays();
            if now != *displays {
                *displays = now;
                lock(app).displays_changed();
            }
        }
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
        self.handle
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
            .collect()
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
            // Physical units, so the window covers the display exactly
            // whatever its scale factor.
            window
                .set_position(PhysicalPosition::new(d.x, d.y))
                .map_err(|e| e.to_string())?;
            window
                .set_size(PhysicalSize::new(d.width, d.height))
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
