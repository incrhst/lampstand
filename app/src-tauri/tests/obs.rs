//! The OBS browser source wired into the app (T86): a stream output gets a
//! URL on the real local server, a program change reaches a WebSocket
//! client following it, and the URL's key survives a restart.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use lampstand_api::{LibraryRequest, OutputRequest, Request, Response, ServiceRequest};
use lampstand_app::config::device_key;
use lampstand_app::handler::{App, Placement, WindowHost};
use lampstand_app::sync::{Obs, Sync, SyncConfig};
use lampstand_core::output::{BackgroundMode, OutputConfig, OutputId, OutputRole, OutputTarget};
use lampstand_core::safe_area::SafeArea;
use lampstand_core::service::ItemKind;
use lampstand_layout::Fonts;
use lampstand_live::Command;
use lampstand_render::output::DisplayInfo;
use lampstand_render::{OutputCommand, OutputSink};
use tokio_tungstenite::tungstenite::Message;

struct Sink;
impl OutputSink for Sink {
    fn send(&self, _: OutputId, _: OutputCommand) {}
}

struct Host;
impl WindowHost for Host {
    fn displays(&self) -> Vec<DisplayInfo> {
        Vec::new()
    }
    fn open(&mut self, _: OutputId, _: &str, _: Placement) -> Result<(), String> {
        Ok(())
    }
    fn close(&mut self, _: OutputId) {}
}

fn open(dir: &Path) -> App {
    App::open(
        dir,
        Fonts::bundled_only(),
        Arc::new(Sink),
        Box::new(Host),
        Arc::new(|_| {}),
    )
    .unwrap()
}

/// Starts the local server over `app`'s scene hub, as the shell does.
async fn serve(app: &mut App, dir: &Path) -> Sync {
    let sync = Sync::start(
        app.library(),
        device_key(dir).unwrap(),
        dir,
        SyncConfig {
            name: "Test".into(),
            port: 0,
            mdns: false,
        },
        Obs {
            hub: app.scene_hub().clone(),
            renderer: None,
        },
        app.emitter(),
        std::sync::Arc::new(|_| {}),
        Default::default(),
    )
    .await
    .unwrap();
    app.set_server_address(format!("127.0.0.1:{}", sync.local_addr().port()));
    sync
}

fn ok(response: Response) -> Response {
    if let Response::Error { message } = &response {
        panic!("request failed: {message}");
    }
    response
}

fn stream_output() -> OutputConfig {
    OutputConfig {
        id: OutputId::generate(),
        name: "OBS".into(),
        role: OutputRole::Stream,
        target: OutputTarget::BrowserSource {
            width: 1280,
            height: 720,
        },
        safe_area: SafeArea::default(),
        background: BackgroundMode::Bleed,
        theme: None,
        idle: None,
        enabled: true,
    }
}

fn url(app: &mut App, output: OutputId) -> String {
    match ok(app.handle(Request::Outputs(OutputRequest::BrowserSourceUrl(output)))) {
        Response::Text(url) => url,
        other => panic!("{other:?}"),
    }
}

fn key(url: &str) -> &str {
    url.split_once("?k=").unwrap().1
}

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn follow(url: &str) -> Ws {
    let (page, query) = url.split_once('?').unwrap();
    let ws = format!("{}/ws?{query}", page.replace("http://", "ws://"));
    tokio_tungstenite::connect_async(ws).await.unwrap().0
}

