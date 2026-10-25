//! Lampstand Remote (T99, T100; decision 0006): a helper's phone or tablet
//! drives the live service from its browser, with nothing to install.
//!
//! | Route | What |
//! | --- | --- |
//! | `GET /remote` | the page (`web/remote/dist`, built from `app/src/remote`) |
//! | `GET /remote/assets/*` | its script and styles |
//! | `GET /remote/session` | `{"name"}` for a phone that is paired, else 401 |
//! | `POST /remote/pair` | `{"pin", "name"}`: a right PIN sets the session cookie |
//! | `WS /remote/ws` | the remote socket; needs the session cookie |
//!
//! The session is a random 256-bit token in an `HttpOnly`, `SameSite=Strict`
//! cookie scoped to `/remote`. The socket carries JSON:
//!
//! - from the page, `{"id": n, "request": <Request>}`, answered with
//!   `{"id": n, "response": <Response>}`;
//! - to the page, `{"hello": {"name"}}` once, then `{"event": <Event>}` (the
//!   last of each kind at once, then every change), and `{"beat": ms}` every
//!   few seconds, so a page can tell a quiet link from a dead one.
//!
//! This module never decides what a request may do: it hands every one to
//! the hub's handler, where the app's gate refuses anything that is not
//! driving the service or its sound. Revoking a session closes its sockets
//! with code 4401.

mod hub;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::{broadcast, mpsc, watch};

use crate::Backend;
pub use hub::{
    PairError, RemoteHandler, RemoteHub, RemotePin, RemoteSession, MAX_WRONG, PIN_TTL,
    TRIES_PER_ADDRESS, TRY_WINDOW,
};

/// The close code a revoked session's socket gets.
pub const REVOKED: u16 = 4401;
const COOKIE: &str = "lampstand_remote";
/// Browsers keep a cookie at most 400 days.
const COOKIE_AGE: u32 = 400 * 24 * 60 * 60;
const BEAT: Duration = Duration::from_secs(5);
/// A request is a few hundred bytes; nothing a page sends is near this.
const MAX_MESSAGE: usize = 64 * 1024;

const PAGE: &str = include_str!("../../web/remote/dist/index.html");
const ASSETS: &[(&str, &str, &str)] = &[
    (
        "remote.js",
        "text/javascript; charset=utf-8",
        include_str!("../../web/remote/dist/remote.js"),
    ),
    (
        "remote.css",
        "text/css; charset=utf-8",
        include_str!("../../web/remote/dist/remote.css"),
    ),
];

/// The remote routes, for [`crate::Server`] to merge in.
pub(crate) fn router<S: Clone + Send + Sync + 'static>(backend: Arc<dyn Backend>) -> Router<S> {
    Router::new()
        .route("/remote", get(page))
        .route("/remote/", get(page))
        .route("/remote/assets/{file}", get(asset))
        .route("/remote/session", get(session))
        .route("/remote/pair", post(pair))
        .route("/remote/ws", get(socket))
        .with_state(backend)
}

/// Headers for everything under `/remote`: never framed by another page,
/// never cached, no referrer.
fn guarded(content_type: &'static str, body: impl IntoResponse) -> Response {
    let mut response = body.into_response();
    let h = response.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; img-src 'self' data:; media-src 'self' data:; \
             style-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'",
        ),
    );
    response
}

fn json_response(status: StatusCode, body: serde_json::Value) -> Response {
    (
        status,
        guarded("application/json; charset=utf-8", body.to_string()),
    )
        .into_response()
}

async fn page(State(backend): State<Arc<dyn Backend>>) -> Response {
    if backend.remotes().is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    guarded("text/html; charset=utf-8", PAGE)
}

async fn asset(Path(file): Path<String>) -> Response {
    match ASSETS.iter().find(|(name, _, _)| *name == file) {
        Some((_, content_type, body)) => guarded(content_type, *body),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// The session token in the request's cookie, if there is one.
fn token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|pair| {
            let (name, value) = pair.trim().split_once('=')?;
            (name == COOKIE).then(|| value.to_string())
        })
}

