//! OBS WebSocket client (T88): obs-websocket protocol v5, built into OBS 28
//! and later. Lampstand drives OBS from the service order instead of
//! replacing it (docs/decisions/0005-manage-obs-not-replace.md).
//!
//! - **One handle, one connection.** [`Obs::start`] spawns a task that
//!   connects, identifies and keeps reconnecting with backoff (0.5 s
//!   doubling to 10 s) for as long as any clone of the handle lives.
//!   [`Obs::status`] follows the connection; [`Obs::events`] carries what
//!   OBS reports (scene, stream, recording, virtual camera, mutes).
//! - **Never waits for OBS.** A request while OBS isn't connected fails at
//!   once with [`ObsError::NotConnected`], and an answer that doesn't come
//!   within 10 s is [`ObsError::Timeout`]. Projection carries on either way.
//! - **A wrong or missing password stops retrying** until
//!   [`Obs::reconfigure`] gives a new one: OBS would refuse it again.
//! - **Secrets.** The password and the stream key never appear in `Debug`
//!   output. Where they are stored is the app's job (T89).
//!
//! PRD: OB-4, OB-5, OB-6, OB-7, OB-8.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use futures_util::{SinkExt as _, StreamExt as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;

/// OBS's default WebSocket port (Tools › WebSocket Server Settings).
pub const DEFAULT_PORT: u16 = 4455;

const RPC_VERSION: u64 = 1;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const BACKOFF_START: Duration = Duration::from_millis(500);
const BACKOFF_MAX: Duration = Duration::from_secs(10);

/// Event categories: General, Config, Scenes, Inputs, Outputs. Leaves out
/// the high-volume ones (volume meters, active-state changes).
const EVENT_SUBSCRIPTIONS: u64 = (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 6);

/// WebSocket close code OBS sends when the password is wrong.
const CLOSE_AUTH_FAILED: u16 = 4009;
const CLOSE_UNSUPPORTED_RPC: u16 = 4010;

/// Where OBS is. Usually the same machine.
#[derive(Clone, PartialEq, Eq)]
pub struct ObsConfig {
    pub host: String,
    pub port: u16,
    /// `None` when OBS's "Enable Authentication" is off.
    pub password: Option<String>,
}

impl Default for ObsConfig {
    fn default() -> Self {
        Self {
            host: "localhost".into(),
            port: DEFAULT_PORT,
            password: None,
        }
    }
}

impl fmt::Debug for ObsConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObsConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("password", &self.password.as_ref().map(|_| "…"))
            .finish()
    }
}

impl ObsConfig {
    fn url(&self) -> String {
        if self.host.contains(':') && !self.host.starts_with('[') {
            format!("ws://[{}]:{}", self.host, self.port)
        } else {
            format!("ws://{}:{}", self.host, self.port)
        }
    }
}

/// The connection, for the status pill in Present (T89).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ObsStatus {
    Connecting,
    Connected {
        obs_version: String,
        websocket_version: String,
    },
    /// Not reachable, or the connection dropped; retrying. `reason` is for
    /// people: "nothing is listening on localhost:4455".
    Disconnected {
        reason: String,
    },
    /// OBS asks for a password and none is set. Not retried.
    PasswordRequired,
    /// OBS refused the password. Not retried.
    WrongPassword,
}

