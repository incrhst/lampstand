//! End to end: a real server on a free port, driven over HTTP and WebSocket.

use std::net::SocketAddr;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

const AMAZING_GRACE: &str = include_str!("../../lampstand-songs/tests/fixtures/amazing-grace.xml");

async fn start() -> (tempfile::TempDir, SocketAddr) {
    let dir = tempfile::tempdir().unwrap();
    let (state, warnings) = lampstand_server::AppState::open(dir.path().to_path_buf()).unwrap();
    assert!(warnings.is_empty());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(lampstand_server::serve(state, listener));
    (dir, addr)
}

async fn next_state(ws: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin)) -> Value {
    loop {
        let msg = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next()).await.expect("a state in time").unwrap().unwrap();
        if let Message::Text(t) = msg {
            let v: Value = serde_json::from_str(&t).unwrap();
            if v["type"] == "state" {
                return v;
            }
        }
    }
}

#[tokio::test]
async fn import_build_a_service_and_drive_it() {
    let (dir, addr) = start().await;
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();

    // Pages are served.
    let page = http.get(format!("{base}/operator")).send().await.unwrap();
    assert!(page.status().is_success());
    assert!(page.text().await.unwrap().contains("Lampstand"));

    // Import a song, find it by a misspelt search.
    let form = reqwest::multipart::Form::new()
        .part("file", reqwest::multipart::Part::bytes(AMAZING_GRACE.as_bytes().to_vec()).file_name("amazing-grace.xml"));
    let r: Value = http.post(format!("{base}/api/import/songs")).multipart(form).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["added"][0], "Amazing Grace");
    let hits: Value = http.get(format!("{base}/api/library/songs?q=amazng")).send().await.unwrap().json().await.unwrap();
    let song = hits[0]["id"].as_str().unwrap().to_string();

    // A client connects and gets the empty state at once.
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws")).await.unwrap();
    ws.send(Message::Text(json!({"type": "hello", "role": "output", "name": "test"}).to_string().into())).await.unwrap();
    let s = next_state(&mut ws).await;
    assert_eq!(s["items"].as_array().unwrap().len(), 0);

    // Build the order; every client hears about it.
    let service = json!({"name": "Sunday", "items": [{"id": "x", "kind": "song", "song": song}, {"id": "y", "kind": "logo"}]});
    let put = http.put(format!("{base}/api/service")).json(&service).send().await.unwrap();
    assert!(put.status().is_success());
    let s = next_state(&mut ws).await;
    assert_eq!(s["service_name"], "Sunday");
    assert_eq!(s["items"][0]["title"], "Amazing Grace");
    assert!(s["live"].is_null());

    // Drive it from the client, as a clicker on the output window would.
    ws.send(Message::Text(json!({"type": "command", "command": {"kind": "next"}}).to_string().into())).await.unwrap();
    let s = next_state(&mut ws).await;
    assert_eq!(s["live"], json!({"item": 0, "cue": 0}));
    assert_eq!(s["live_cue"]["kind"], "text");
    assert!(s["live_cue"]["lines"][0].as_str().unwrap().starts_with("Amazing grace"));
    ws.send(Message::Text(json!({"type": "command", "command": {"kind": "mode", "mode": "black"}}).to_string().into())).await.unwrap();
    assert_eq!(next_state(&mut ws).await["mode"], "black");

    // The order survives a restart.
    let saved = std::fs::read_to_string(dir.path().join("services/current.json")).unwrap();
    assert!(saved.contains("Sunday"));
}

#[tokio::test]
async fn bad_input_is_reported_not_fatal() {
    let (_dir, addr) = start().await;
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();
    let r = http.get(format!("{base}/api/scripture?ref=Hezekiah%203:16&translation=KJV")).send().await.unwrap();
    assert_eq!(r.status(), 422);
    let form = reqwest::multipart::Form::new().part("file", reqwest::multipart::Part::bytes(b"not a deck".to_vec()).file_name("broken.pptx"));
    let r: Value = http.post(format!("{base}/api/import/deck")).multipart(form).send().await.unwrap().json().await.unwrap();
    assert!(r[0]["problem"].is_string(), "{r}");
    let img = http.get(format!("{base}/media/deck/nope/0/0.png")).send().await.unwrap();
    assert_eq!(img.status(), 404);
}