/// A request a page on another site made (a form post, a socket opened by
/// script) carries that site in `Origin`. Only this server's own page may
/// pair or drive. A request with no `Origin` is not from a browser page.
fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok());
    let origin = origin.to_str().ok().and_then(|o| o.strip_prefix("http://"));
    matches!((origin, host), (Some(o), Some(h)) if o.eq_ignore_ascii_case(h))
}

async fn session(State(backend): State<Arc<dyn Backend>>, headers: HeaderMap) -> Response {
    let Some(hub) = backend.remotes() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match token(&headers).and_then(|t| hub.session(&t)) {
        Some((_, name)) => json_response(StatusCode::OK, json!({ "name": name })),
        None => json_response(StatusCode::UNAUTHORIZED, json!({})),
    }
}

#[derive(Deserialize)]
struct PairBody {
    pin: String,
    #[serde(default)]
    name: String,
}

async fn pair(
    State(backend): State<Arc<dyn Backend>>,
    ConnectInfo(from): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(hub) = backend.remotes() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !same_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(body) = serde_json::from_slice::<PairBody>(&body) else {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({ "error": "Type the six digits shown on the laptop." }),
        );
    };
    match hub.pair(from.ip(), &body.pin, &body.name) {
        Ok((token, name)) => {
            eprintln!("lampstand: a remote paired as {name:?} from {}", from.ip());
            let mut response = json_response(StatusCode::OK, json!({ "name": name }));
            let cookie = format!(
                "{COOKIE}={token}; Path=/remote; HttpOnly; SameSite=Strict; Max-Age={COOKIE_AGE}"
            );
            if let Ok(value) = HeaderValue::from_str(&cookie) {
                response.headers_mut().insert(header::SET_COOKIE, value);
            }
            response
        }
        Err(e) => {
            eprintln!("lampstand: a remote at {} did not pair: {e:?}", from.ip());
            let status = match e {
                PairError::TooMany => StatusCode::TOO_MANY_REQUESTS,
                PairError::NoPin => StatusCode::GONE,
                PairError::Wrong { .. } => StatusCode::FORBIDDEN,
            };
            json_response(status, json!({ "error": e.words() }))
        }
    }
}

async fn socket(
    State(backend): State<Arc<dyn Backend>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(hub) = backend.remotes() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !same_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    // Watched from before the session is checked, so a revoke while the
    // socket opens still closes it.
    let revocations = hub.revocations();
    let Some((id, name)) = token(&headers).and_then(|t| hub.session(&t)) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    ws.max_message_size(MAX_MESSAGE)
        .on_upgrade(move |socket| serve(socket, hub, revocations, id, name))
}

/// Counts a page as connected while it lives.
struct Open {
    hub: Arc<RemoteHub>,
    id: String,
}

impl Drop for Open {
    fn drop(&mut self) {
        self.hub.connection(&self.id, false);
    }
}

#[derive(Deserialize)]
struct Incoming {
    id: serde_json::Value,
    request: serde_json::Value,
}

fn refused(message: &str) -> serde_json::Value {
    json!({ "type": "Error", "data": { "message": message } })
}