/// Something OBS reported. Events Lampstand has no use for yet arrive as
/// [`ObsEvent::Other`].
#[derive(Debug, Clone, PartialEq)]
pub enum ObsEvent {
    ProgramSceneChanged {
        scene: String,
    },
    /// Scenes were added, removed, renamed or reordered.
    SceneListChanged,
    SceneCollectionChanged {
        collection: String,
    },
    StreamStateChanged {
        active: bool,
        state: String,
    },
    RecordStateChanged {
        active: bool,
        state: String,
    },
    VirtualCamStateChanged {
        active: bool,
    },
    InputMuteChanged {
        input: String,
        muted: bool,
    },
    /// OBS is closing; a `Disconnected` status follows.
    Exiting,
    Other {
        event_type: String,
        data: Value,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObsError {
    NotConnected,
    /// The connection dropped before OBS answered.
    Disconnected,
    Timeout,
    /// OBS answered with a failure, such as 600 for a scene that doesn't
    /// exist or 500 for starting a stream that is already running.
    Request {
        code: u16,
        comment: Option<String>,
    },
    /// OBS answered with something this client doesn't understand.
    Protocol(String),
}

impl fmt::Display for ObsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConnected => f.write_str("OBS is not connected"),
            Self::Disconnected => f.write_str("OBS disconnected before answering"),
            Self::Timeout => f.write_str("OBS did not answer in time"),
            Self::Request { code, comment } => match comment {
                Some(c) => write!(f, "OBS refused the request ({code}): {c}"),
                None => write!(f, "OBS refused the request ({code})"),
            },
            Self::Protocol(m) => write!(f, "unexpected message from OBS: {m}"),
        }
    }
}

impl std::error::Error for ObsError {}

/// Request status codes Lampstand checks for.
pub mod codes {
    pub const OUTPUT_RUNNING: u16 = 500;
    pub const OUTPUT_NOT_RUNNING: u16 = 501;
    pub const RESOURCE_NOT_FOUND: u16 = 600;
    pub const RESOURCE_ALREADY_EXISTS: u16 = 601;
}

/// OBS's scenes, top of OBS's list first, as OBS shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneList {
    pub program: String,
    /// Set only in Studio Mode.
    pub preview: Option<String>,
    pub scenes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamStatus {
    #[serde(rename = "outputActive")]
    pub active: bool,
    #[serde(rename = "outputReconnecting")]
    pub reconnecting: bool,
    /// "HH:MM:SS.mmm".
    #[serde(rename = "outputTimecode")]
    pub timecode: String,
    #[serde(rename = "outputSkippedFrames")]
    pub skipped_frames: u64,
    #[serde(rename = "outputTotalFrames")]
    pub total_frames: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RecordStatus {
    #[serde(rename = "outputActive")]
    pub active: bool,
    #[serde(rename = "outputPaused")]
    pub paused: bool,
    #[serde(rename = "outputTimecode")]
    pub timecode: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Input {
    #[serde(rename = "inputName")]
    pub name: String,
    /// For example `wasapi_input_capture`, `coreaudio_input_capture`,
    /// `browser_source`.
    #[serde(rename = "inputKind")]
    pub kind: String,
}

impl Input {
    /// A microphone, line in or desktop/app audio capture: what OBS's
    /// mixer is mostly for and what an operator mutes. Video, screen and
    /// browser sources can carry sound too (OBS lets you mute those), but
    /// listing every camera as a mute button buries the microphones.
    pub fn is_audio_device(&self) -> bool {
        let kind = self.kind.as_str();
        kind.ends_with("_input_capture")
            || kind.ends_with("_output_capture")
            || matches!(kind, "sck_audio_capture" | "jack_output_client")
    }
}

/// Where OBS sends the stream (Settings › Stream).
#[derive(Clone, PartialEq, Eq)]
pub enum StreamDestination {
    /// One of OBS's listed services. `service` is OBS's name for it, such
    /// as "YouTube - RTMPS" or "Facebook Live"; `server` is often "auto".
    Service {
        service: String,
        server: String,
        key: String,
    },
    /// A custom RTMP or RTMPS server.
    Custom { server: String, key: String },
    /// Something else (WHIP, a plugin's type). Read-only here.
    Other { kind: String },
}

impl fmt::Debug for StreamDestination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Service {
                service, server, ..
            } => f
                .debug_struct("Service")
                .field("service", service)
                .field("server", server)
                .field("key", &"…")
                .finish(),
            Self::Custom { server, .. } => f
                .debug_struct("Custom")
                .field("server", server)
                .field("key", &"…")
                .finish(),
            Self::Other { kind } => f.debug_struct("Other").field("kind", kind).finish(),
        }
    }
}

