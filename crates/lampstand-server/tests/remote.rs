//! Lampstand Remote's routes (T99): pairing by PIN over HTTP, the session
//! cookie, the socket and what it carries, revoking, and a session kept
//! across a restart. What a request may do is the app's gate, tested in the
//! app crate (`app/src-tauri/tests/remote.rs`); here the handler only echoes.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use lampstand_core::id::BlobHash;
use lampstand_core::output::OutputId;
use lampstand_core::scene::Scene;
use lampstand_server::remote::REVOKED;
use lampstand_server::{Backend, RemoteHub, Server};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

struct App {
    remotes: Option<Arc<RemoteHub>>,
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
        self.remotes.clone()
    }
}

struct Fixture {
    hub: Arc<RemoteHub>,
    server: Server,
    _dir: tempfile::TempDir,
}

/// A server whose remote handler answers `{"echo": <request>}`.
async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    fixture_in(dir).await
}

async fn fixture_in(dir: tempfile::TempDir) -> Fixture {
    let hub = Arc::new(RemoteHub::open(dir.path().join("remotes.json")).unwrap());
    hub.set_handler(Arc::new(|request| {
        Box::pin(async move { json!({ "echo": request }) })
    }));
    let server = Server::start(
        SocketAddr::from(([127, 0, 0, 1], 0)),
        Arc::new(App {
            remotes: Some(hub.clone()),
        }),
    )
    .await
    .unwrap();
    Fixture {
        hub,
        server,
        _dir: dir,
    }
}

struct Reply {
    status: u16,
    head: String,
    body: String,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap()
    }

    /// The session token the reply's `Set-Cookie` carries.
    fn cookie(&self) -> Option<String> {
        self.head.lines().find_map(|l| {
            let (name, value) = l.split_once(':')?;
            if !name.eq_ignore_ascii_case("set-cookie") {
                return None;
            }
            Some(value.trim().split(';').next()?.to_string())
        })
    }
}

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

impl Fixture {
    async fn http(&self, method: &str, path: &str, extra: &str, body: &str) -> Reply {
        let mut stream = tokio::net::TcpStream::connect(self.server.local_addr())
            .await
            .unwrap();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n{extra}\
             Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.unwrap();
        let raw = String::from_utf8_lossy(&raw).to_string();
        let (head, body) = raw.split_once("\r\n\r\n").unwrap();
        Reply {
            status: head.split(' ').nth(1).unwrap().parse().unwrap(),
            head: head.to_string(),
            body: body.to_string(),
        }
    }

    async fn pair(&self, pin: &str) -> Reply {
        let body = json!({ "pin": pin, "name": "Sam's phone" }).to_string();
        self.http("POST", "/remote/pair", "", &body).await
    }

    /// Pairs with a fresh PIN and returns the cookie (`name=token`).
    async fn paired(&self) -> String {
        let pin = self.hub.start_pairing();
        let reply = self.pair(&pin.pin).await;
        assert_eq!(reply.status, 200, "{}", reply.body);
        reply.cookie().unwrap()
    }

    async fn ws(&self, cookie: Option<&str>) -> Result<Ws, u16> {
        let url = format!("ws://{}/remote/ws", self.server.local_addr());
        let mut request = url.into_client_request().unwrap();
        if let Some(cookie) = cookie {
            request
                .headers_mut()
                .insert("cookie", HeaderValue::from_str(cookie).unwrap());
        }
        match tokio_tungstenite::connect_async(request).await {
            Ok((ws, _)) => Ok(ws),
            Err(tokio_tungstenite::tungstenite::Error::Http(r)) => Err(r.status().as_u16()),
            Err(e) => panic!("{e}"),
        }
    }
}

/// The next text message that is not a heartbeat.
async fn next(ws: &mut Ws) -> Value {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(10), ws.next())
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

#[tokio::test]
async fn the_right_pin_sets_a_cookie_that_opens_the_socket() {
    let f = fixture().await;
    let pin = f.hub.start_pairing();
    let reply = f.pair(&pin.pin).await;
    assert_eq!(reply.status, 200);
    assert_eq!(reply.json()["name"], "Sam's phone");
    let set = reply
        .head
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("set-cookie"))
        .unwrap()
        .to_string();
    for part in ["HttpOnly", "SameSite=Strict", "Path=/remote"] {
        assert!(set.contains(part), "{set} lacks {part}");
    }
    let cookie = reply.cookie().unwrap();
    let session = f
        .http(
            "GET",
            "/remote/session",
            &format!("Cookie: {cookie}\r\n"),
            "",
        )
        .await;
    assert_eq!(session.status, 200);
    assert_eq!(session.json()["name"], "Sam's phone");

    let mut ws = f.ws(Some(&cookie)).await.unwrap();
    assert_eq!(next(&mut ws).await["hello"]["name"], "Sam's phone");
    assert_eq!(f.hub.sessions()[0].connected, 1);
}

#[tokio::test]
async fn a_wrong_pin_sets_no_cookie_and_the_socket_stays_shut() {
    let f = fixture().await;
    let pin = f.hub.start_pairing();
    let wrong = format!("{:06}", (pin.pin.parse::<u32>().unwrap() + 1) % 1_000_000);
    let reply = f.pair(&wrong).await;
    assert_eq!(reply.status, 403);
    assert!(reply.cookie().is_none());
    assert!(reply.json()["error"]
        .as_str()
        .unwrap()
        .contains("not right"));
    assert_eq!(f.ws(None).await.err(), Some(401));
    assert_eq!(
        f.ws(Some("lampstand_remote=made-up")).await.err(),
        Some(401)
    );
    assert_eq!(f.http("GET", "/remote/session", "", "").await.status, 401);
}

