//! OBS control (T89) against a mock obs-websocket server: the view goes
//! through every connection state, says why OBS isn't connected, follows
//! streaming, and keeps the password to itself.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use lampstand_api::{Event, ObsConnection, ObsProblem, ObsRequest, ObsView, Response};
use lampstand_app::obs_control::{ObsControl, Os, Probe};
use lampstand_connectors::obs::auth_response;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;

const SALT: &str = "c2FsdA==";
const CHALLENGE: &str = "Y2hhbGxlbmdl";

/// A pretend OBS on a free port. `streaming` decides what
/// `GetStreamStatus` says; `stop` closes the listener and every connection.
struct MockObs {
    port: u16,
    streaming: Arc<AtomicBool>,
    events: mpsc::UnboundedSender<Value>,
    stop: watch::Sender<bool>,
}

impl MockObs {
    async fn start(password: Option<&'static str>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let streaming = Arc::new(AtomicBool::new(false));
        let (events, events_rx) = mpsc::unbounded_channel::<Value>();
        let events_rx = Arc::new(tokio::sync::Mutex::new(events_rx));
        let (stop, stopped) = watch::channel(false);
        let mock = Self {
            port,
            streaming: streaming.clone(),
            events,
            stop,
        };
        tokio::spawn(async move {
            let mut stopped_accept = stopped.clone();
            loop {
                let tcp = tokio::select! {
                    accepted = listener.accept() => accepted.unwrap().0,
                    _ = stopped_accept.changed() => return,
                };
                let ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
                tokio::spawn(serve(
                    ws,
                    password,
                    streaming.clone(),
                    events_rx.clone(),
                    stopped.clone(),
                ));
            }
        });
        mock
    }

    fn stop(&self) {
        self.stop.send_replace(true);
    }
}

type ServerWs = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

async fn serve(
    mut ws: ServerWs,
    password: Option<&'static str>,
    streaming: Arc<AtomicBool>,
    events: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<Value>>>,
    mut stopped: watch::Receiver<bool>,
) {
    let mut hello = json!({ "obsWebSocketVersion": "5.5.2", "rpcVersion": 1 });
    if password.is_some() {
        hello["authentication"] = json!({ "challenge": CHALLENGE, "salt": SALT });
    }
    send(&mut ws, 0, hello).await;
    let Some(identify) = recv(&mut ws).await else {
        return;
    };
    if let Some(password) = password {
        if identify["d"]["authentication"].as_str()
            != Some(&auth_response(password, SALT, CHALLENGE))
        {
            let _ = ws
                .close(Some(CloseFrame {
                    code: CloseCode::Library(4009),
                    reason: "Authentication failed.".into(),
                }))
                .await;
            return;
        }
    }
    send(&mut ws, 2, json!({ "negotiatedRpcVersion": 1 })).await;
    let mut events = events.lock().await;
    loop {
        tokio::select! {
            _ = stopped.changed() => return,
            Some(event) = events.recv() => send(&mut ws, 5, event).await,
            message = recv(&mut ws) => {
                let Some(message) = message else { return };
                let d = &message["d"];
                let request_type = d["requestType"].as_str().unwrap_or_default();
                let on = streaming.load(Ordering::SeqCst);
                let data = match request_type {
                    "GetVersion" => json!({ "obsVersion": "30.2.3" }),
                    "GetStreamStatus" => json!({
                        "outputActive": on, "outputReconnecting": false,
                        "outputTimecode": if on { "00:12:34.567" } else { "00:00:00.000" },
                        "outputSkippedFrames": if on { 3 } else { 0 },
                        "outputTotalFrames": if on { 45000 } else { 0 },
                    }),
                    "GetRecordStatus" => json!({
                        "outputActive": false, "outputPaused": false, "outputTimecode": "00:00:00.000",
                    }),
                    _ => Value::Null,
                };
                send(&mut ws, 7, json!({
                    "requestType": request_type, "requestId": d["requestId"],
                    "requestStatus": { "result": true, "code": 100 },
                    "responseData": data,
                })).await;
            }
        }
    }
}

async fn send(ws: &mut ServerWs, op: u8, d: Value) {
    let _ = ws
        .send(Message::text(json!({ "op": op, "d": d }).to_string()))
        .await;
}

async fn recv(ws: &mut ServerWs) -> Option<Value> {
    loop {
        match ws.next().await? {
            Ok(Message::Text(t)) => return serde_json::from_str(&t).ok(),
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => {}
        }
    }
}

/// OBS installed (as a flatpak) and, per `running`, running or not.
struct Machine {
    running: AtomicBool,
    launched: Mutex<Vec<String>>,
}

