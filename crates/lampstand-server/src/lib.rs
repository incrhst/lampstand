//! The local server: one HTTP and WebSocket endpoint for the operator page,
//! the output process, the stage display and the OBS stream page.
//!
//! PRD: LV-1, OB-1, OB-2, PS-7. Design and protocol: `docs/v0.1.md`.
//!
//! The pages in `web/` are compiled in for release builds; debug builds read
//! them from disk, so they can be edited without rebuilding.

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Multipart, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use image::ImageEncoder;
use serde::Deserialize;
use tokio::sync::watch;

use lampstand_show::protocol::{ClientMessage, Command, Role, ServerMessage, Snapshot};
use lampstand_show::{Library, Service, Show};

pub use lampstand_show::protocol::DEFAULT_PORT;

#[derive(rust_embed::Embed)]
#[folder = "web/"]
struct Web;

/// Rendered deck images kept in memory: enough for a few decks at two sizes.
const IMAGE_CACHE: usize = 400;

pub struct AppState {
    lib: RwLock<Library>,
    show: Mutex<Show>,
    snapshots: watch::Sender<Arc<String>>,
    images: Mutex<ImageCache>,
    data_dir: PathBuf,
}

#[derive(Default)]
struct ImageCache {
    map: HashMap<(String, usize, usize, u32), Arc<Vec<u8>>>,
    order: VecDeque<(String, usize, usize, u32)>,
}

impl ImageCache {
    fn get(&self, k: &(String, usize, usize, u32)) -> Option<Arc<Vec<u8>>> {
        self.map.get(k).cloned()
    }

