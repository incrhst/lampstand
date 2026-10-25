//! Lampstand Remote through the gate (T99): a paired phone's socket on the
//! real local server, with the app's `remote::answer` in front of a
//! stand-in for the app. What the gate refuses is answered with an error and
//! never reaches the app; what it lets through is carried out. Also times a
//! command to the `Event::Live` it causes, over the socket.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use lampstand_api::{Event, Request, Response};
use lampstand_core::id::BlobHash;
use lampstand_core::output::OutputId;
use lampstand_core::program::Overlay;
use lampstand_core::scene::Scene;
use lampstand_live::Cursor;
use lampstand_server::{Backend, RemoteHub, Server};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

struct App {
    hub: Arc<RemoteHub>,
}

impl Backend for App {
    fn output_key_valid(&self, _: OutputId, _: &str) -> bool {
        false
    }
    fn watch_scene(&self, _: OutputId) -> Option<Receiver<Arc<Scene>>> {
        None
    }
    fn blob_path(&self, _: &BlobHash) -> Option<PathBuf> {
        None
    }
    fn remotes(&self) -> Option<Arc<RemoteHub>> {
        Some(self.hub.clone())
    }
}

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct Fixture {
    hub: Arc<RemoteHub>,
    /// What reached the stand-in app.
    reached: Arc<Mutex<Vec<Request>>>,
    ws: Ws,
    next_id: u64,
    _server: Server,
    _dir: tempfile::TempDir,
}

/// A server whose remote requests go through `remote::answer` to a stand-in
/// app that records them and, for a live command, says the slide moved.
async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let hub = Arc::new(lampstand_app::remote::open_hub(dir.path()).unwrap());
    let reached: Arc<Mutex<Vec<Request>>> = Arc::default();
    let (seen, events) = (reached.clone(), Arc::downgrade(&hub));
    hub.set_handler(Arc::new(move |request| {
        let (seen, events) = (seen.clone(), events.clone());
        Box::pin(async move {
            lampstand_app::remote::answer(request, |request| async move {
                if matches!(request, Request::Live(_)) {
                    if let Some(hub) = events.upgrade() {
                        lampstand_app::remote::bridge(
                            &hub,
                            &Event::Live {
                                live: Some(Cursor {
                                    item: lampstand_core::service::ItemId::generate(),
                                    slide: 1,
                                    step: 0,
                                }),
                                shown: None,
                                preview: None,
                                idle: false,
                                overlay: Overlay::None,
                            },
                        );
                    }
                }
                seen.lock().unwrap().push(request);
                Response::Ok
            })
            .await
        })
    }));
    let server = Server::start(
        SocketAddr::from(([127, 0, 0, 1], 0)),
        Arc::new(App { hub: hub.clone() }),
    )
    .await
    .unwrap();
    let pin = hub.start_pairing();
    let (token, _) = hub
        .pair("127.0.0.1".parse().unwrap(), &pin.pin, "Test phone")
        .unwrap();
    let url = format!("ws://{}/remote/ws", server.local_addr());
    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert(
        "cookie",
        HeaderValue::from_str(&format!("lampstand_remote={token}")).unwrap(),
    );
    let (ws, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let mut f = Fixture {
        hub,
        reached,
        ws,
        next_id: 0,
        _server: server,
        _dir: dir,
    };
    assert!(f.next().await.get("hello").is_some());
    f
}

impl Fixture {
    async fn next(&mut self) -> Value {
        loop {
            let message = tokio::time::timeout(Duration::from_secs(10), self.ws.next())
                .await
                .expect("a message within 10 s")
                .expect("open")
                .expect("no error");
            if let Message::Text(text) = message {
                let value: Value = serde_json::from_str(&text).unwrap();
                if value.get("beat").is_none() {
                    return value;
                }
            }
        }
    }

    async fn send(&mut self, request: Value) {
        self.next_id += 1;
        let ask = json!({ "id": self.next_id, "request": request });
        self.ws
            .send(Message::Text(ask.to_string().into()))
            .await
            .unwrap();
    }

    /// Sends `request` and returns the response to it, skipping events.
    async fn ask(&mut self, request: Value) -> Value {
        self.send(request).await;
        loop {
            let reply = self.next().await;
            if reply["id"] == self.next_id {
                return reply["response"].clone();
            }
        }
    }
}

#[tokio::test]
async fn refused_requests_never_reach_the_app() {
    let mut f = fixture().await;
    for refused in [
        json!({ "area": "Library", "request": "ListThemes" }),
        json!({ "area": "Settings", "request": "Get" }),
        json!({ "area": "Service", "request": "List" }),
        json!({ "area": "Team", "request": "StartPairing" }),
        json!({ "area": "Remote", "request": "StartPairing" }),
        json!({ "area": "Sound", "request": "Outputs" }),
        json!({ "area": "Live", "request": "EndService" }),
        json!({ "area": "Outputs", "request": "List" }),
        json!({ "nonsense": true }),
    ] {
        let response = f.ask(refused.clone()).await;
        assert_eq!(response["type"], "Error", "{refused} was not refused");
    }
    assert!(f.reached.lock().unwrap().is_empty());
    assert!(f.hub.pairing().is_none(), "a phone cannot start pairing");
}

#[tokio::test]
async fn live_and_sound_commands_reach_the_app() {
    let mut f = fixture().await;
    for allowed in [
        json!({ "area": "Live", "request": "Next" }),
        json!({ "area": "Live", "request": "ToggleBlack" }),
        json!({ "area": "Sound", "request": "Pause" }),
        json!({ "area": "Sound", "request": "Panic" }),
    ] {
        let response = f.ask(allowed.clone()).await;
        assert_eq!(response["type"], "Ok", "{allowed}");
    }
    assert_eq!(f.reached.lock().unwrap().len(), 4);
}

/// The time from sending Next to the `Event::Live` it causes, over the
/// socket on this machine, with the app stood in for. ARCHITECTURE.md has
/// the figure measured against the real app on Wi-Fi.
#[tokio::test]
async fn a_command_is_answered_with_live_state_quickly() {
    let mut f = fixture().await;
    let mut times = Vec::new();
    for _ in 0..20 {
        let start = Instant::now();
        f.send(json!({ "area": "Live", "request": "Next" })).await;
        loop {
            let message = f.next().await;
            if message["event"]["type"] == "Live" {
                break;
            }
        }
        times.push(start.elapsed());
    }
    times.sort();
    let median = times[times.len() / 2];
    eprintln!("command to Event::Live over the socket: median {median:?}");
    assert!(median < Duration::from_millis(150), "{median:?}");
}