impl Probe for Machine {
    fn exists(&self, path: &Path) -> bool {
        path.ends_with("com.obsproject.Studio")
    }
    fn env(&self, _: &str) -> Option<OsString> {
        None
    }
    fn running(&self, _: Os, _: &str) -> Option<bool> {
        Some(self.running.load(Ordering::SeqCst))
    }
    fn spawn(&self, program: &Path, args: &[&str], _: Option<&Path>) -> io::Result<()> {
        self.launched
            .lock()
            .unwrap()
            .push(format!("{} {}", program.display(), args.join(" ")));
        Ok(())
    }
}

struct Harness {
    control: Arc<ObsControl>,
    machine: Arc<Machine>,
    streaming_output: Arc<AtomicBool>,
    pushed: Arc<Mutex<Vec<ObsView>>>,
    _dir: tempfile::TempDir,
    dir: PathBuf,
}

/// A controller with OBS settings saved for `port`, as Setup would.
async fn harness(port: u16, password: Option<&str>) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    let mut settings = json!({ "host": "127.0.0.1", "port": port });
    if let Some(p) = password {
        settings["password"] = p.into();
    }
    std::fs::write(path.join("obs.json"), settings.to_string()).unwrap();
    start(dir, path).await
}

async fn start(dir: tempfile::TempDir, path: PathBuf) -> Harness {
    let machine = Arc::new(Machine {
        running: AtomicBool::new(false),
        launched: Mutex::new(Vec::new()),
    });
    let streaming_output = Arc::new(AtomicBool::new(false));
    let pushed = Arc::new(Mutex::new(Vec::new()));
    let sink = pushed.clone();
    let control = ObsControl::start(
        &path,
        streaming_output.clone(),
        machine.clone(),
        Os::Linux,
        Arc::new(move |event| {
            if let Event::Obs(view) = event {
                sink.lock().unwrap().push(view);
            }
        }),
    );
    Harness {
        control,
        machine,
        streaming_output,
        pushed,
        _dir: dir,
        dir: path,
    }
}

impl Harness {
    async fn status(&self) -> ObsView {
        match self.control.handle(ObsRequest::Status).await {
            Response::Obs(view) => view,
            other => panic!("expected a view, got {other:?}"),
        }
    }

    /// Waits until a pushed view (or the current one) matches.
    async fn wait_for(&self, what: impl Fn(&ObsView) -> bool) -> ObsView {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let view = self.status().await;
                if what(&view) {
                    return view;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("the view never got there")
    }
}

fn connected(v: &ObsView) -> bool {
    matches!(v.connection, ObsConnection::Connected { .. })
}

#[tokio::test(flavor = "multi_thread")]
async fn connects_follows_streaming_and_reports_disconnects() {
    let mock = MockObs::start(None).await;
    let h = harness(mock.port, None).await;

    let view = h.wait_for(connected).await;
    assert_eq!(
        view.connection,
        ObsConnection::Connected {
            obs_version: "30.2.3".into(),
            websocket_version: "5.5.2".into(),
        }
    );
    assert_eq!(view.problem, None);
    assert!(view.relevant, "saved settings make OBS relevant");
    let view = h.wait_for(|v| v.stream.is_some()).await;
    assert!(!view.stream.unwrap().active);

    // OBS starts streaming: its event triggers a refresh with the timecode.
    mock.streaming.store(true, Ordering::SeqCst);
    mock.events
        .send(json!({
            "eventType": "StreamStateChanged", "eventIntent": 64,
            "eventData": { "outputActive": true, "outputState": "OBS_WEBSOCKET_OUTPUT_STARTED" },
        }))
        .unwrap();
    let view = h
        .wait_for(|v| v.stream.as_ref().is_some_and(|s| s.active))
        .await;
    let stream = view.stream.unwrap();
    assert_eq!(stream.timecode, "00:12:34");
    assert_eq!((stream.skipped_frames, stream.total_frames), (3, 45000));
    assert!(
        h.pushed
            .lock()
            .unwrap()
            .iter()
            .any(|v| v.stream.as_ref().is_some_and(|s| s.active)),
        "the change was pushed as Event::Obs"
    );

    // OBS quits: not running, and not shown as connecting between retries.
    mock.stop();
    let view = h
        .wait_for(|v| matches!(v.connection, ObsConnection::Disconnected { .. }))
        .await;
    assert_eq!(view.stream, None);
    let view = h.wait_for(|v| v.problem.is_some()).await;
    assert_eq!(view.problem, Some(ObsProblem::NotRunning));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
        matches!(
            h.status().await.connection,
            ObsConnection::Disconnected { .. }
        ),
        "a retry keeps showing how the last attempt ended"
    );