/// A handle on the OBS connection. Cheap to clone; the connection closes
/// when the last clone is dropped.
#[derive(Clone)]
pub struct Obs {
    inner: Arc<Inner>,
}

struct Inner {
    commands: mpsc::Sender<Command>,
    config: watch::Sender<ObsConfig>,
    status: watch::Receiver<ObsStatus>,
    events: broadcast::Sender<ObsEvent>,
}

enum Command {
    Request {
        request_type: String,
        data: Value,
        reply: oneshot::Sender<Result<Value, ObsError>>,
    },
}

impl Obs {
    /// Starts connecting to OBS. Needs a Tokio runtime.
    pub fn start(config: ObsConfig) -> Self {
        let (commands, command_rx) = mpsc::channel(64);
        let (config_tx, config_rx) = watch::channel(config);
        let (status_tx, status) = watch::channel(ObsStatus::Connecting);
        let (events, _) = broadcast::channel(256);
        tokio::spawn(supervise(config_rx, command_rx, status_tx, events.clone()));
        Self {
            inner: Arc::new(Inner {
                commands,
                config: config_tx,
                status,
                events,
            }),
        }
    }

    /// Changes where OBS is or the password, and reconnects at once.
    pub fn reconfigure(&self, config: ObsConfig) {
        self.inner.config.send_replace(config);
    }

    pub fn status(&self) -> watch::Receiver<ObsStatus> {
        self.inner.status.clone()
    }

    pub fn events(&self) -> broadcast::Receiver<ObsEvent> {
        self.inner.events.subscribe()
    }

    /// Sends any obs-websocket request and returns its `responseData`
    /// (`null` when there is none). The typed calls below cover what the
    /// app uses.
    pub async fn request(&self, request_type: &str, data: Value) -> Result<Value, ObsError> {
        if !matches!(*self.inner.status.borrow(), ObsStatus::Connected { .. }) {
            return Err(ObsError::NotConnected);
        }
        let (reply, answer) = oneshot::channel();
        self.inner
            .commands
            .send(Command::Request {
                request_type: request_type.to_string(),
                data,
                reply,
            })
            .await
            .map_err(|_| ObsError::NotConnected)?;
        match tokio::time::timeout(REQUEST_TIMEOUT, answer).await {
            Err(_) => Err(ObsError::Timeout),
            Ok(Err(_)) => Err(ObsError::Disconnected),
            Ok(Ok(result)) => result,
        }
    }

    async fn call<T: for<'de> Deserialize<'de>>(
        &self,
        request_type: &str,
        data: Value,
    ) -> Result<T, ObsError> {
        let value = self.request(request_type, data).await?;
        serde_json::from_value(value)
            .map_err(|e| ObsError::Protocol(format!("{request_type}: {e}")))
    }

    // Scenes (OB-4, OB-8).

