//! OBS control (T89) against a mock obs-websocket server: the view goes
//! through every connection state, says why OBS isn't connected, follows
//! streaming, and keeps the password to itself. Then scenes per item
//! (T90), and Present's OBS drawer (T91): screenshots and audio mutes.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use lampstand_api::{
    Event, ObsAudioInput, ObsConnection, ObsProblem, ObsRequest, ObsSceneFailure, ObsSceneProblem,
    ObsView, Response,
};
use lampstand_app::obs_control::{ObsControl, Os, Probe, SceneCue};
use lampstand_connectors::obs::auth_response;
use lampstand_core::output::OutputId;
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpSocket};
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
    /// Its scenes (`MOCK_SCENES`, top of the list first), the one on air and every
    /// `SetCurrentProgramScene` it accepted.
    scenes: Arc<Mutex<Scenes>>,
    events: mpsc::UnboundedSender<Value>,
    stop: watch::Sender<bool>,
}

#[derive(Default)]
struct Scenes {
    program: String,
    switched: Vec<String>,
    /// Its inputs: name, kind, and muted if it has audio.
    inputs: Vec<(String, &'static str, Option<bool>)>,
    /// Every `GetSourceScreenshot`: the source and width.
    screenshots: Vec<(String, u64)>,
    /// What "Set up OBS" works on (#133).
    setup: SetupState,
}

/// Scene collections, the scenes added to them, what is in each scene and
/// each browser source's settings, as OBS keeps them.
struct SetupState {
    current: String,
    collections: Vec<String>,
    scenes: Vec<String>,
    items: Vec<(String, Vec<String>)>,
    browser_sources: Vec<(String, Value)>,
    /// Every request that changes something, by type and what it named.
    writes: Vec<String>,
    /// Outputs that are running, and a scene OBS will not make.
    virtual_cam: bool,
    replay_buffer: bool,
    fail_create_scene: bool,
}

impl Default for SetupState {
    fn default() -> Self {
        Self {
            current: "Church".into(),
            collections: vec!["Church".into()],
            scenes: Vec::new(),
            items: Vec::new(),
            browser_sources: Vec::new(),
            writes: Vec::new(),
            virtual_cam: false,
            replay_buffer: false,
            fail_create_scene: false,
        }
    }
}

const MOCK_SCENES: [&str; 3] = ["Worship", "Sermon", "Wide"];

impl MockObs {
    async fn start(password: Option<&'static str>) -> Self {
        Self::start_on(TcpListener::bind("127.0.0.1:0").await.unwrap(), password)
    }