    // Running, but nothing answers: its WebSocket server is off.
    h.machine.running.store(true, Ordering::SeqCst);
    let view = h
        .wait_for(|v| v.problem == Some(ObsProblem::ServerOff))
        .await;
    assert!(matches!(
        view.connection,
        ObsConnection::Disconnected { .. }
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn passwords_are_checked_and_never_shown() {
    let mock = MockObs::start(Some("right")).await;
    let h = harness(mock.port, None).await;
    let view = h
        .wait_for(|v| v.connection == ObsConnection::PasswordRequired)
        .await;
    assert_eq!(view.problem, Some(ObsProblem::PasswordRequired));
    assert!(!view.has_password);

    // Save and test with the wrong one: answers once OBS has refused it.
    let Response::Obs(view) = h
        .control
        .handle(ObsRequest::Configure {
            host: "127.0.0.1".into(),
            port: mock.port,
            password: Some("wrong".into()),
        })
        .await
    else {
        panic!("configure failed")
    };
    assert_eq!(view.connection, ObsConnection::WrongPassword);
    assert_eq!(view.problem, Some(ObsProblem::WrongPassword));
    assert!(view.has_password);

    let Response::Obs(view) = h
        .control
        .handle(ObsRequest::Configure {
            host: "127.0.0.1".into(),
            port: mock.port,
            password: Some("right".into()),
        })
        .await
    else {
        panic!("configure failed")
    };
    assert!(connected(&view), "{view:?}");

    let saved = std::fs::read_to_string(h.dir.join("obs.json")).unwrap();
    assert!(
        saved.contains("right"),
        "the password is saved on the device"
    );
    let shown = serde_json::to_string(&view).unwrap();
    assert!(
        !shown.contains("right"),
        "but never sent to the UI: {shown}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn unconfigured_obs_matters_only_with_a_stream_output() {
    // Nothing listening on the default port is a fair assumption for a
    // test machine, but use a port that was just free to be sure.
    let free = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    let h = start(dir, path).await;
    // No obs.json: the default settings, and nothing to tell the operator.
    let view = h.status().await;
    assert_eq!((view.host.as_str(), view.port), ("localhost", 4455));
    assert!(!view.relevant);

    h.streaming_output.store(true, Ordering::SeqCst);
    assert!(h.status().await.relevant);

    // Settings saved for a free port: not running, and Open OBS starts it.
    let Response::Obs(view) = h
        .control
        .handle(ObsRequest::Configure {
            host: "localhost".into(),
            port: free,
            password: None,
        })
        .await
    else {
        panic!("configure failed")
    };
    assert_eq!(view.problem, Some(ObsProblem::NotRunning), "{view:?}");
    assert!(matches!(
        h.control.handle(ObsRequest::Launch).await,
        Response::Obs(_)
    ));
    assert_eq!(
        h.machine.launched.lock().unwrap().as_slice(),
        ["flatpak run com.obsproject.Studio --minimize-to-tray"]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_settings_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    let h = start(dir, path).await;
    for (host, port) in [("  ", 4455), ("localhost", 0)] {
        let response = h
            .control
            .handle(ObsRequest::Configure {
                host: host.into(),
                port,
                password: None,
            })
            .await;
        assert!(
            matches!(response, Response::Error { .. }),
            "{host:?} {port}"
        );
    }
    assert!(!h.dir.join("obs.json").exists());
}

/// Against the OBS on this machine, by hand: what Lampstand detects, then
/// Open OBS and what it detects once OBS is up. Starts OBS. Run with
/// `cargo test -p lampstand-app --test obs_control -- --ignored --nocapture`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "starts the real OBS on this machine"]
async fn real_obs_on_this_machine() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("obs.json"),
        json!({ "host": "localhost", "port": 4455 }).to_string(),
    )
    .unwrap();
    let control = ObsControl::start(
        dir.path(),
        Arc::new(AtomicBool::new(true)),
        Arc::new(lampstand_app::obs_control::System),
        Os::current(),
        Arc::new(|_| {}),
    );
    let status = || async {
        match control.handle(ObsRequest::Status).await {
            Response::Obs(view) => view,
            other => panic!("{other:?}"),
        }
    };
    tokio::time::sleep(Duration::from_secs(2)).await;
    let before = status().await;
    println!(
        "before: {:?} problem={:?}",
        before.connection, before.problem
    );
    if before.problem == Some(ObsProblem::NotRunning) {
        println!("launch: {:?}", control.handle(ObsRequest::Launch).await);
        for _ in 0..30 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let view = status().await;
            if view.problem != Some(ObsProblem::NotRunning) {
                break;
            }
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    let after = status().await;
    println!("after: {:?} problem={:?}", after.connection, after.problem);
}