#[tokio::test]
async fn the_fifth_wrong_pin_voids_it_and_an_address_is_rate_limited() {
    let f = fixture().await;
    let pin = f.hub.start_pairing();
    let wrong = format!("{:06}", (pin.pin.parse::<u32>().unwrap() + 1) % 1_000_000);
    for _ in 0..5 {
        assert_eq!(f.pair(&wrong).await.status, 403);
    }
    // The same address has used its tries for this minute, so even a new
    // and right PIN is refused from it.
    let fresh = f.hub.start_pairing();
    assert_eq!(f.pair(&fresh.pin).await.status, 429);
    assert!(f.hub.sessions().is_empty());
}

#[tokio::test]
async fn a_pin_that_ran_out_is_gone() {
    let f = fixture().await;
    let pin = f.hub.start_pairing();
    f.hub.stop_pairing();
    let reply = f.pair(&pin.pin).await;
    assert_eq!(reply.status, 410);
    assert!(reply.cookie().is_none());
}

#[tokio::test]
async fn another_sites_page_cannot_pair_or_open_the_socket() {
    let f = fixture().await;
    let pin = f.hub.start_pairing();
    let body = json!({ "pin": pin.pin }).to_string();
    let reply = f
        .http(
            "POST",
            "/remote/pair",
            "Origin: http://evil.example\r\n",
            &body,
        )
        .await;
    assert_eq!(reply.status, 403);
    let cookie = f.paired().await;
    let url = format!("ws://{}/remote/ws", f.server.local_addr());
    let mut request = url.into_client_request().unwrap();
    let h = request.headers_mut();
    h.insert("cookie", HeaderValue::from_str(&cookie).unwrap());
    h.insert("origin", HeaderValue::from_static("http://evil.example"));
    assert!(tokio_tungstenite::connect_async(request).await.is_err());
}

#[tokio::test]
async fn requests_reach_the_handler_in_order_and_events_reach_every_page() {
    let f = fixture().await;
    f.hub.publish("Live", r#"{"type":"Live","data":1}"#);
    let cookie = f.paired().await;
    let mut ws = f.ws(Some(&cookie)).await.unwrap();
    assert!(next(&mut ws).await.get("hello").is_some());
    // The last event of each kind arrives on connect.
    assert_eq!(next(&mut ws).await["event"]["data"], 1);
    for n in 0..3 {
        let ask = json!({ "id": n, "request": { "area": "Live", "request": "Next" } });
        ws.send(Message::Text(ask.to_string().into()))
            .await
            .unwrap();
    }
    for n in 0..3 {
        let reply = next(&mut ws).await;
        assert_eq!(reply["id"], n);
        assert_eq!(reply["response"]["echo"]["request"], "Next");
    }
    f.hub.publish("Live", r#"{"type":"Live","data":2}"#);
    assert_eq!(next(&mut ws).await["event"]["data"], 2);
}

#[tokio::test]
async fn revoking_a_session_closes_its_socket_at_once() {
    let f = fixture().await;
    let cookie = f.paired().await;
    let kept_cookie = f.paired().await;
    let mut ws = f.ws(Some(&cookie)).await.unwrap();
    let mut kept = f.ws(Some(&kept_cookie)).await.unwrap();
    next(&mut ws).await;
    next(&mut kept).await;
    let token = cookie.split_once('=').unwrap().1;
    let id = f.hub.session(token).unwrap().0;
    assert!(f.hub.revoke(&id));
    let closed = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Close(Some(frame)))) => return Some(u16::from(frame.code)),
                Some(Ok(_)) => continue,
                _ => return None,
            }
        }
    })
    .await
    .expect("closed within 2 s");
    assert_eq!(closed, Some(REVOKED));
    assert_eq!(f.ws(Some(&cookie)).await.err(), Some(401));
    // The other phone carries on.
    f.hub.publish("Live", r#"{"type":"Live","data":3}"#);
    assert_eq!(next(&mut kept).await["event"]["data"], 3);
}

#[tokio::test]
async fn a_session_still_works_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    let f = fixture_in(dir).await;
    let cookie = f.paired().await;
    let Fixture { server, hub, _dir } = f;
    drop(server);
    drop(hub);
    let f = fixture_in(_dir).await;
    assert!(path.join("remotes.json").exists());
    let mut ws = f.ws(Some(&cookie)).await.unwrap();
    assert_eq!(next(&mut ws).await["hello"]["name"], "Sam's phone");
}

#[tokio::test]
async fn without_a_hub_every_remote_route_is_missing() {
    let server = Server::start(
        SocketAddr::from(([127, 0, 0, 1], 0)),
        Arc::new(App { remotes: None }),
    )
    .await
    .unwrap();
    let f = Fixture {
        hub: Arc::new(RemoteHub::open(PathBuf::from("/nonexistent/remotes.json")).unwrap()),
        server,
        _dir: tempfile::tempdir().unwrap(),
    };
    assert_eq!(f.http("GET", "/remote", "", "").await.status, 404);
    assert_eq!(f.pair("123456").await.status, 404);
}

#[tokio::test]
async fn the_page_is_served_and_cannot_be_framed() {
    let f = fixture().await;
    let page = f.http("GET", "/remote", "", "").await;
    assert_eq!(page.status, 200);
    let head = page.head.to_ascii_lowercase();
    assert!(head.contains("x-frame-options: deny"));
    assert!(head.contains("frame-ancestors 'none'"));
    assert_eq!(
        f.http("GET", "/remote/assets/remote.js", "", "")
            .await
            .status,
        200
    );
    assert_eq!(
        f.http("GET", "/remote/assets/../../etc/passwd", "", "")
            .await
            .status,
        404
    );
}