    /// On a given port, for an OBS that starts after Lampstand.
    fn start_on(listener: TcpListener, password: Option<&'static str>) -> Self {
        let port = listener.local_addr().unwrap().port();
        let scenes = Arc::new(Mutex::new(Scenes {
            program: "Worship".into(),
            inputs: vec![
                ("Pulpit mic".into(), "coreaudio_input_capture", Some(false)),
                // Has sound, but isn't a mixer channel anyone mutes.
                ("Camera".into(), "av_capture_input_v2", Some(false)),
                ("Lampstand stream".into(), "browser_source", Some(false)),
                ("Band".into(), "wasapi_input_capture", Some(true)),
            ],
            ..Scenes::default()
        }));
        let streaming = Arc::new(AtomicBool::new(false));
        let (events, events_rx) = mpsc::unbounded_channel::<Value>();
        let events_rx = Arc::new(tokio::sync::Mutex::new(events_rx));
        let (stop, stopped) = watch::channel(false);
        let mock = Self {
            port,
            streaming: streaming.clone(),
            scenes: scenes.clone(),
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
                    scenes.clone(),
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

    fn switched(&self) -> Vec<String> {
        self.scenes.lock().unwrap().switched.clone()
    }

    /// The writes "Set up OBS" has made since the last call.
    fn writes(&self) -> Vec<String> {
        std::mem::take(&mut self.scenes.lock().unwrap().setup.writes)
    }

    fn browser_source(&self, name: &str) -> Option<Value> {
        let sc = self.scenes.lock().unwrap();
        sc.setup
            .browser_sources
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
    }

    fn screenshots(&self) -> Vec<(String, u64)> {
        self.scenes.lock().unwrap().screenshots.clone()
    }

    /// Someone mutes or unmutes an input in OBS itself.
    fn mute_in_obs(&self, input: &str, muted: bool) {
        for (name, _, m) in &mut self.scenes.lock().unwrap().inputs {
            if name == input {
                *m = Some(muted);
            }
        }
        self.events
            .send(json!({
                "eventType": "InputMuteStateChanged", "eventIntent": 8,
                "eventData": { "inputName": input, "inputMuted": muted },
            }))
            .unwrap();
    }

    /// Someone adds an audio input in OBS itself.
    fn add_input_in_obs(&self, input: &str) {
        self.scenes
            .lock()
            .unwrap()
            .inputs
            .push((input.into(), "pulse_input_capture", Some(false)));
        self.events
            .send(json!({
                "eventType": "InputCreated", "eventIntent": 8,
                "eventData": { "inputName": input, "inputKind": "coreaudio_input_capture" },
            }))
            .unwrap();
    }

    /// Someone switches scene in OBS itself.
    fn switch_in_obs(&self, scene: &str) {
        self.scenes.lock().unwrap().program = scene.into();
        self.events
            .send(json!({
                "eventType": "CurrentProgramSceneChanged", "eventIntent": 4,
                "eventData": { "sceneName": scene },
            }))
            .unwrap();
    }
}

type ServerWs = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

async fn serve(
    mut ws: ServerWs,
    password: Option<&'static str>,
    streaming: Arc<AtomicBool>,
    scenes: Arc<Mutex<Scenes>>,
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
                let mut status = json!({ "result": true, "code": 100 });
                let mut echo = None;
                let data = match request_type {
                    "GetSceneList" => {
                        let program = scenes.lock().unwrap().program.clone();
                        let names: Vec<String> = MOCK_SCENES
                            .iter()
                            .map(|s| s.to_string())
                            .chain(scenes.lock().unwrap().setup.scenes.clone())
                            .collect();
                        let list: Vec<Value> = names
                            .iter()
                            .rev()
                            .enumerate()
                            .map(|(i, name)| json!({ "sceneIndex": i, "sceneName": name }))
                            .collect();
                        json!({ "currentProgramSceneName": program, "currentPreviewSceneName": null, "scenes": list })
                    }
                    "SetCurrentProgramScene" => {
                        let name = d["requestData"]["sceneName"].as_str().unwrap_or_default().to_string();
                        if MOCK_SCENES.contains(&name.as_str()) {
                            let mut sc = scenes.lock().unwrap();
                            sc.program = name.clone();
                            sc.switched.push(name.clone());
                            echo = Some(name);
                        } else {
                            status = json!({ "result": false, "code": 600, "comment": format!("No source was found by the name of `{name}`.") });
                        }
                        Value::Null
                    }
                    "GetSourceScreenshot" => {
                        let req = &d["requestData"];
                        let name = req["sourceName"].as_str().unwrap_or_default().to_string();
                        let width = req["imageWidth"].as_u64().unwrap_or_default();
                        scenes.lock().unwrap().screenshots.push((name.clone(), width));
                        if MOCK_SCENES.contains(&name.as_str()) {
                            json!({ "imageData": format!("data:image/jpg;base64,{name}") })
                        } else {
                            status = json!({ "result": false, "code": 600, "comment": "No source was found." });
                            Value::Null
                        }
                    }
                    "GetInputList" => {
                        let sc = scenes.lock().unwrap();
                        let inputs: Vec<Value> = sc.inputs.iter()
                            .map(|(name, kind, _)| (name.clone(), *kind))
                            .chain(sc.setup.browser_sources.iter().map(|(name, _)| (name.clone(), "browser_source")))
                            .map(|(name, kind)| json!({ "inputName": name, "inputKind": kind, "unversionedInputKind": kind }))
                            .collect();
                        json!({ "inputs": inputs })
                    }
                    "GetVirtualCamStatus" => json!({ "outputActive": scenes.lock().unwrap().setup.virtual_cam }),
                    "GetReplayBufferStatus" => {
                        let on = scenes.lock().unwrap().setup.replay_buffer;
                        json!({ "outputActive": on })
                    }
                    "GetSceneCollectionList" => {
                        let sc = scenes.lock().unwrap();
                        json!({ "currentSceneCollectionName": sc.setup.current, "sceneCollections": sc.setup.collections })
                    }
                    "CreateSceneCollection" | "SetCurrentSceneCollection" => {
                        let name = d["requestData"]["sceneCollectionName"].as_str().unwrap_or_default().to_string();
                        let mut sc = scenes.lock().unwrap();
                        sc.setup.writes.push(format!("{request_type} {name}"));
                        if request_type == "CreateSceneCollection" {
                            sc.setup.collections.push(name.clone());
                        }
                        sc.setup.current = name;
                        Value::Null
                    }
                    "CreateScene" => {
                        let name = d["requestData"]["sceneName"].as_str().unwrap_or_default().to_string();
                        let mut sc = scenes.lock().unwrap();
                        sc.setup.writes.push(format!("CreateScene {name}"));
                        if sc.setup.fail_create_scene {
                            status = json!({ "result": false, "code": 500, "comment": "boom, C:\\Users\\secret" });
                        } else if MOCK_SCENES.contains(&name.as_str()) || sc.setup.scenes.contains(&name) {
                            status = json!({ "result": false, "code": 601, "comment": "already exists" });
                        } else {
                            sc.setup.scenes.push(name);
                        }
                        Value::Null
                    }
                    "CreateInput" | "CreateSceneItem" => {
                        let req = &d["requestData"];
                        let scene = req["sceneName"].as_str().unwrap_or_default().to_string();
                        let source = req["inputName"].as_str().or(req["sourceName"].as_str()).unwrap_or_default().to_string();
                        let mut sc = scenes.lock().unwrap();
                        sc.setup.writes.push(format!("{request_type} {scene}/{source}"));
                        let known = sc.setup.browser_sources.iter().any(|(n, _)| *n == source);
                        if request_type == "CreateInput" && known {
                            status = json!({ "result": false, "code": 601, "comment": "already exists" });
                        } else {
                            if request_type == "CreateInput" {
                                sc.setup.browser_sources.push((source.clone(), req["inputSettings"].clone()));
                            }
                            match sc.setup.items.iter_mut().find(|(n, _)| *n == scene) {
                                Some((_, items)) => items.push(source),
                                None => sc.setup.items.push((scene, vec![source])),
                            }
                        }
                        Value::Null
                    }
                    "GetSceneItemList" => {
                        let scene = d["requestData"]["sceneName"].as_str().unwrap_or_default().to_string();
                        let sc = scenes.lock().unwrap();
                        let items: Vec<Value> = sc.setup.items.iter()
                            .filter(|(n, _)| *n == scene)
                            .flat_map(|(_, items)| items.iter())
                            .map(|source| json!({ "sourceName": source }))
                            .collect();
                        json!({ "sceneItems": items })
                    }
                    "GetInputSettings" => {
                        let name = d["requestData"]["inputName"].as_str().unwrap_or_default().to_string();
                        let sc = scenes.lock().unwrap();
                        match sc.setup.browser_sources.iter().find(|(n, _)| *n == name) {
                            Some((_, settings)) => json!({ "inputSettings": settings, "inputKind": "browser_source" }),
                            None => {
                                status = json!({ "result": false, "code": 600, "comment": "no such input" });
                                Value::Null
                            }
                        }
                    }
                    "SetInputSettings" => {
                        let req = &d["requestData"];
                        let name = req["inputName"].as_str().unwrap_or_default().to_string();
                        let mut sc = scenes.lock().unwrap();
                        sc.setup.writes.push(format!("SetInputSettings {name}"));
                        if let Some((_, settings)) = sc.setup.browser_sources.iter_mut().find(|(n, _)| *n == name) {
                            for (k, v) in req["inputSettings"].as_object().cloned().unwrap_or_default() {
                                settings[k] = v;
                            }
                        }
                        Value::Null
                    }
                    "GetInputMute" | "SetInputMute" => {
                        let req = &d["requestData"];
                        let name = req["inputName"].as_str().unwrap_or_default().to_string();
                        let set = req["inputMuted"].as_bool();
                        let mut sc = scenes.lock().unwrap();
                        match sc.inputs.iter_mut().find(|(n, _, _)| *n == name) {
                            Some((_, _, Some(muted))) => {
                                if let Some(set) = set {
                                    *muted = set;
                                }
                                json!({ "inputMuted": *muted })
                            }
                            // OBS: "The specified input does not support audio."
                            _ => {
                                status = json!({ "result": false, "code": 604, "comment": "The specified input does not support audio." });
                                Value::Null
                            }
                        }
                    }
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
                // Like OBS: the event announcing the switch can arrive
                // before the answer to the request.
                if let Some(scene) = echo {
                    send(&mut ws, 5, json!({
                        "eventType": "CurrentProgramSceneChanged", "eventIntent": 4,
                        "eventData": { "sceneName": scene },
                    })).await;
                }
                send(&mut ws, 7, json!({
                    "requestType": request_type, "requestId": d["requestId"],
                    "requestStatus": status,
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
    /// A computer on two networks.
    fn addresses(&self) -> Vec<String> {
        vec!["10.0.0.7".into(), "192.168.1.20".into()]
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
    /// The host the app's local server has now, which the shell's address
    /// watch changes when the network does. The app's picture address is
    /// built from it.
    server_host: Arc<Mutex<String>>,
    /// Every output the app was asked for a picture of.
    asked: Arc<Mutex<Vec<OutputId>>>,
    _dir: tempfile::TempDir,
    dir: PathBuf,
}

/// A controller with OBS settings saved for `port`, as Setup would.
async fn harness(port: u16, password: Option<&str>) -> Harness {
    harness_on("127.0.0.1", port, password).await
}

/// The same, with OBS at `host`.
async fn harness_on(host: &str, port: u16, password: Option<&str>) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    let mut settings = json!({ "host": host, "port": port });
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
                sink.lock().unwrap().push(*view);
            }
        }),
    );
    // The app, as the shell wires it: the picture's address and size from
    // the output, with the server's host as the address watch last left it.
    let server_host = Arc::new(Mutex::new("127.0.0.1".to_string()));
    let asked = Arc::new(Mutex::new(Vec::new()));
    let (host, log) = (server_host.clone(), asked.clone());
    control.set_pictures(Arc::new(move |output| {
        log.lock().unwrap().push(output);
        let host = host.lock().unwrap().clone();
        Ok((format!("http://{host}:7340/obs/6c9f?k=sekret"), 1920, 1080))
    }));
    Harness {
        control,
        machine,
        streaming_output,
        pushed,
        server_host,
        asked,
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

/// A port nothing listens on, for as long as the socket is kept: bound, so
/// no other test can be handed it, but never listening, so a connection
/// is refused. (A port found by binding and dropping a listener can be
/// taken by someone else before it is used.)
fn unused_port(ip: &str) -> (TcpSocket, u16) {
    let socket = TcpSocket::new_v4().unwrap();
    socket.bind(format!("{ip}:0").parse().unwrap()).unwrap();
    let port = socket.local_addr().unwrap().port();
    (socket, port)
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
    // Nothing listens on this port for the whole test.
    let (_reserved, free) = unused_port("127.0.0.1");
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
    // Configuring answers while the first connection attempt is still under
    // way, so wait for it to fail before looking for the problem.
    let Response::Obs(_) = h
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
    let view = h
        .wait_for(|v| matches!(v.connection, ObsConnection::Disconnected { .. }))
        .await;
    assert_eq!(view.problem, Some(ObsProblem::NotRunning), "{view:?}");
    assert!(matches!(
        h.control.handle(ObsRequest::Launch).await,
        Response::Obs(_)
    ));
    assert_eq!(
        h.machine.launched.lock().unwrap().as_slice(),
        ["flatpak run com.obsproject.Studio --minimize-to-tray"]
    );
    assert!(matches!(
        h.control.handle(ObsRequest::Download).await,
        Response::Obs(_)
    ));
    assert_eq!(
        h.machine
            .launched
            .lock()
            .unwrap()
            .last()
            .map(String::as_str),
        Some("xdg-open https://obsproject.com/download")
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

/// The drawer (T91) against the real OBS, with the settings Lampstand
/// saved in `LAMPSTAND_OBS_DIR` (the app-data folder holding `obs.json`).
/// Read-only: lists the scenes and mutes and saves a few program pictures
/// to `LAMPSTAND_OBS_OUT`, without switching or muting anything.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "talks to the real OBS on this machine"]
async fn real_obs_drawer() {
    let dir = PathBuf::from(std::env::var("LAMPSTAND_OBS_DIR").expect("LAMPSTAND_OBS_DIR"));
    let out = PathBuf::from(std::env::var("LAMPSTAND_OBS_OUT").expect("LAMPSTAND_OBS_OUT"));
    let control = ObsControl::start(
        &dir,
        Arc::new(AtomicBool::new(true)),
        Arc::new(lampstand_app::obs_control::System),
        Os::current(),
        Arc::new(|_| {}),
    );
    let mut view = None;
    for _ in 0..50 {
        if let Response::Obs(v) = control.handle(ObsRequest::Status).await {
            if connected(&v) && v.program_scene.is_some() {
                view = Some(v);
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let view = view.expect("OBS never connected");
    println!("scenes: {:?}", view.scenes);
    println!("program: {:?}", view.program_scene);
    println!("audio: {:?}", view.audio);
    let started = std::time::Instant::now();
    for i in 0..8 {
        let scene = if i == 0 {
            view.scenes.last().cloned()
        } else {
            None
        };
        let response = control
            .handle(ObsRequest::Screenshot { scene, width: 640 })
            .await;
        let Response::Thumbnail(url) = response else {
            panic!("{response:?}")
        };
        let (head, data) = url.split_once(',').unwrap();
        assert_eq!(head, "data:image/jpeg;base64");
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .unwrap();
        assert_eq!(&bytes[..2], [0xff, 0xd8], "a JPEG");
        std::fs::write(out.join(format!("obs-{i}.jpg")), bytes).unwrap();
    }
    println!("8 pictures in {:?}", started.elapsed());
}

fn cue(item: lampstand_core::service::ItemId, scene: Option<&str>) -> SceneCue {
    SceneCue {
        item,
        scene: scene.map(str::to_string),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn items_switch_scenes_and_a_hand_pick_holds() {
    use lampstand_core::service::ItemId;
    let mock = MockObs::start(None).await;
    let h = harness(mock.port, None).await;
    let view = h.wait_for(|v| connected(v) && !v.scenes.is_empty()).await;
    assert_eq!(view.scenes, MOCK_SCENES, "top of OBS's list first");
    assert_eq!(view.program_scene.as_deref(), Some("Worship"));

    // The sermon goes live: OBS switches, and its echo isn't a hand pick.
    let cues = h.control.cues();
    cues.send(cue(ItemId::generate(), Some("Sermon"))).unwrap();
    let view = h
        .wait_for(|v| v.program_scene.as_deref() == Some("Sermon"))
        .await;
    assert!(!view.held);
    assert_eq!(view.scene_problem, None);
    assert_eq!(mock.switched(), ["Sermon"]);

    // Someone cuts to the wide shot in OBS: held, and nothing fights it.
    mock.switch_in_obs("Wide");
    let view = h.wait_for(|v| v.held).await;
    assert_eq!(view.program_scene.as_deref(), Some("Wide"));

    // The closing song applies its own scene and ends the hold.
    cues.send(cue(ItemId::generate(), Some("Worship"))).unwrap();
    let view = h
        .wait_for(|v| !v.held && v.program_scene.as_deref() == Some("Worship"))
        .await;
    assert_eq!(view.scene_problem, None);
    assert_eq!(mock.switched(), ["Sermon", "Worship"]);

    // Picked by hand in Present (T91), then resumed.
    assert!(matches!(
        h.control
            .handle(ObsRequest::SetScene {
                scene: "Wide".into()
            })
            .await,
        Response::Obs(ObsView { held: true, .. })
    ));
    let Response::Obs(view) = h.control.handle(ObsRequest::Resume).await else {
        panic!("resume failed")
    };
    assert!(!view.held);
    assert_eq!(mock.switched(), ["Sermon", "Worship", "Wide", "Worship"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_scene_warns_without_stopping_anything() {
    use lampstand_core::service::ItemId;
    let mock = MockObs::start(None).await;
    let h = harness(mock.port, None).await;
    h.wait_for(connected).await;
    h.control
        .cues()
        .send(cue(ItemId::generate(), Some("Pastor and slides")))
        .unwrap();
    let view = h.wait_for(|v| v.scene_problem.is_some()).await;
    assert_eq!(
        view.scene_problem,
        Some(ObsSceneProblem {
            scene: "Pastor and slides".into(),
            reason: ObsSceneFailure::Missing,
        })
    );
    assert!(mock.switched().is_empty());
    assert!(connected(&view), "OBS is still connected and usable");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_switch_while_obs_is_away_happens_when_it_connects() {
    use lampstand_core::service::ItemId;
    // OBS isn't running yet: reserve a port for it.
    let (reserved, port) = unused_port("127.0.0.1");
    let h = harness(port, None).await;
    h.wait_for(|v| matches!(v.connection, ObsConnection::Disconnected { .. }))
        .await;

    h.control
        .cues()
        .send(cue(ItemId::generate(), Some("Sermon")))
        .unwrap();
    let view = h.wait_for(|v| v.scene_problem.is_some()).await;
    assert_eq!(
        view.scene_problem.map(|p| p.reason),
        Some(ObsSceneFailure::NotConnected)
    );

    // OBS starts on that port: the sermon's scene goes on air.
    // The reserved socket starts listening, so the port is never up for grabs.
    let mock = MockObs::start_on(reserved.listen(1024).unwrap(), None);
    let view = h
        .wait_for(|v| v.program_scene.as_deref() == Some("Sermon"))
        .await;
    assert_eq!(view.scene_problem, None);
    assert_eq!(mock.switched(), ["Sermon"]);
}

fn audio(list: &[(&str, bool)]) -> Vec<ObsAudioInput> {
    list.iter()
        .map(|(name, muted)| ObsAudioInput {
            name: name.to_string(),
            muted: *muted,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_drawer_previews_scenes_live() {
    let mock = MockObs::start(None).await;
    let h = harness(mock.port, None).await;
    h.wait_for(|v| v.program_scene.is_some()).await;

    // The program scene, again and again while the drawer is open.
    for _ in 0..3 {
        let response = h
            .control
            .handle(ObsRequest::Screenshot {
                scene: None,
                width: 480,
            })
            .await;
        assert_eq!(
            response,
            Response::Thumbnail("data:image/jpeg;base64,Worship".into())
        );
    }
    // A scene before switching to it; nothing switches.
    let response = h
        .control
        .handle(ObsRequest::Screenshot {
            scene: Some("Sermon".into()),
            width: 320,
        })
        .await;
    assert_eq!(
        response,
        Response::Thumbnail("data:image/jpeg;base64,Sermon".into())
    );
    assert!(mock.switched().is_empty());

    // After a switch in OBS, the preview follows the new program scene.
    mock.switch_in_obs("Wide");
    h.wait_for(|v| v.program_scene.as_deref() == Some("Wide"))
        .await;
    let response = h
        .control
        .handle(ObsRequest::Screenshot {
            scene: None,
            width: 480,
        })
        .await;
    assert_eq!(
        response,
        Response::Thumbnail("data:image/jpeg;base64,Wide".into())
    );
    assert_eq!(
        mock.screenshots(),
        [
            ("Worship".into(), 480),
            ("Worship".into(), 480),
            ("Worship".into(), 480),
            ("Sermon".into(), 320),
            ("Wide".into(), 480),
        ]
    );

    // A scene OBS doesn't have is an error, not a picture.
    assert!(matches!(
        h.control
            .handle(ObsRequest::Screenshot {
                scene: Some("Gone".into()),
                width: 480,
            })
            .await,
        Response::Error { .. }
    ));

    // No OBS: an error at once, so the drawer can say so.
    mock.stop();
    h.wait_for(|v| !connected(v)).await;
    assert!(matches!(
        h.control
            .handle(ObsRequest::Screenshot {
                scene: None,
                width: 480,
            })
            .await,
        Response::Error { .. }
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn mutes_follow_obs_and_present() {
    let mock = MockObs::start(None).await;
    let h = harness(mock.port, None).await;
    // Only audio devices: the camera and browser source have sound too.
    let view = h.wait_for(|v| !v.audio.is_empty()).await;
    assert_eq!(view.audio, audio(&[("Pulpit mic", false), ("Band", true)]));

    // Muted in Present: the answer already has it.
    let Response::Obs(view) = h
        .control
        .handle(ObsRequest::SetMute {
            input: "Pulpit mic".into(),
            muted: true,
        })
        .await
    else {
        panic!("mute failed")
    };
    assert_eq!(view.audio, audio(&[("Pulpit mic", true), ("Band", true)]));

    // Unmuted in OBS itself: the event updates the view, and it's pushed.
    mock.mute_in_obs("Band", false);
    h.wait_for(|v| v.audio == audio(&[("Pulpit mic", true), ("Band", false)]))
        .await;
    assert!(h
        .pushed
        .lock()
        .unwrap()
        .iter()
        .any(|v| v.audio == audio(&[("Pulpit mic", true), ("Band", false)])));

    // A new input in OBS appears.
    mock.add_input_in_obs("Choir mics");
    h.wait_for(|v| v.audio.len() == 3).await;

    // An input OBS can't mute is an error.
    assert!(matches!(
        h.control
            .handle(ObsRequest::SetMute {
                input: "Nowhere".into(),
                muted: true,
            })
            .await,
        Response::Error { .. }
    ));

    // Disconnected: no toggles to show.
    mock.stop();
    let view = h.wait_for(|v| !connected(v)).await;
    assert!(view.audio.is_empty());
}

// ---- OBS on another computer (#133) --------------------------------------

/// This computer's own network address, if it is on a network. The fake OBS
/// listens on every interface and is reached at this address, so Lampstand
/// sees a host that isn't loopback, as it does with OBS on another computer.
/// This is a stand-in for a second machine: the packets never leave this one.
fn own_lan_address() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then(|| ip.to_string())
}

/// A fake OBS for the "Lampstand" collection, reached "remotely".
async fn remote_obs() -> Option<(MockObs, Harness)> {
    let Some(lan) = own_lan_address() else {
        // Never a silent pass: without a network these tests prove nothing.
        panic!(
            "these tests need this computer on a network (any address that isn't loopback), \
             because the fake OBS is reached at that address to stand in for another computer"
        );
    };
    let listener = TcpListener::bind("0.0.0.0:0").await.unwrap();
    let mock = MockObs::start_on(listener, None);
    // The fake starts with a "Lampstand stream" audio input for the drawer
    // tests; this OBS has never been set up.
    mock.scenes
        .lock()
        .unwrap()
        .inputs
        .retain(|(name, _, _)| name != "Lampstand stream");
    let h = harness_on(&lan, mock.port, None).await;
    h.wait_for(connected).await;
    Some((mock, h))
}

/// Sets up with the picture the app has (the window sends only the output).
async fn set_up(h: &Harness) -> Response {
    h.control
        .handle(ObsRequest::SetUp {
            output: OutputId::generate(),
        })
        .await
}

fn text(response: Response) -> String {
    match response {
        Response::Text(t) | Response::Error { message: t } => t,
        other => panic!("expected words, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn set_up_writes_this_computers_address_into_a_remote_obs() {
    let Some((mock, h)) = remote_obs().await else {
        return;
    };
    let view = h.status().await;
    assert!(view.remote);
    assert_eq!(view.addresses, ["10.0.0.7", "192.168.1.20"]);
    assert_eq!(view.pinned_address, None);

    // The picture's URL says localhost (this computer found no network
    // address): refused, and nothing written to OBS.
    let refused = text(set_up(&h).await);
    assert!(refused.contains("no network address"), "{refused}");
    assert!(mock.writes().is_empty());

    // Pin the address OBS should use; a bad one is refused.
    assert!(matches!(
        h.control
            .handle(ObsRequest::UseAddress {
                address: Some("localhost".into())
            })
            .await,
        Response::Error { .. }
    ));
    let view = match h
        .control
        .handle(ObsRequest::UseAddress {
            address: Some("192.168.1.20".into()),
        })
        .await
    {
        Response::Obs(view) => view,
        other => panic!("{other:?}"),
    };
    assert_eq!(view.pinned_address.as_deref(), Some("192.168.1.20"));

    let done = text(set_up(&h).await);
    assert!(done.contains("OBS is set up"), "{done}");
    // Neither the key nor the host's secrets are repeated in the answer.
    assert!(!done.contains("sekret"), "{done}");
    let source = mock.browser_source("Lampstand stream").unwrap();
    assert_eq!(
        source["url"], "http://192.168.1.20:7340/obs/6c9f?k=sekret",
        "never localhost"
    );
    assert_eq!(
        (source["width"].as_u64(), source["height"].as_u64()),
        (Some(1920), Some(1080))
    );
    assert_eq!(source["shutdown"], false);
    let writes = mock.writes();
    assert_eq!(writes[0], "CreateSceneCollection Lampstand");
    assert!(writes.contains(&"CreateScene Slides".to_string()));
    assert!(writes.contains(&"CreateScene Pastor and slides".to_string()));
    // The church's own scenes were not made again or edited.
    for own in ["Sermon", "Wide"] {
        assert!(!writes.iter().any(|w| w.contains(own)), "{writes:?}");
    }
    {
        let sc = mock.scenes.lock().unwrap();
        for scene in ["Slides", "Worship", "Pastor and slides"] {
            let items = &sc.setup.items.iter().find(|(n, _)| n == scene).unwrap().1;
            assert_eq!(items, &["Lampstand stream"], "{scene}");
        }
    }

    // Again: nothing changes.
    assert!(text(set_up(&h).await).contains("Nothing changed"));
    assert!(mock.writes().is_empty());

    // The address is pinned to the other network: only the source changes.
    h.control
        .handle(ObsRequest::UseAddress {
            address: Some("10.0.0.7".into()),
        })
        .await;
    assert!(text(set_up(&h).await).contains("OBS is set up"));
    assert_eq!(mock.writes(), ["SetInputSettings Lampstand stream"]);
    assert_eq!(
        mock.browser_source("Lampstand stream").unwrap()["url"],
        "http://10.0.0.7:7340/obs/6c9f?k=sekret"
    );

    // The pin survives a restart, in obs.json beside the connection.
    let saved = std::fs::read_to_string(h.dir.join("obs.json")).unwrap();
    assert!(
        saved.contains("\"pinned_address\": \"10.0.0.7\""),
        "{saved}"
    );
    mock.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn set_up_waits_while_obs_is_on_air_and_for_a_connection() {
    let Some((mock, h)) = remote_obs().await else {
        return;
    };
    h.control
        .handle(ObsRequest::UseAddress {
            address: Some("192.168.1.20".into()),
        })
        .await;
    mock.streaming.store(true, Ordering::SeqCst);
    mock.events
        .send(json!({
            "eventType": "StreamStateChanged", "eventIntent": 64,
            "eventData": { "outputActive": true, "outputState": "OBS_WEBSOCKET_OUTPUT_STARTED" },
        }))
        .unwrap();
    h.wait_for(|v| v.stream.as_ref().is_some_and(|s| s.active))
        .await;
    let words = text(set_up(&h).await);
    assert!(words.contains("streaming, recording"), "{words}");
    assert!(mock.writes().is_empty());
    mock.stop();

    // OBS gone: it says Lampstand isn't connected, and writes nothing.
    h.wait_for(|v| matches!(v.connection, ObsConnection::Disconnected { .. }))
        .await;
    let words = text(set_up(&h).await);
    assert!(words.contains("isn't connected to OBS"), "{words}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_remote_obs_that_is_closed_is_diagnosed_in_plain_words() {
    let lan = own_lan_address().expect("this computer needs a network address for this test");
    // This computer answers but nothing listens on OBS's port.
    // A bound socket that never listens isn't refused on every system when
    // it is reached by this computer's own address, so free the port.
    let free = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
    let port = free.local_addr().unwrap().port();
    drop(free);
    let h = harness_on(&lan, port, None).await;
    let view = h
        .wait_for(|v| matches!(v.connection, ObsConnection::Disconnected { .. }))
        .await;
    assert!(view.remote);
    assert_eq!(view.problem, Some(ObsProblem::RemoteNotListening));
    // Detection of a local OBS is for this computer only.
    assert!(h.machine.launched.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_church_with_obs_on_this_computer_sees_nothing_new() {
    let mock = MockObs::start(None).await;
    let h = harness(mock.port, None).await;
    let view = h.wait_for(connected).await;
    // No remote part of the view, whatever the computer's addresses.
    assert!(!view.remote);
    assert!(view.addresses.is_empty());
    assert_eq!(view.pinned_address, None);
    assert_eq!(view.problem, None);

    // Saving the connection writes the same obs.json as 0.7.0: host, port.
    h.control
        .handle(ObsRequest::Configure {
            host: "localhost".into(),
            port: mock.port,
            password: None,
        })
        .await;
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(h.dir.join("obs.json")).unwrap()).unwrap();
    assert_eq!(saved, json!({ "host": "localhost", "port": mock.port }));

    // A pin changes nothing for OBS on this computer: not shown, and the
    // picture's URL is left as it is.
    h.control
        .handle(ObsRequest::UseAddress {
            address: Some("192.168.1.20".into()),
        })
        .await;
    let view = h.wait_for(connected).await;
    assert_eq!(view.pinned_address, None);
    assert!(view.addresses.is_empty());
    let url = "http://127.0.0.1:7340/obs/6c9f?k=sekret";
    assert!(text(set_up(&h).await).contains("OBS is set up"));
    assert_eq!(mock.browser_source("Lampstand stream").unwrap()["url"], url);

    // A refusing local OBS is diagnosed as before.
    mock.stop();
    let view = h
        .wait_for(|v| matches!(v.connection, ObsConnection::Disconnected { .. }))
        .await;
    assert!(matches!(
        view.problem,
        Some(ObsProblem::NotRunning | ObsProblem::ServerOff | ObsProblem::NotInstalled)
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn set_up_waits_for_the_virtual_camera_and_replay_buffer_too() {
    let (mock, h) = remote_obs().await.unwrap();
    h.control
        .handle(ObsRequest::UseAddress {
            address: Some("192.168.1.20".into()),
        })
        .await;
    for which in ["virtual camera", "replay buffer"] {
        {
            let mut sc = mock.scenes.lock().unwrap();
            sc.setup.virtual_cam = which == "virtual camera";
            sc.setup.replay_buffer = which == "replay buffer";
        }
        let words = text(set_up(&h).await);
        assert!(
            words.contains("sending its picture out"),
            "{which}: {words}"
        );
        assert!(mock.writes().is_empty(), "{which}");
    }
    mock.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_setup_that_fails_part_way_puts_obs_back_and_keeps_obs_words_out() {
    let (mock, h) = remote_obs().await.unwrap();
    h.control
        .handle(ObsRequest::UseAddress {
            address: Some("192.168.1.20".into()),
        })
        .await;
    mock.scenes.lock().unwrap().setup.fail_create_scene = true;
    let words = text(set_up(&h).await);
    assert!(words.contains("OBS is back on “Church”"), "{words}");
    // OBS's own error text (and anything in it) never reaches the volunteer.
    assert!(
        !words.contains("boom") && !words.contains("secret"),
        "{words}"
    );
    assert_eq!(mock.scenes.lock().unwrap().setup.current, "Church");
    mock.scenes.lock().unwrap().setup.fail_create_scene = false;
    mock.writes();

    // Next time it works, and says OBS is now on the new collection.
    let done = text(set_up(&h).await);
    assert!(
        done.contains("OBS is now showing the “Lampstand” collection"),
        "{done}"
    );
    // Already on it: nothing to say about switching.
    let again = text(set_up(&h).await);
    assert!(again.contains("Nothing changed"), "{again}");
    assert!(!again.contains("now showing"), "{again}");
    mock.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pin_must_be_an_address_this_computer_has() {
    let (mock, h) = remote_obs().await.unwrap();
    for bad in ["192.168.1.99", "999.1.1.1", "169.254.1.1", "localhost"] {
        assert!(
            matches!(
                h.control
                    .handle(ObsRequest::UseAddress {
                        address: Some(bad.into())
                    })
                    .await,
                Response::Error { .. }
            ),
            "{bad}"
        );
    }
    assert_eq!(h.status().await.pinned_address, None);
    // A picture address that isn't Lampstand's own never reaches OBS.
    h.control
        .handle(ObsRequest::UseAddress {
            address: Some("10.0.0.7".into()),
        })
        .await;
    // Even the app's own address is checked before it reaches OBS.
    *h.server_host.lock().unwrap() = "u:p@10.0.0.7".into();
    let words = text(set_up(&h).await);
    assert!(words.contains("picture addresses"), "{words}");
    assert!(mock.writes().is_empty());
    mock.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_window_cannot_choose_the_address_and_the_pin_beats_the_address_watch() {
    let (mock, h) = remote_obs().await.unwrap();
    // A request that carries an address of its own: the field is not part
    // of the request, so it is ignored, and the app is asked for the output.
    let request: ObsRequest = serde_json::from_value(json!({
        "SetUp": { "output": OutputId::generate(), "url": "http://evil.example/obs/a?k=b" }
    }))
    .unwrap();
    *h.server_host.lock().unwrap() = "192.168.1.20".into();
    let done = text(h.control.handle(request).await);
    assert!(done.contains("OBS is set up"), "{done}");
    assert_eq!(h.asked.lock().unwrap().len(), 1);
    let url = || mock.browser_source("Lampstand stream").unwrap()["url"].clone();
    assert_eq!(url(), "http://192.168.1.20:7340/obs/6c9f?k=sekret");

    // The network changes and the shell's address watch moves the server's
    // address. With nothing pinned, the next setup follows it.
    *h.server_host.lock().unwrap() = "10.0.0.7".into();
    assert!(text(set_up(&h).await).contains("OBS is set up"));
    assert_eq!(url(), "http://10.0.0.7:7340/obs/6c9f?k=sekret");

    // With an address pinned, the pin wins over whatever the watch found.
    h.control
        .handle(ObsRequest::UseAddress {
            address: Some("192.168.1.20".into()),
        })
        .await;
    *h.server_host.lock().unwrap() = "10.0.0.7".into();
    assert!(text(set_up(&h).await).contains("OBS is set up"));
    assert_eq!(url(), "http://192.168.1.20:7340/obs/6c9f?k=sekret");
    *h.server_host.lock().unwrap() = "10.9.9.9".into();
    assert!(text(set_up(&h).await).contains("Nothing changed"));
    assert_eq!(url(), "http://192.168.1.20:7340/obs/6c9f?k=sekret");
    mock.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn set_up_without_the_app_or_for_a_picture_that_is_not_one_says_so() {
    let mock = MockObs::start(None).await;
    let h = harness(mock.port, None).await;
    h.wait_for(connected).await;
    h.control.set_pictures(Arc::new(|_| {
        Err("that output is not an OBS browser source".into())
    }));
    let words = text(set_up(&h).await);
    assert!(words.contains("not an OBS browser source"), "{words}");
    assert!(mock.writes().is_empty());
    mock.stop();
}