    fn put(&mut self, k: (String, usize, usize, u32), v: Arc<Vec<u8>>) {
        if self.map.insert(k.clone(), v).is_none() {
            self.order.push_back(k);
        }
        while self.order.len() > IMAGE_CACHE {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl AppState {
    /// Open the library and the last service in `data_dir`. Returns start-up warnings
    /// (files that could not be read) alongside.
    pub fn open(data_dir: PathBuf) -> Result<(Arc<AppState>, Vec<String>), lampstand_show::LibraryError> {
        let (lib, warnings) = Library::open(&data_dir)?;
        let service = std::fs::read_to_string(service_path(&data_dir))
            .ok()
            .and_then(|t| serde_json::from_str::<Service>(&t).ok())
            .unwrap_or_else(|| Service { name: "Service".into(), items: Vec::new() });
        let show = Show::new(service, &lib);
        let first = Arc::new(String::new());
        let (tx, _) = watch::channel(first);
        let state = Arc::new(AppState { lib: RwLock::new(lib), show: Mutex::new(show), snapshots: tx, images: Mutex::default(), data_dir });
        state.publish();
        Ok((state, warnings))
    }

    pub fn snapshot(&self) -> Snapshot {
        let logo = self.lib.read().unwrap().logo_path().map(|_| format!("/media/logo.png?v={}", self.logo_version()));
        self.show.lock().unwrap().snapshot(logo)
    }

    fn logo_version(&self) -> u64 {
        let lib = self.lib.read().unwrap();
        lib.logo_path()
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    fn publish(&self) {
        let msg = ServerMessage::State(Box::new(self.snapshot()));
        let json = serde_json::to_string(&msg).expect("snapshot serialises");
        self.snapshots.send_replace(Arc::new(json));
    }

    pub fn command(&self, command: &Command) {
        self.show.lock().unwrap().apply(command, now_ms());
        self.publish();
    }

    fn set_service(&self, service: Service) -> std::io::Result<()> {
        std::fs::write(service_path(&self.data_dir), serde_json::to_string_pretty(&service).unwrap())?;
        let lib = self.lib.read().unwrap();
        self.show.lock().unwrap().set_service(service, &lib);
        drop(lib);
        self.publish();
        Ok(())
    }

    fn refresh(&self) {
        let lib = self.lib.read().unwrap();
        self.show.lock().unwrap().refresh(&lib);
        drop(lib);
        self.publish();
    }
}

fn service_path(dir: &std::path::Path) -> PathBuf {
    dir.join("services").join("current.json")
}

/// The default data directory: `LAMPSTAND_DATA`, else the platform's app-data folder.
pub fn default_data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("LAMPSTAND_DATA") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Lampstand")
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or(home).join("Lampstand")
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share")).join("lampstand")
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(|| async { Redirect::to("/operator") }))
        .route("/operator", get(|| page("operator.html")))
        .route("/stage", get(|| page("stage.html")))
        .route("/stream", get(|| page("stream.html")))
        .route("/ws", get(ws))
        .route("/media/deck/{deck}/{slide}/{step}", get(deck_image))
        .route("/media/logo.png", get(logo))
        .route("/api/library", get(library))
        .route("/api/library/songs", get(songs))
        .route("/api/service", get(get_service).put(put_service))
        .route("/api/scripture", get(scripture))
        .route("/api/import/songs", post(import_songs))
        .route("/api/import/bible", post(import_bible))
        .route("/api/import/deck", post(import_deck))
        .route("/api/logo", post(set_logo))
        .route("/api/command", post(http_command))
        .fallback(get(static_file))
        .layer(axum::extract::DefaultBodyLimit::max(512 * 1024 * 1024))
        .with_state(state)
}

/// Serve on `addr` until the process ends.
pub async fn serve(state: Arc<AppState>, listener: tokio::net::TcpListener) -> std::io::Result<()> {
    axum::serve(listener, router(state).into_make_service_with_connect_info::<SocketAddr>()).await
}

async fn page(name: &str) -> Response {
    embedded(name)
}

async fn static_file(uri: axum::http::Uri) -> Response {
    embedded(uri.path().trim_start_matches('/'))
}

fn embedded(path: &str) -> Response {
    if path.contains("..") {
        return StatusCode::NOT_FOUND.into_response();
    }
    match Web::get(path) {
        Some(f) => (
            [(header::CONTENT_TYPE, f.metadata.mimetype().to_string()), (header::CACHE_CONTROL, "no-cache".to_string())],
            f.data.into_owned(),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

// --- WebSocket -----------------------------------------------------------------------------

async fn ws(State(state): State<Arc<AppState>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| client(state, socket, addr))
}

async fn client(state: Arc<AppState>, socket: WebSocket, addr: SocketAddr) {
    let (mut tx, mut rx) = socket.split();
    let mut snapshots = state.snapshots.subscribe();
    let first = snapshots.borrow_and_update().clone();
    if tx.send(Message::Text(first.as_str().into())).await.is_err() {
        return;
    }
    // v0.1: only this machine may drive the show. Stage and stream pages elsewhere on the
    // network are read-only.
    let may_command = addr.ip().is_loopback();
    let mut role = None;
    loop {
        tokio::select! {
            changed = snapshots.changed() => {
                if changed.is_err() { break; }
                let json = snapshots.borrow_and_update().clone();
                if tx.send(Message::Text(json.as_str().into())).await.is_err() { break; }
            }
            msg = rx.next() => {
                let Some(Ok(msg)) = msg else { break };
                let Message::Text(text) = msg else { continue };
                match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(ClientMessage::Hello { role: r, name }) => {
                        tracing::info!(%addr, ?r, name, "client connected");
                        role = Some(r);
                    }
                    Ok(ClientMessage::Command { command }) => {
                        if may_command && role != Some(Role::Stream) {
                            state.command(&command);
                        } else {
                            let e = ServerMessage::Error { message: "this client may not control the show".into() };
                            let _ = tx.send(Message::Text(serde_json::to_string(&e).unwrap().into())).await;
                        }
                    }
                    Err(e) => tracing::warn!(%addr, "bad message: {e}"),
                }
            }
        }
    }
    tracing::info!(%addr, ?role, "client left");
}

async fn http_command(State(state): State<Arc<AppState>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, Json(command): Json<Command>) -> StatusCode {
    if !addr.ip().is_loopback() {
        return StatusCode::FORBIDDEN;
    }
    state.command(&command);
    StatusCode::NO_CONTENT
}

// --- Media --------------------------------------------------------------------------------

#[derive(Deserialize)]
struct Width {
    w: Option<u32>,
}

async fn deck_image(State(state): State<Arc<AppState>>, Path((deck, slide, step)): Path<(String, usize, String)>, Query(q): Query<Width>) -> Response {
    let Some(step) = step.strip_suffix(".png").and_then(|s| s.parse::<usize>().ok()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // Round widths up to a few sizes so every client shares the cache.
    let w = q.w.unwrap_or(1920).clamp(160, 3840);
    let w = [320, 640, 960, 1280, 1920, 2560, 3840].into_iter().find(|s| *s >= w).unwrap_or(3840);
    let key = (deck.clone(), slide, step, w);
    if let Some(png) = state.images.lock().unwrap().get(&key) {
        return png_response(png);
    }
    let d = match state.lib.read().unwrap().deck(&deck).map(|e| e.deck.clone()) {
        Some(Ok(d)) => d,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let rendered = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
        let img = d.render(slide, step, w).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new_with_quality(&mut out, image::codecs::png::CompressionType::Fast, image::codecs::png::FilterType::Adaptive)
            .write_image(&img.pixels, img.width, img.height, image::ExtendedColorType::Rgba8)
            .map_err(|e| e.to_string())?;
        Ok(out)
    })
    .await;
    match rendered {
        Ok(Ok(png)) => {
            let png = Arc::new(png);
            state.images.lock().unwrap().put(key, png.clone());
            png_response(png)
        }
        Ok(Err(e)) => (StatusCode::UNPROCESSABLE_ENTITY, e).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

fn png_response(png: Arc<Vec<u8>>) -> Response {
    ([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "max-age=31536000, immutable")], png.as_ref().clone()).into_response()
}

async fn logo(State(state): State<Arc<AppState>>) -> Response {
    let Some(path) = state.lib.read().unwrap().logo_path() else { return StatusCode::NOT_FOUND.into_response() };
    match std::fs::read(&path) {
        Ok(bytes) => {
            let mime = if path.extension().is_some_and(|e| e == "jpg") { "image/jpeg" } else { "image/png" };
            ([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "no-cache")], bytes).into_response()
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

// --- REST ---------------------------------------------------------------------------------

#[derive(serde::Serialize)]
struct SongRow {
    id: String,
    title: String,
    authors: Vec<String>,
    ccli: Option<u32>,
    first_line: String,
}

#[derive(Deserialize)]
struct SongQuery {
    q: Option<String>,
}

async fn songs(State(state): State<Arc<AppState>>, Query(q): Query<SongQuery>) -> Json<Vec<SongRow>> {
    let lib = state.lib.read().unwrap();
    let ids = lib.search_songs(q.q.as_deref().unwrap_or(""), 50);
    Json(
        ids.into_iter()
            .filter_map(|id| {
                let s = lib.song(&id)?;
                let first_line = s.arranged().ok().and_then(|v| v.first().and_then(|sec| sec.lyrics().into_iter().next())).unwrap_or_default();
                Some(SongRow { id, title: s.title.clone(), authors: s.authors.clone(), ccli: s.ccli_number, first_line })
            })
            .collect(),
    )
}

#[derive(serde::Serialize)]
struct LibraryView {
    songs: usize,
    bibles: Vec<BibleRow>,
    decks: Vec<DeckRow>,
    logo: bool,
    data_dir: String,
}

#[derive(serde::Serialize)]
struct BibleRow {
    abbreviation: String,
    name: String,
    copyright: Option<String>,
}

#[derive(serde::Serialize)]
struct DeckRow {
    id: String,
    name: String,
    slides: Option<usize>,
    problem: Option<String>,
}

async fn library(State(state): State<Arc<AppState>>) -> Json<LibraryView> {
    let lib = state.lib.read().unwrap();
    Json(LibraryView {
        songs: lib.songs().count(),
        bibles: lib
            .bibles()
            .map(|b| BibleRow { abbreviation: b.meta.abbreviation.clone(), name: b.meta.name.clone(), copyright: b.meta.copyright.clone() })
            .collect(),
        decks: lib
            .decks()
            .map(|(id, e)| DeckRow {
                id: id.clone(),
                name: e.name.clone(),
                slides: e.deck.as_ref().ok().map(|d| d.info().slides.iter().filter(|s| !s.hidden).count()),
                problem: e.deck.as_ref().err().cloned(),
            })
            .collect(),
        logo: lib.logo_path().is_some(),
        data_dir: state.data_dir.display().to_string(),
    })
}

async fn get_service(State(state): State<Arc<AppState>>) -> Json<Service> {
    Json(state.show.lock().unwrap().service().clone())
}

async fn put_service(State(state): State<Arc<AppState>>, Json(service): Json<Service>) -> Response {
    match state.set_service(service) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct ScriptureQuery {
    r#ref: String,
    translation: String,
}

/// Check a reference: the canonical form and the text, or why it fails.
async fn scripture(State(state): State<Arc<AppState>>, Query(q): Query<ScriptureQuery>) -> Response {
    let lib = state.lib.read().unwrap();
    let reference: lampstand_scripture::Reference = match q.r#ref.parse() {
        Ok(r) => r,
        Err(e) => return (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    };
    let Some(bible) = lib.bible(&q.translation) else {
        return (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({ "error": format!("no translation called {}", q.translation) }))).into_response();
    };
    match bible.passage(&reference) {
        Ok(verses) => Json(serde_json::json!({
            "reference": reference.to_string(),
            "translation": bible.meta.abbreviation,
            "verses": verses.iter().map(|v| serde_json::json!({ "chapter": v.chapter, "verse": v.verse, "text": v.text })).collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(e) => (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn files(mut multipart: Multipart) -> Result<(Vec<(String, Vec<u8>)>, HashMap<String, String>), String> {
    let mut files = Vec::new();
    let mut fields = HashMap::new();
    while let Some(field) = multipart.next_field().await.map_err(|e| e.to_string())? {
        let name = field.name().unwrap_or_default().to_string();
        match field.file_name().map(str::to_string) {
            Some(file_name) => {
                let bytes = field.bytes().await.map_err(|e| e.to_string())?;
                files.push((file_name, bytes.to_vec()));
            }
            None => {
                let text = field.text().await.map_err(|e| e.to_string())?;
                fields.insert(name, text);
            }
        }
    }
    Ok((files, fields))
}

fn error(e: impl std::fmt::Display) -> Response {
    (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({ "error": e.to_string() }))).into_response()
}

async fn import_songs(State(state): State<Arc<AppState>>, multipart: Multipart) -> Response {
    let (files, _) = match files(multipart).await {
        Ok(f) => f,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };
    let result = state.lib.write().unwrap().import_songs(&files);
    match result {
        Ok(summary) => {
            state.refresh();
            Json(summary).into_response()
        }
        Err(e) => error(e),
    }
}

async fn import_bible(State(state): State<Arc<AppState>>, multipart: Multipart) -> Response {
    let (files, fields) = match files(multipart).await {
        Ok(f) => f,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };
    let mut all = lampstand_show::ImportSummary::default();
    for (name, bytes) in &files {
        let r = state.lib.write().unwrap().import_bible(name, bytes, fields.get("abbreviation").map(String::as_str));
        match r {
            Ok(s) => {
                all.added.extend(s.added);
                all.notes.extend(s.notes);
            }
            Err(e) => all.notes.push(format!("{name}: {e}")),
        }
    }
    state.refresh();
    Json(all).into_response()
}

async fn import_deck(State(state): State<Arc<AppState>>, multipart: Multipart) -> Response {
    let (files, _) = match files(multipart).await {
        Ok(f) => f,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };
    let mut ids = Vec::new();
    for (name, bytes) in files {
        let name2 = name.clone();
        let st = state.clone();
        // Opening a large deck takes a moment; keep it off the async workers.
        let r = tokio::task::spawn_blocking(move || st.lib.write().unwrap().import_deck(&name2, &bytes)).await;
        match r {
            Ok(Ok(id)) => {
                let lib = state.lib.read().unwrap();
                let problem = lib.deck(&id).and_then(|e| e.deck.as_ref().err().cloned());
                ids.push(serde_json::json!({ "id": id, "name": name, "problem": problem }));
            }
            Ok(Err(e)) => return error(e),
            Err(e) => return error(e),
        }
    }
    state.refresh();
    Json(ids).into_response()
}

async fn set_logo(State(state): State<Arc<AppState>>, multipart: Multipart) -> Response {
    let (files, _) = match files(multipart).await {
        Ok(f) => f,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };
    let Some((name, bytes)) = files.into_iter().next() else { return error("no file") };
    let jpeg = name.to_lowercase().ends_with(".jpg") || name.to_lowercase().ends_with(".jpeg");
    if let Err(e) = state.lib.write().unwrap().set_logo(&bytes, jpeg) {
        return error(e);
    }
    state.publish();
    StatusCode::NO_CONTENT.into_response()
}