    pub async fn scene_list(&self) -> Result<SceneList, ObsError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            current_program_scene_name: String,
            current_preview_scene_name: Option<String>,
            scenes: Vec<RawScene>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawScene {
            scene_name: String,
            scene_index: i64,
        }
        let mut raw: Raw = self.call("GetSceneList", Value::Null).await?;
        // OBS numbers scenes from the bottom of its list.
        raw.scenes.sort_by_key(|s| std::cmp::Reverse(s.scene_index));
        Ok(SceneList {
            program: raw.current_program_scene_name,
            preview: raw.current_preview_scene_name,
            scenes: raw.scenes.into_iter().map(|s| s.scene_name).collect(),
        })
    }

    pub async fn set_program_scene(&self, scene: &str) -> Result<(), ObsError> {
        self.request("SetCurrentProgramScene", json!({ "sceneName": scene }))
            .await
            .map(drop)
    }

    /// A JPEG of what a scene (or any source) shows now, `width` pixels
    /// wide with OBS keeping the aspect ratio, as a `data:` URL for an
    /// `<img>`. OBS renders a scene that isn't on program too, so this
    /// previews a scene before switching to it (OB-8).
    pub async fn screenshot(&self, source: &str, width: u32) -> Result<String, ObsError> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(rename = "imageData")]
            image: String,
        }
        let raw: Raw = self
            .call(
                "GetSourceScreenshot",
                json!({
                    "sourceName": source,
                    "imageFormat": "jpg",
                    // OBS refuses widths under 8 or over 4096.
                    "imageWidth": width.clamp(8, 4096),
                    "imageCompressionQuality": 70,
                }),
            )
            .await?;
        // OBS labels it "image/jpg", which isn't a registered type.
        Ok(match raw.image.strip_prefix("data:image/jpg;") {
            Some(rest) => format!("data:image/jpeg;{rest}"),
            None => raw.image,
        })
    }

    // Streaming, recording and the virtual camera (OB-5, OB-6).

    pub async fn stream_status(&self) -> Result<StreamStatus, ObsError> {
        self.call("GetStreamStatus", Value::Null).await
    }

    pub async fn start_stream(&self) -> Result<(), ObsError> {
        self.request("StartStream", Value::Null).await.map(drop)
    }

    pub async fn stop_stream(&self) -> Result<(), ObsError> {
        self.request("StopStream", Value::Null).await.map(drop)
    }

    pub async fn record_status(&self) -> Result<RecordStatus, ObsError> {
        self.call("GetRecordStatus", Value::Null).await
    }

    pub async fn start_record(&self) -> Result<(), ObsError> {
        self.request("StartRecord", Value::Null).await.map(drop)
    }

    pub async fn stop_record(&self) -> Result<(), ObsError> {
        self.request("StopRecord", Value::Null).await.map(drop)
    }

    pub async fn virtual_cam_active(&self) -> Result<bool, ObsError> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(rename = "outputActive")]
            active: bool,
        }
        let raw: Raw = self.call("GetVirtualCamStatus", Value::Null).await?;
        Ok(raw.active)
    }

    pub async fn start_virtual_cam(&self) -> Result<(), ObsError> {
        self.request("StartVirtualCam", Value::Null).await.map(drop)
    }

    pub async fn stop_virtual_cam(&self) -> Result<(), ObsError> {
        self.request("StopVirtualCam", Value::Null).await.map(drop)
    }

    /// Where OBS streams to. Holds the stream key: don't log it.
    pub async fn stream_destination(&self) -> Result<StreamDestination, ObsError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            stream_service_type: String,
            stream_service_settings: Settings,
        }
        #[derive(Deserialize)]
        struct Settings {
            #[serde(default)]
            service: String,
            #[serde(default)]
            server: String,
            #[serde(default)]
            key: String,
        }
        let raw: Raw = self.call("GetStreamServiceSettings", Value::Null).await?;
        let s = raw.stream_service_settings;
        Ok(match raw.stream_service_type.as_str() {
            "rtmp_common" => StreamDestination::Service {
                service: s.service,
                server: s.server,
                key: s.key,
            },
            "rtmp_custom" => StreamDestination::Custom {
                server: s.server,
                key: s.key,
            },
            _ => StreamDestination::Other {
                kind: raw.stream_service_type,
            },
        })
    }

    /// Sets where OBS streams to. OBS refuses while it is streaming.
    pub async fn set_stream_destination(
        &self,
        destination: &StreamDestination,
    ) -> Result<(), ObsError> {
        let (kind, settings) = match destination {
            StreamDestination::Service {
                service,
                server,
                key,
            } => (
                "rtmp_common",
                json!({ "service": service, "server": server, "key": key }),
            ),
            StreamDestination::Custom { server, key } => {
                ("rtmp_custom", json!({ "server": server, "key": key }))
            }
            StreamDestination::Other { kind } => {
                return Err(ObsError::Protocol(format!(
                    "can't set a {kind} destination from Lampstand"
                )))
            }
        };
        self.request(
            "SetStreamServiceSettings",
            json!({ "streamServiceType": kind, "streamServiceSettings": settings }),
        )
        .await
        .map(drop)
    }

    // Inputs and audio mutes (OB-8).

    pub async fn inputs(&self) -> Result<Vec<Input>, ObsError> {
        #[derive(Deserialize)]
        struct Raw {
            inputs: Vec<Input>,
        }
        let raw: Raw = self.call("GetInputList", Value::Null).await?;
        Ok(raw.inputs)
    }

    /// Fails with a request error for an input that has no audio.
    pub async fn input_muted(&self, input: &str) -> Result<bool, ObsError> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(rename = "inputMuted")]
            muted: bool,
        }
        let raw: Raw = self
            .call("GetInputMute", json!({ "inputName": input }))
            .await?;
        Ok(raw.muted)
    }

    pub async fn set_input_muted(&self, input: &str, muted: bool) -> Result<(), ObsError> {
        self.request(
            "SetInputMute",
            json!({ "inputName": input, "inputMuted": muted }),
        )
        .await
        .map(drop)
    }

    // Setup (OB-7).

    pub async fn scene_collections(&self) -> Result<(String, Vec<String>), ObsError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            current_scene_collection_name: String,
            scene_collections: Vec<String>,
        }
        let raw: Raw = self.call("GetSceneCollectionList", Value::Null).await?;
        Ok((raw.current_scene_collection_name, raw.scene_collections))
    }

    /// Creates a scene collection and switches OBS to it.
    pub async fn create_scene_collection(&self, name: &str) -> Result<(), ObsError> {
        self.request(
            "CreateSceneCollection",
            json!({ "sceneCollectionName": name }),
        )
        .await
        .map(drop)
    }

    pub async fn set_scene_collection(&self, name: &str) -> Result<(), ObsError> {
        self.request(
            "SetCurrentSceneCollection",
            json!({ "sceneCollectionName": name }),
        )
        .await
        .map(drop)
    }

    pub async fn create_scene(&self, name: &str) -> Result<(), ObsError> {
        self.request("CreateScene", json!({ "sceneName": name }))
            .await
            .map(drop)
    }

    /// Adds a browser source to `scene`, or, if an input of that name
    /// already exists, adds it to `scene` too (OBS shares inputs between
    /// scenes by name).
    pub async fn add_browser_source(
        &self,
        scene: &str,
        name: &str,
        url: &str,
        width: u32,
        height: u32,
    ) -> Result<(), ObsError> {
        let created = self
            .request(
                "CreateInput",
                json!({
                    "sceneName": scene,
                    "inputName": name,
                    "inputKind": "browser_source",
                    "inputSettings": browser_settings(url, width, height),
                }),
            )
            .await;
        match created {
            Err(ObsError::Request { code, .. }) if code == codes::RESOURCE_ALREADY_EXISTS => self
                .request(
                    "CreateSceneItem",
                    json!({ "sceneName": scene, "sourceName": name }),
                )
                .await
                .map(drop),
            other => other.map(drop),
        }
    }

    /// Points an existing browser source at a new URL or size (a rotated
    /// key, a resized stream output).
    pub async fn update_browser_source(
        &self,
        name: &str,
        url: &str,
        width: u32,
        height: u32,
    ) -> Result<(), ObsError> {
        self.request(
            "SetInputSettings",
            json!({
                "inputName": name,
                "inputSettings": browser_settings(url, width, height),
                "overlay": true,
            }),
        )
        .await
        .map(drop)
    }
}