/// The next scene message's text, or `None` if none comes within `wait`.
async fn next_message(ws: &mut Ws, wait: Duration) -> Option<String> {
    let message = tokio::time::timeout(wait, ws.next()).await.ok()?;
    match message.expect("open").expect("no error") {
        Message::Text(text) => Some(text.to_string()),
        other => panic!("expected text, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_program_change_reaches_the_browser_source() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = open(dir.path());
    let _sync = serve(&mut app, dir.path()).await;
    let output = stream_output();
    ok(app.handle(Request::Outputs(OutputRequest::Put(output.clone()))));
    let url = url(&mut app, output.id);
    assert!(
        url.starts_with(&format!(
            "http://127.0.0.1:{}/obs/",
            _sync.local_addr().port()
        )),
        "{url}"
    );

    let mut ws = follow(&url).await;
    // The current scene first: nothing on air yet.
    let first = next_message(&mut ws, Duration::from_secs(10))
        .await
        .unwrap();
    let first: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(first["scene"]["width"], 1280.0);
    assert!(!first.to_string().contains("Amazing grace"));

    let service = match ok(app.handle(Request::Service(ServiceRequest::Create {
        title: "Sunday".into(),
        starts_at: None,
        time_zone: None,
    }))) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    };
    let song = match ok(app.handle(Request::Library(LibraryRequest::SearchSongs {
        query: "amazing grace".into(),
        limit: 1,
    }))) {
        Response::Songs(hits) => hits[0].song,
        other => panic!("{other:?}"),
    };
    ok(app.handle(Request::Service(ServiceRequest::InsertItem {
        service: service.id,
        index: 0,
        kind: ItemKind::Song {
            song,
            arrangement: None,
        },
    })));
    ok(app.handle(Request::Service(ServiceRequest::Open(service.id))));
    ok(app.handle(Request::Live(Command::Next)));

    // Loading may publish too; the lyric arrives within a few messages.
    let mut seen = Vec::new();
    while let Some(text) = next_message(&mut ws, Duration::from_secs(10)).await {
        if text.contains("Amazing grace! How sweet the sound") {
            let message: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(message["scene"]["width"], 1280.0);
            assert_eq!(message["scene"]["height"], 720.0);
            return;
        }
        seen.push(text);
    }
    panic!("no scene with the first verse; got {seen:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_url_survives_a_restart_and_a_new_key_replaces_it() {
    let dir = tempfile::tempdir().unwrap();
    let output = stream_output();
    let before = {
        let mut app = open(dir.path());
        let _sync = serve(&mut app, dir.path()).await;
        ok(app.handle(Request::Outputs(OutputRequest::Put(output.clone()))));
        let url = url(&mut app, output.id);
        assert!(app.scene_hub().key_valid(output.id, key(&url)));
        key(&url).to_string()
    };

    let mut app = open(dir.path());
    let _sync = serve(&mut app, dir.path()).await;
    // Restored at start, before anyone asks for the URL.
    assert!(app.scene_hub().key_valid(output.id, &before));
    let after = url(&mut app, output.id);
    assert_eq!(key(&after), before);
    let mut ws = follow(&after).await;
    assert!(next_message(&mut ws, Duration::from_secs(10))
        .await
        .is_some());

    let rotated = match ok(
        app.handle(Request::Outputs(OutputRequest::RotateBrowserSourceKey(
            output.id,
        ))),
    ) {
        Response::Text(url) => url,
        other => panic!("{other:?}"),
    };
    assert_ne!(key(&rotated), before);
    assert!(!app.scene_hub().key_valid(output.id, &before));
    assert_eq!(url(&mut app, output.id), rotated);
    drop(app);
    let app = open(dir.path());
    assert!(app.scene_hub().key_valid(output.id, key(&rotated)));
}

#[tokio::test(flavor = "multi_thread")]
async fn turning_a_stream_output_off_takes_its_url_away_until_it_is_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = open(dir.path());
    let _sync = serve(&mut app, dir.path()).await;
    let mut output = stream_output();
    ok(app.handle(Request::Outputs(OutputRequest::Put(output.clone()))));
    let url = url(&mut app, output.id);
    let k = key(&url).to_string();

    output.enabled = false;
    ok(app.handle(Request::Outputs(OutputRequest::Put(output.clone()))));
    assert!(!app.scene_hub().key_valid(output.id, &k));
    output.enabled = true;
    ok(app.handle(Request::Outputs(OutputRequest::Put(output.clone()))));
    assert!(app.scene_hub().key_valid(output.id, &k));

    ok(app.handle(Request::Outputs(OutputRequest::Remove(output.id))));
    assert!(!app.scene_hub().key_valid(output.id, &k));
    assert!(matches!(
        app.handle(Request::Outputs(OutputRequest::BrowserSourceUrl(output.id))),
        Response::Error { .. }
    ));
}

fn clients(app: &mut App, output: OutputId) -> u32 {
    match ok(app.handle(Request::Outputs(OutputRequest::BrowserSourceStatus))) {
        Response::BrowserSources(all) => {
            all.into_iter()
                .find(|s| s.output == output)
                .unwrap_or_else(|| panic!("no status for {output:?}"))
                .clients
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_setup_screen_can_tell_whether_obs_is_showing_a_browser_source() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = open(dir.path());
    let _sync = serve(&mut app, dir.path()).await;
    let output = stream_output();
    ok(app.handle(Request::Outputs(OutputRequest::Put(output.clone()))));
    let url = url(&mut app, output.id);
    // The URL is made but no page has loaded it: waiting for OBS.
    assert_eq!(clients(&mut app, output.id), 0);

    let mut page = follow(&url).await;
    assert!(next_message(&mut page, Duration::from_secs(10))
        .await
        .is_some());
    assert_eq!(clients(&mut app, output.id), 1, "OBS is showing it");

    // OBS closes the source; the next look no longer counts it.
    page.close(None).await.ok();
    drop(page);
    let mut left = clients(&mut app, output.id);
    for _ in 0..50 {
        if left == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        left = clients(&mut app, output.id);
    }
    assert_eq!(left, 0, "a closed page is not counted");
}

/// Puts a service with one song live: the program then shows its first
/// verse.
fn go_live_with_a_song(app: &mut App) {
    let service = match ok(app.handle(Request::Service(ServiceRequest::Create {
        title: "Sunday".into(),
        starts_at: None,
        time_zone: None,
    }))) {
        Response::Service(s) => s,
        other => panic!("{other:?}"),
    };
    let song = match ok(app.handle(Request::Library(LibraryRequest::SearchSongs {
        query: "amazing grace".into(),
        limit: 1,
    }))) {
        Response::Songs(hits) => hits[0].song,
        other => panic!("{other:?}"),
    };
    ok(app.handle(Request::Service(ServiceRequest::InsertItem {
        service: service.id,
        index: 0,
        kind: ItemKind::Song {
            song,
            arrangement: None,
        },
    })));
    ok(app.handle(Request::Service(ServiceRequest::Open(service.id))));
    ok(app.handle(Request::Live(Command::Next)));
}

/// The next scene that shows the first verse, as JSON.
async fn next_verse_scene(ws: &mut Ws) -> serde_json::Value {
    let mut seen = Vec::new();
    while let Some(text) = next_message(ws, Duration::from_secs(10)).await {
        if text.contains("Amazing grace! How sweet the sound") {
            let message: serde_json::Value = serde_json::from_str(&text).unwrap();
            return message["scene"].clone();
        }
        seen.push(text);
    }
    panic!("no scene with the first verse; got {seen:?}");
}

/// Whether a scene is the stream's lower third: text only, all of it in
/// the bottom 30% of the frame.
fn is_lower_third(scene: &serde_json::Value) -> bool {
    let height = scene["height"].as_f64().unwrap();
    let nodes = scene["nodes"].as_array().unwrap();
    !nodes.is_empty()
        && nodes.iter().all(|n| {
            n["kind"]["Text"]["rect"]["y"]
                .as_f64()
                .is_some_and(|y| y >= height * 0.69)
        })
}

#[tokio::test(flavor = "multi_thread")]
async fn one_browser_source_switches_between_lower_thirds_and_full_screen() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = open(dir.path());
    let _sync = serve(&mut app, dir.path()).await;
    let mut output = stream_output();
    ok(app.handle(Request::Outputs(OutputRequest::Put(output.clone()))));
    let obs_url = url(&mut app, output.id);
    let mut ws = follow(&obs_url).await;
    go_live_with_a_song(&mut app);
    let lower = next_verse_scene(&mut ws).await;
    assert!(is_lower_third(&lower), "{lower}");

    // The same source, now full screen: the page OBS already has redraws.
    output.role = OutputRole::Audience { primary: false };
    ok(app.handle(Request::Outputs(OutputRequest::Put(output.clone()))));
    let full = next_verse_scene(&mut ws).await;
    assert!(!is_lower_third(&full), "{full}");
    assert_eq!(full["width"], 1280.0);
    assert_eq!(
        url(&mut app, output.id),
        obs_url,
        "the URL in OBS still works"
    );

    // And back.
    output.role = OutputRole::Stream;
    ok(app.handle(Request::Outputs(OutputRequest::Put(output))));
    assert!(is_lower_third(&next_verse_scene(&mut ws).await));
}

#[tokio::test(flavor = "multi_thread")]
async fn set_up_obs_gets_the_same_address_and_the_size_from_the_app() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = open(dir.path());
    let sync = serve(&mut app, dir.path()).await;
    let output = stream_output();
    ok(app.handle(Request::Outputs(OutputRequest::Put(output.clone()))));
    let shown = url(&mut app, output.id);

    let (picture, width, height) = app.obs_picture(output.id).unwrap();
    assert_eq!(picture, shown, "the same address and key as Setup shows");
    assert_eq!((width, height), (1280, 720));

    // The address follows the one the shell last gave the server.
    app.set_server_address("192.168.1.20:7340".into());
    assert!(app
        .obs_picture(output.id)
        .unwrap()
        .0
        .starts_with("http://192.168.1.20:7340/obs/"));

    // Not an OBS picture, or not an output at all.
    assert!(app.obs_picture(OutputId::generate()).is_err());
    drop(sync);
}