async fn serve(
    socket: WebSocket,
    hub: Arc<RemoteHub>,
    mut revocations: watch::Receiver<u64>,
    id: String,
    name: String,
) {
    hub.connection(&id, true);
    let _open = Open {
        hub: hub.clone(),
        id: id.clone(),
    };
    let (mut tx, mut rx) = socket.split();
    // Requests are answered one at a time, in order (Next then Back must not
    // swap), while events keep flowing.
    let (asks, mut asked) = mpsc::channel::<Incoming>(16);
    let (answers, mut answered) = mpsc::channel::<String>(16);
    let handler = hub.handler();
    let worker = tokio::spawn(async move {
        while let Some(Incoming { id, request }) = asked.recv().await {
            let response = match &handler {
                Some(handle) => handle(request).await,
                None => refused("The remote is not ready yet."),
            };
            let reply = json!({ "id": id, "response": response }).to_string();
            if answers.send(reply).await.is_err() {
                return;
            }
        }
    });
    // Subscribed before the last events are sent, so none falls between.
    let mut feed = hub.subscribe();
    let mut beat = tokio::time::interval(BEAT);
    let hello = json!({ "hello": { "name": name } }).to_string();
    let mut first = vec![Message::Text(hello.into())];
    first.extend(
        hub.latest()
            .iter()
            .map(|m| Message::Text(m.as_ref().into())),
    );
    for message in first {
        if tx.send(message).await.is_err() {
            worker.abort();
            return;
        }
    }
    loop {
        let out = tokio::select! {
            event = feed.recv() => match event {
                Ok(m) => vec![m],
                // Too slow to keep up: start again from what is true now.
                Err(broadcast::error::RecvError::Lagged(_)) => hub.latest(),
                Err(broadcast::error::RecvError::Closed) => break,
            }
            .into_iter()
            .map(|m| Message::Text(m.as_ref().into()))
            .collect(),
            Some(reply) = answered.recv() => vec![Message::Text(reply.into())],
            incoming = rx.next() => match incoming {
                Some(Ok(Message::Text(text))) => match serde_json::from_str::<Incoming>(&text) {
                    Ok(ask) => {
                        let rid = ask.id.clone();
                        match asks.try_send(ask) {
                            Ok(()) => Vec::new(),
                            Err(_) => vec![Message::Text(json!({
                                "id": rid,
                                "response": refused("Too many presses at once. Try again."),
                            }).to_string().into())],
                        }
                    }
                    Err(_) => {
                        eprintln!("lampstand: a remote sent something unreadable");
                        Vec::new()
                    }
                },
                None | Some(Err(_) | Ok(Message::Close(_))) => break,
                Some(Ok(_)) => Vec::new(),
            },
            changed = revocations.changed() => {
                if changed.is_err() || !hub.alive(&id) {
                    eprintln!("lampstand: a revoked remote was disconnected");
                    let _ = tx
                        .send(Message::Close(Some(CloseFrame {
                            code: REVOKED,
                            reason: "revoked".into(),
                        })))
                        .await;
                    break;
                }
                Vec::new()
            }
            _ = beat.tick() => vec![Message::Text(
                json!({ "beat": hub_now_ms() }).to_string().into(),
            )],
        };
        for message in out {
            if tx.send(message).await.is_err() {
                worker.abort();
                return;
            }
        }
    }
    worker.abort();
}

fn hub_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(header::HeaderName, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.append(k.clone(), HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn the_token_is_read_from_the_cookie() {
        let h = headers(&[(header::COOKIE, "a=1; lampstand_remote=abc ; b=2")]);
        assert_eq!(token(&h).as_deref(), Some("abc"));
        assert_eq!(token(&headers(&[(header::COOKIE, "a=1")])), None);
        assert_eq!(token(&HeaderMap::new()), None);
    }

    #[test]
    fn only_this_servers_own_page_passes_the_origin_check() {
        let host = (header::HOST, "192.168.1.20:7340");
        assert!(same_origin(&headers(std::slice::from_ref(&host))));
        assert!(same_origin(&headers(&[
            host.clone(),
            (header::ORIGIN, "http://192.168.1.20:7340")
        ])));
        assert!(!same_origin(&headers(&[
            host.clone(),
            (header::ORIGIN, "http://evil.example")
        ])));
        assert!(!same_origin(&headers(&[
            host,
            (header::ORIGIN, "https://192.168.1.20:7340")
        ])));
        assert!(!same_origin(&headers(&[(header::ORIGIN, "http://x")])));
    }
}