fn browser_settings(url: &str, width: u32, height: u32) -> Value {
    json!({
        "url": url,
        "width": width,
        "height": height,
        // Keep the page running when the scene isn't live, so switching to
        // it shows the current slide at once.
        "shutdown": false,
        "restart_when_active": false,
    })
}

/// The `authentication` string for `Identify`:
/// base64(sha256(base64(sha256(password + salt)) + challenge)).
pub fn auth_response(password: &str, salt: &str, challenge: &str) -> String {
    let b64 = base64::engine::general_purpose::STANDARD;
    let secret = b64.encode(Sha256::digest(format!("{password}{salt}")));
    b64.encode(Sha256::digest(format!("{secret}{challenge}")))
}

// The connection task.

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

enum End {
    /// Every handle is gone.
    Exit,
    /// New settings; connect again now.
    Reconfigured,
    /// Retry after a backoff. `identified` resets the backoff.
    Lost { reason: String, identified: bool },
    /// Retry only after new settings.
    Refused(ObsStatus),
}

async fn supervise(
    mut config: watch::Receiver<ObsConfig>,
    mut commands: mpsc::Receiver<Command>,
    status: watch::Sender<ObsStatus>,
    events: broadcast::Sender<ObsEvent>,
) {
    let mut backoff = BACKOFF_START;
    loop {
        let current = config.borrow_and_update().clone();
        status.send_replace(ObsStatus::Connecting);
        let end = tokio::select! {
            end = connect(&current, &mut commands, &status, &events) => end,
            changed = config.changed() => match changed {
                Ok(()) => End::Reconfigured,
                Err(_) => End::Exit,
            },
        };
        let retry_after = match end {
            End::Exit => return,
            End::Reconfigured => {
                backoff = BACKOFF_START;
                continue;
            }
            End::Lost { reason, identified } => {
                status.send_replace(ObsStatus::Disconnected { reason });
                if identified {
                    backoff = BACKOFF_START;
                }
                let wait = backoff;
                backoff = (backoff * 2).min(BACKOFF_MAX);
                Some(wait)
            }
            End::Refused(refused) => {
                status.send_replace(refused);
                None
            }
        };
        let sleep = async {
            match retry_after {
                Some(wait) => tokio::time::sleep(wait).await,
                None => std::future::pending().await,
            }
        };
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                _ = &mut sleep => break,
                changed = config.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    backoff = BACKOFF_START;
                    break;
                }
                command = commands.recv() => match command {
                    None => return,
                    Some(Command::Request { reply, .. }) => {
                        let _ = reply.send(Err(ObsError::NotConnected));
                    }
                },
            }
        }
    }
}

