//! The OBS client against a mock obs-websocket v5 server (T88).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use lampstand_connectors::obs::{
    auth_response, codes, Obs, ObsConfig, ObsError, ObsEvent, ObsStatus, StreamDestination,
};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, watch};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;

const SALT: &str = "lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI=";
const CHALLENGE: &str = "+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY=";

/// A pretend OBS. Answers a few requests the way OBS 30 does.
struct MockObs {
    port: u16,
    connections: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<(String, Value)>>>,
    /// Sending drops every open connection without a close frame.
    kick: broadcast::Sender<()>,
}

impl MockObs {
    async fn start(password: Option<&'static str>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let connections = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let destination = Arc::new(Mutex::new(json!({
            "streamServiceType": "rtmp_common",
            "streamServiceSettings": { "service": "YouTube - RTMPS", "server": "auto", "key": "old-key" },
        })));
        let (kick, _) = broadcast::channel(4);
        let mock = Self {
            port,
            connections: connections.clone(),
            requests: requests.clone(),
            kick: kick.clone(),
        };
        tokio::spawn(async move {
            loop {
                let (tcp, _) = listener.accept().await.unwrap();
                connections.fetch_add(1, Ordering::SeqCst);
                let ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
                tokio::spawn(serve(
                    ws,
                    password,
                    requests.clone(),
                    destination.clone(),
                    kick.subscribe(),
                ));
            }
        });
        mock
    }

    fn config(&self, password: Option<&str>) -> ObsConfig {
        ObsConfig {
            host: "127.0.0.1".into(),
            port: self.port,
            password: password.map(str::to_string),
        }
    }

    fn requests_of(&self, request_type: &str) -> Vec<Value> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(t, _)| t == request_type)
            .map(|(_, d)| d.clone())
            .collect()
    }
}

type ServerWs = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

