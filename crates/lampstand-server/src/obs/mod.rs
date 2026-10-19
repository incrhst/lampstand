//! The OBS browser source (T43, PRD OB-1, OB-2): the routes, the page, and
//! the scene feed behind them.
//!
//! OBS loads `GET /obs/<output>?k=<key>`. The page (`web/obs/`, TypeScript
//! compiled to `web/obs/dist`) opens `WS /obs/<output>/ws?k=<key>` and draws
//! each message in the DOM with a transparent background. A message is
//! JSON, one of
//!
//! - `{"seq": n, "scene": <Scene>}`, drawn by the page;
//! - `{"seq": n, "width": w, "height": h, "frame": n}`, for a deck slide:
//!   the page shows `GET /frames/<output>/<n>.webp?k=<key>`, the scene
//!   rasterised by `render_offscreen` (needs `Backend::renderer`; without it
//!   the deck is sent as a scene and drawn in the DOM).
//!
//! The page's code and the bundled fonts it draws text with
//! (`/obs/assets/fonts.css` and the files it names, the same ones layout and
//! the renderer shape with) are public; scenes, frames and blobs need the
//! output's key. A wrong key or unknown output is a plain 404.

mod feed;
mod hub;

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use lampstand_core::id::BlobHash;
use lampstand_core::output::OutputId;
use lampstand_core::scene::Scene;
use serde::Serialize;

use crate::Backend;
use feed::{is_deck, Feed, Feeds};
pub use hub::SceneHub;

const INDEX_HTML: &str = include_str!("../../web/obs/index.html");
const SCRIPTS: &[(&str, &str)] = &[
    ("main.js", include_str!("../../web/obs/dist/main.js")),
    ("render.js", include_str!("../../web/obs/dist/render.js")),
    ("paint.js", include_str!("../../web/obs/dist/paint.js")),
    ("fonts.js", include_str!("../../web/obs/dist/fonts.js")),
    ("scene.js", include_str!("../../web/obs/dist/scene.js")),
];

/// File prefix in `assets/fonts` and the family name the scene uses.
const FAMILIES: &[(&str, &str)] = &[
    ("NotoSans", "Noto Sans"),
    ("NotoSerif", "Noto Serif"),
    ("Carlito", "Carlito"),
    ("Caladea", "Caladea"),
    ("Arimo", "Arimo"),
    ("Tinos", "Tinos"),
    ("Cousine", "Cousine"),
];

#[derive(Clone)]
struct Obs {
    backend: Arc<dyn Backend>,
    feeds: Arc<Feeds>,
}

/// The browser-source routes, for [`crate::Server`] to merge in.
pub(crate) fn router<S: Clone + Send + Sync + 'static>(backend: Arc<dyn Backend>) -> Router<S> {
    Router::new()
        .route("/obs/assets/fonts.css", get(fonts_css))
        .route("/obs/assets/fonts/{file}", get(font_file))
        .route("/obs/assets/{file}", get(script))
        .route("/obs/{output}", get(page))
        .route("/obs/{output}/ws", get(feed_ws))
        .route("/blobs/{hash}", get(blob))
        .route("/frames/{output}/{file}", get(frame))
        .with_state(Obs {
            backend,
            feeds: Arc::default(),
        })
}

fn query(uri: &Uri, name: &str) -> Option<String> {
    uri.query()?.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == name).then(|| v.to_string())
    })
}

/// The output named in the path, if the request carries its key.
fn authorize(obs: &Obs, output: &str, uri: &Uri) -> Option<OutputId> {
    let output: OutputId = output.parse().ok()?;
    let key = query(uri, "k")?;
    obs.backend.output_key_valid(output, &key).then_some(output)
}

fn not_found() -> Response {
    StatusCode::NOT_FOUND.into_response()
}

fn with_type(content_type: &'static str, cache: &'static str, body: impl IntoResponse) -> Response {
    let mut response = body.into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    response
}