async fn connect(
    config: &ObsConfig,
    commands: &mut mpsc::Receiver<Command>,
    status: &watch::Sender<ObsStatus>,
    events: &broadcast::Sender<ObsEvent>,
) -> End {
    let address = format!("{}:{}", config.host, config.port);
    let ws = match tokio::time::timeout(
        CONNECT_TIMEOUT,
        tokio_tungstenite::connect_async(config.url()),
    )
    .await
    {
        Err(_) => {
            return lost(format!("OBS at {address} did not answer"), false);
        }
        Ok(Err(e)) => {
            return lost(
                format!("nothing is listening on {address} ({e}); is OBS running with its WebSocket server on?"),
                false,
            );
        }
        Ok(Ok((ws, _))) => ws,
    };
    let mut ws = ws;
    let (obs_version, websocket_version) =
        match tokio::time::timeout(HANDSHAKE_TIMEOUT, identify(&mut ws, config)).await {
            Err(_) => return lost("OBS did not finish the handshake".into(), false),
            Ok(Err(end)) => return end,
            Ok(Ok(versions)) => versions,
        };
    status.send_replace(ObsStatus::Connected {
        obs_version,
        websocket_version,
    });
    run(ws, commands, events).await
}

fn lost(reason: String, identified: bool) -> End {
    End::Lost { reason, identified }
}