async fn serve(
    mut ws: ServerWs,
    password: Option<&'static str>,
    requests: Arc<Mutex<Vec<(String, Value)>>>,
    destination: Arc<Mutex<Value>>,
    mut kick: broadcast::Receiver<()>,
) {
    let mut hello = json!({ "obsWebSocketVersion": "5.5.2", "rpcVersion": 1 });
    if password.is_some() {
        hello["authentication"] = json!({ "challenge": CHALLENGE, "salt": SALT });
    }
    send(&mut ws, 0, hello).await;
    let Some(identify) = recv(&mut ws).await else {
        return;
    };
    assert_eq!(identify["op"], 1);
    if let Some(password) = password {
        let expected = auth_response(password, SALT, CHALLENGE);
        if identify["d"]["authentication"].as_str() != Some(expected.as_str()) {
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

    loop {
        let message = tokio::select! {
            _ = kick.recv() => return,
            m = recv(&mut ws) => match m { Some(m) => m, None => return },
        };
        if message["op"] != 6 {
            continue;
        }
        let d = &message["d"];
        let request_type = d["requestType"].as_str().unwrap().to_string();
        let id = d["requestId"].clone();
        let data = d.get("requestData").cloned().unwrap_or(Value::Null);
        requests
            .lock()
            .unwrap()
            .push((request_type.clone(), data.clone()));
        let ok = |response: Value| {
            json!({
                "requestType": request_type, "requestId": id,
                "requestStatus": { "result": true, "code": 100 },
                "responseData": response,
            })
        };
        let reply = match request_type.as_str() {
            "GetVersion" => ok(json!({ "obsVersion": "30.2.3", "obsWebSocketVersion": "5.5.2" })),
            "GetSceneList" => ok(json!({
                "currentProgramSceneName": "Worship",
                "currentPreviewSceneName": null,
                "scenes": [
                    { "sceneIndex": 0, "sceneName": "Pastor and slides" },
                    { "sceneIndex": 2, "sceneName": "Countdown" },
                    { "sceneIndex": 1, "sceneName": "Worship" },
                ],
            })),
            "GetStreamServiceSettings" => ok(destination.lock().unwrap().clone()),
            "SetStreamServiceSettings" => {
                *destination.lock().unwrap() = data;
                ok(Value::Null)
            }
            "GetVirtualCamStatus" => ok(json!({ "outputActive": true })),
            // OBS 30 labels its JPEGs "image/jpg".
            "GetSourceScreenshot" if data["sourceName"] == "Worship" => ok(json!({
                "imageData": format!("data:image/jpg;base64,{}", data["imageWidth"]),
            })),
            "GetSourceScreenshot" => json!({
                "requestType": request_type, "requestId": id,
                "requestStatus": { "result": false, "code": 600, "comment": "No source was found." },
            }),
            "SetCurrentProgramScene" if data["sceneName"] == "Nowhere" => json!({
                "requestType": request_type, "requestId": id,
                "requestStatus": { "result": false, "code": 600, "comment": "No source was found by the name of `Nowhere`." },
            }),
            // Never answered.
            "Hang" => continue,
            // An event goes out while the request waits for its answer.
            "Slow" => {
                send(
                    &mut ws,
                    5,
                    json!({
                        "eventType": "CurrentProgramSceneChanged", "eventIntent": 4,
                        "eventData": { "sceneName": "Countdown" },
                    }),
                )
                .await;
                ok(json!({ "done": true }))
            }
            _ => ok(Value::Null),
        };
        send(&mut ws, 7, reply).await;
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

async fn wait_for(
    status: &mut watch::Receiver<ObsStatus>,
    what: impl Fn(&ObsStatus) -> bool,
) -> ObsStatus {
    tokio::time::timeout(Duration::from_secs(10), status.wait_for(what))
        .await
        .expect("status never arrived")
        .unwrap()
        .clone()
}

fn connected(s: &ObsStatus) -> bool {
    matches!(s, ObsStatus::Connected { .. })
}

#[tokio::test]
async fn connects_with_a_password_and_answers_requests() {
    let mock = MockObs::start(Some("supersecretpassword")).await;
    let obs = Obs::start(mock.config(Some("supersecretpassword")));
    let status = wait_for(&mut obs.status(), connected).await;
    assert_eq!(
        status,
        ObsStatus::Connected {
            obs_version: "30.2.3".into(),
            websocket_version: "5.5.2".into(),
        }
    );

    let scenes = obs.scene_list().await.unwrap();
    assert_eq!(scenes.program, "Worship");
    assert_eq!(scenes.preview, None);
    assert_eq!(scenes.scenes, ["Countdown", "Worship", "Pastor and slides"]);

    obs.set_program_scene("Pastor and slides").await.unwrap();
    assert_eq!(
        mock.requests_of("SetCurrentProgramScene"),
        [json!({ "sceneName": "Pastor and slides" })]
    );
    assert!(obs.virtual_cam_active().await.unwrap());
}

#[tokio::test]
async fn takes_a_screenshot_of_a_scene() {
    let mock = MockObs::start(None).await;
    let obs = Obs::start(mock.config(None));
    wait_for(&mut obs.status(), connected).await;
    assert_eq!(
        obs.screenshot("Worship", 480).await.unwrap(),
        "data:image/jpeg;base64,480"
    );
    // Out-of-range widths are clamped to what OBS accepts.
    assert_eq!(
        obs.screenshot("Worship", 0).await.unwrap(),
        "data:image/jpeg;base64,8"
    );
    assert_eq!(
        mock.requests_of("GetSourceScreenshot")[0],
        json!({
            "sourceName": "Worship", "imageFormat": "jpg",
            "imageWidth": 480, "imageCompressionQuality": 70,
        })
    );
    assert!(matches!(
        obs.screenshot("Nowhere", 480).await,
        Err(ObsError::Request {
            code: codes::RESOURCE_NOT_FOUND,
            ..
        })
    ));
}

#[tokio::test]
async fn a_failed_request_carries_obs_code() {
    let mock = MockObs::start(None).await;
    let obs = Obs::start(mock.config(None));
    wait_for(&mut obs.status(), connected).await;
    match obs.set_program_scene("Nowhere").await {
        Err(ObsError::Request { code, comment }) => {
            assert_eq!(code, codes::RESOURCE_NOT_FOUND);
            assert!(comment.unwrap().contains("Nowhere"));
        }
        other => panic!("expected a request error, got {other:?}"),
    }
}

#[tokio::test]
async fn a_wrong_password_stops_until_reconfigured() {
    let mock = MockObs::start(Some("right")).await;
    let obs = Obs::start(mock.config(Some("wrong")));
    wait_for(&mut obs.status(), |s| *s == ObsStatus::WrongPassword).await;

    // No retry with the same password.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(mock.connections.load(Ordering::SeqCst), 1);
    assert_eq!(obs.scene_list().await.unwrap_err(), ObsError::NotConnected);

    obs.reconfigure(mock.config(Some("right")));
    wait_for(&mut obs.status(), connected).await;
}

#[tokio::test]
async fn a_missing_password_is_reported() {
    let mock = MockObs::start(Some("right")).await;
    let obs = Obs::start(mock.config(None));
    wait_for(&mut obs.status(), |s| *s == ObsStatus::PasswordRequired).await;
}

#[tokio::test]
async fn a_dropped_connection_reconnects_and_fails_what_was_waiting() {
    let mock = MockObs::start(None).await;
    let obs = Obs::start(mock.config(None));
    let mut status = obs.status();
    wait_for(&mut status, connected).await;

    let waiting = {
        let obs = obs.clone();
        tokio::spawn(async move { obs.request("Hang", Value::Null).await })
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        while mock.requests_of("Hang").is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    mock.kick.send(()).unwrap();

    assert_eq!(waiting.await.unwrap(), Err(ObsError::Disconnected));
    wait_for(&mut status, |s| matches!(s, ObsStatus::Disconnected { .. })).await;
    wait_for(&mut status, connected).await;
    assert_eq!(mock.connections.load(Ordering::SeqCst), 2);
    obs.scene_list().await.unwrap();
}

#[tokio::test]
async fn an_event_can_arrive_while_a_request_waits() {
    let mock = MockObs::start(None).await;
    let obs = Obs::start(mock.config(None));
    let mut events = obs.events();
    wait_for(&mut obs.status(), connected).await;

    let answer = obs.request("Slow", Value::Null).await.unwrap();
    assert_eq!(answer, json!({ "done": true }));
    assert_eq!(
        events.recv().await.unwrap(),
        ObsEvent::ProgramSceneChanged {
            scene: "Countdown".into()
        }
    );
}

#[tokio::test]
async fn nothing_listening_is_disconnected_not_waiting() {
    // A port that was just free.
    let port = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let obs = Obs::start(ObsConfig {
        host: "127.0.0.1".into(),
        port,
        password: None,
    });
    let status = wait_for(&mut obs.status(), |s| {
        matches!(s, ObsStatus::Disconnected { .. })
    })
    .await;
    let ObsStatus::Disconnected { reason } = status else {
        unreachable!()
    };
    assert!(reason.contains("is OBS running"), "{reason}");
    assert_eq!(obs.start_stream().await, Err(ObsError::NotConnected));
}

#[tokio::test]
async fn sets_and_reads_the_stream_destination() {
    let mock = MockObs::start(None).await;
    let obs = Obs::start(mock.config(None));
    wait_for(&mut obs.status(), connected).await;

    assert_eq!(
        obs.stream_destination().await.unwrap(),
        StreamDestination::Service {
            service: "YouTube - RTMPS".into(),
            server: "auto".into(),
            key: "old-key".into(),
        }
    );
    let custom = StreamDestination::Custom {
        server: "rtmps://live.example.org/app".into(),
        key: "new-key".into(),
    };
    obs.set_stream_destination(&custom).await.unwrap();
    assert_eq!(obs.stream_destination().await.unwrap(), custom);
}