async fn page(State(obs): State<Obs>, Path(output): Path<String>, uri: Uri) -> Response {
    if authorize(&obs, &output, &uri).is_none() {
        return not_found();
    }
    let mut response = with_type("text/html; charset=utf-8", "no-store", INDEX_HTML);
    // The key is in the URL, so keep it out of anything the page loads.
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

async fn script(Path(file): Path<String>) -> Response {
    match SCRIPTS.iter().find(|(name, _)| *name == file) {
        Some((_, code)) => with_type("text/javascript; charset=utf-8", "no-cache", *code),
        None => not_found(),
    }
}

/// `@font-face` for every bundled font file.
async fn fonts_css() -> Response {
    let mut css = String::new();
    for (file, _) in lampstand_layout::bundled_font_files() {
        let Some((prefix, style)) = file.trim_end_matches(".ttf").split_once('-') else {
            continue;
        };
        let Some((_, family)) = FAMILIES.iter().find(|(p, _)| *p == prefix) else {
            continue;
        };
        let weight = if style.starts_with("Bold") { 700 } else { 400 };
        let slant = if style.ends_with("Italic") {
            "italic"
        } else {
            "normal"
        };
        css.push_str(&format!(
            "@font-face{{font-family:\"{family}\";font-weight:{weight};font-style:{slant};\
             src:url(\"/obs/assets/fonts/{file}\") format(\"truetype\");font-display:block}}\n"
        ));
    }
    with_type("text/css; charset=utf-8", "public, max-age=86400", css)
}

async fn font_file(Path(file): Path<String>) -> Response {
    let found = lampstand_layout::bundled_font_files()
        .iter()
        .find(|(name, _)| *name == file);
    match found {
        Some((_, bytes)) => with_type("font/ttf", "public, max-age=31536000, immutable", *bytes),
        None => not_found(),
    }
}

/// `GET /blobs/<hash>?o=<output>&k=<key>`: images and videos a scene names.
async fn blob(State(obs): State<Obs>, Path(hash): Path<String>, uri: Uri) -> Response {
    let Some(output) = query(&uri, "o") else {
        return not_found();
    };
    if authorize(&obs, &output, &uri).is_none() {
        return not_found();
    }
    let Ok(hash) = hash.parse::<BlobHash>() else {
        return not_found();
    };
    let Some(path) = obs.backend.blob_path(&hash) else {
        return not_found();
    };
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return not_found();
    };
    // Blobs are named by their hash, so they never change.
    let content_type = match image::guess_format(&bytes) {
        Ok(format) => format.to_mime_type(),
        Err(_) => "application/octet-stream",
    };
    with_type(content_type, "private, max-age=31536000, immutable", bytes)
}

/// `GET /frames/<output>/<n>.webp?k=<key>`.
async fn frame(
    State(obs): State<Obs>,
    Path((output, file)): Path<(String, String)>,
    uri: Uri,
) -> Response {
    let Some(output) = authorize(&obs, &output, &uri) else {
        return not_found();
    };
    let Some(seq) = file
        .strip_suffix(".webp")
        .and_then(|n| n.parse::<u64>().ok())
    else {
        return not_found();
    };
    let (Some(feed), Some(renderer)) = (obs.feeds.get(output), obs.backend.renderer()) else {
        return not_found();
    };
    match feed.frame(renderer, seq).await {
        Some(bytes) => with_type("image/webp", "private, max-age=3600", bytes.to_vec()),
        None => not_found(),
    }
}

/// `WS /obs/<output>/ws?k=<key>`.
async fn feed_ws(
    State(obs): State<Obs>,
    Path(output): Path<String>,
    uri: Uri,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(output) = authorize(&obs, &output, &uri) else {
        return not_found();
    };
    let Some(feed) = obs.feeds.subscribe(obs.backend.as_ref(), output) else {
        return not_found();
    };
    let frames = obs.backend.renderer().is_some();
    ws.on_upgrade(move |socket| follow(socket, feed, frames))
}

#[derive(Serialize)]
#[serde(untagged)]
enum Update<'a> {
    Scene {
        seq: u64,
        scene: &'a Scene,
    },
    Frame {
        seq: u64,
        width: f32,
        height: f32,
        frame: u64,
    },
}

/// Sends the current scene and then every change until either end leaves.
async fn follow(mut socket: WebSocket, feed: Arc<Feed>, frames: bool) {
    let mut changes = feed.watch();
    loop {
        let current = changes.borrow_and_update().clone();
        if let Some(scene) = &current.scene {
            let update = if frames && is_deck(scene) {
                Update::Frame {
                    seq: current.seq,
                    width: scene.width,
                    height: scene.height,
                    frame: current.seq,
                }
            } else {
                Update::Scene {
                    seq: current.seq,
                    scene,
                }
            };
            let Ok(json) = serde_json::to_string(&update) else {
                return;
            };
            if socket.send(Message::Text(json.into())).await.is_err() {
                return;
            }
        }
        tokio::select! {
            changed = changes.changed() => if changed.is_err() { return },
            incoming = socket.recv() => match incoming {
                None | Some(Err(_) | Ok(Message::Close(_))) => return,
                Some(Ok(_)) => {}
            },
        }
    }
}