/// `Hello` → `Identify` → `Identified`, then `GetVersion` for the status.
async fn identify(ws: &mut Ws, config: &ObsConfig) -> Result<(String, String), End> {
    let hello = next_op(ws, 0).await?;
    let websocket_version = hello["obsWebSocketVersion"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    if hello["rpcVersion"]
        .as_u64()
        .is_some_and(|v| v < RPC_VERSION)
    {
        return Err(End::Refused(ObsStatus::Disconnected {
            reason: format!("OBS's WebSocket server ({websocket_version}) is too old; Lampstand needs OBS 28 or later"),
        }));
    }
    let mut identify = json!({
        "rpcVersion": RPC_VERSION,
        "eventSubscriptions": EVENT_SUBSCRIPTIONS,
    });
    if let Some(auth) = hello.get("authentication") {
        let Some(password) = config.password.as_deref() else {
            let _ = ws.close(None).await;
            return Err(End::Refused(ObsStatus::PasswordRequired));
        };
        let salt = auth["salt"].as_str().unwrap_or_default();
        let challenge = auth["challenge"].as_str().unwrap_or_default();
        identify["authentication"] = auth_response(password, salt, challenge).into();
    }
    send(ws, 1, identify).await?;
    next_op(ws, 2).await?;

    send(
        ws,
        6,
        json!({ "requestType": "GetVersion", "requestId": "lampstand-version" }),
    )
    .await?;
    let version = next_op(ws, 7).await?;
    let obs_version = version["responseData"]["obsVersion"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    Ok((obs_version, websocket_version))
}

async fn send(ws: &mut Ws, op: u8, d: Value) -> Result<(), End> {
    let text = json!({ "op": op, "d": d }).to_string();
    ws.send(Message::text(text))
        .await
        .map_err(|e| lost(format!("lost OBS: {e}"), false))
}

/// The next message, which must have opcode `op`; returns its `d`.
async fn next_op(ws: &mut Ws, op: u64) -> Result<Value, End> {
    loop {
        let message = match ws.next().await {
            None => return Err(lost("OBS closed the connection".into(), false)),
            Some(Err(e)) => return Err(lost(format!("lost OBS: {e}"), false)),
            Some(Ok(m)) => m,
        };
        match message {
            Message::Text(text) => {
                let v: Value = serde_json::from_str(&text)
                    .map_err(|e| lost(format!("OBS sent bad JSON: {e}"), false))?;
                if v["op"].as_u64() == Some(op) {
                    return Ok(v["d"].clone());
                }
            }
            Message::Close(frame) => return Err(closed(frame.as_ref(), false)),
            _ => {}
        }
    }
}

fn closed(frame: Option<&CloseFrame>, identified: bool) -> End {
    let Some(frame) = frame else {
        return lost("OBS closed the connection".into(), identified);
    };
    match u16::from(frame.code) {
        CLOSE_AUTH_FAILED => End::Refused(ObsStatus::WrongPassword),
        CLOSE_UNSUPPORTED_RPC => End::Refused(ObsStatus::Disconnected {
            reason: "OBS's WebSocket server doesn't speak protocol v5".into(),
        }),
        _ if frame.code == CloseCode::Away => lost("OBS is closing".into(), identified),
        code => lost(
            format!("OBS closed the connection ({code}: {})", frame.reason),
            identified,
        ),
    }
}

/// The identified session: requests out, answers and events in.
async fn run(
    ws: Ws,
    commands: &mut mpsc::Receiver<Command>,
    events: &broadcast::Sender<ObsEvent>,
) -> End {
    let (mut sink, mut stream) = ws.split();
    let mut pending: HashMap<String, oneshot::Sender<Result<Value, ObsError>>> = HashMap::new();
    let mut next_id: u64 = 0;
    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(Command::Request { request_type, data, reply }) = command else {
                    let _ = sink.close().await;
                    return End::Exit;
                };
                next_id += 1;
                let id = next_id.to_string();
                let mut d = json!({ "requestType": request_type, "requestId": id });
                if !data.is_null() {
                    d["requestData"] = data;
                }
                let text = json!({ "op": 6, "d": d }).to_string();
                if let Err(e) = sink.send(Message::text(text)).await {
                    let _ = reply.send(Err(ObsError::Disconnected));
                    return lost(format!("lost OBS: {e}"), true);
                }
                pending.retain(|_, waiting| !waiting.is_closed());
                pending.insert(id, reply);
            }
            message = stream.next() => {
                let message = match message {
                    None => return lost("OBS closed the connection".into(), true),
                    Some(Err(e)) => return lost(format!("lost OBS: {e}"), true),
                    Some(Ok(m)) => m,
                };
                let text = match message {
                    Message::Text(text) => text,
                    Message::Close(frame) => return closed(frame.as_ref(), true),
                    _ => continue,
                };
                let Ok(v) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                let d = &v["d"];
                match v["op"].as_u64() {
                    Some(5) => {
                        let _ = events.send(event(d));
                    }
                    Some(7) => {
                        if let Some(reply) = d["requestId"].as_str().and_then(|id| pending.remove(id)) {
                            let _ = reply.send(response(d));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn response(d: &Value) -> Result<Value, ObsError> {
    let status = &d["requestStatus"];
    match status["result"].as_bool() {
        Some(true) => Ok(d.get("responseData").cloned().unwrap_or(Value::Null)),
        Some(false) => Err(ObsError::Request {
            code: status["code"].as_u64().unwrap_or(0) as u16,
            comment: status["comment"].as_str().map(str::to_string),
        }),
        None => Err(ObsError::Protocol(
            "response without a requestStatus".into(),
        )),
    }
}

fn event(d: &Value) -> ObsEvent {
    let event_type = d["eventType"].as_str().unwrap_or_default();
    let data = &d["eventData"];
    let text = |key: &str| data[key].as_str().unwrap_or_default().to_string();
    let flag = |key: &str| data[key].as_bool().unwrap_or(false);
    match event_type {
        "CurrentProgramSceneChanged" => ObsEvent::ProgramSceneChanged {
            scene: text("sceneName"),
        },
        "SceneListChanged" | "SceneCreated" | "SceneRemoved" | "SceneNameChanged" => {
            ObsEvent::SceneListChanged
        }
        "CurrentSceneCollectionChanged" => ObsEvent::SceneCollectionChanged {
            collection: text("sceneCollectionName"),
        },
        "StreamStateChanged" => ObsEvent::StreamStateChanged {
            active: flag("outputActive"),
            state: text("outputState"),
        },
        "RecordStateChanged" => ObsEvent::RecordStateChanged {
            active: flag("outputActive"),
            state: text("outputState"),
        },
        "VirtualcamStateChanged" => ObsEvent::VirtualCamStateChanged {
            active: flag("outputActive"),
        },
        "InputMuteStateChanged" => ObsEvent::InputMuteChanged {
            input: text("inputName"),
            muted: flag("inputMuted"),
        },
        "ExitStarted" => ObsEvent::Exiting,
        _ => ObsEvent::Other {
            event_type: event_type.to_string(),
            data: data.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_devices_are_mics_and_desktop_audio() {
        let input = |kind: &str| Input {
            name: "x".into(),
            kind: kind.into(),
        };
        for kind in [
            "coreaudio_input_capture",
            "coreaudio_output_capture",
            "sck_audio_capture",
            "wasapi_input_capture",
            "wasapi_output_capture",
            "wasapi_process_output_capture",
            "pulse_input_capture",
            "pulse_output_capture",
            "alsa_input_capture",
            "jack_output_client",
        ] {
            assert!(input(kind).is_audio_device(), "{kind}");
        }
        for kind in [
            "screen_capture",
            "av_capture_input_v2",
            "macos-avcapture",
            "dshow_input",
            "browser_source",
            "ffmpeg_source",
            "ndi_source",
        ] {
            assert!(!input(kind).is_audio_device(), "{kind}");
        }
    }

    /// The worked example in the obs-websocket protocol docs.
    #[test]
    fn auth_matches_protocol_example() {
        assert_eq!(
            auth_response(
                "supersecretpassword",
                "lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI=",
                "+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY=",
            ),
            "1Ct943GAT+6YQUUX47Ia/ncufilbe6+oD6lY+5kaCu4="
        );
    }

    #[test]
    fn secrets_stay_out_of_debug() {
        let config = ObsConfig {
            password: Some("hunter2".into()),
            ..ObsConfig::default()
        };
        assert!(!format!("{config:?}").contains("hunter2"));
        let destination = StreamDestination::Custom {
            server: "rtmp://a".into(),
            key: "live_123".into(),
        };
        assert!(!format!("{destination:?}").contains("live_123"));
    }

    #[test]
    fn ipv6_hosts_are_bracketed() {
        let config = ObsConfig {
            host: "::1".into(),
            ..ObsConfig::default()
        };
        assert_eq!(config.url(), "ws://[::1]:4455");
    }
}
