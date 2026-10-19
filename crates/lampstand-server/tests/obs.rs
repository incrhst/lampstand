//! The OBS browser source routes (T43): keys and 404s, the page and its
//! fonts, the scene feed over WebSocket, blobs, and deck frames. The pixel
//! comparison with a real browser is `web/tests/obs.spec.ts`.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use lampstand_core::geom::{Color, Point, Transform};
use lampstand_core::id::BlobHash;
use lampstand_core::output::OutputId;
use lampstand_core::scene::*;
use lampstand_render::{BlobSource, RenderError, Renderer, RendererOptions};
use lampstand_server::{Backend, SceneHub, Server};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;

struct App {
    hub: SceneHub,
    blobs: tempfile::TempDir,
    renderer: Option<Arc<Renderer>>,
}

impl Backend for App {
    fn output_key_valid(&self, output: OutputId, key: &str) -> bool {
        self.hub.key_valid(output, key)
    }
    fn watch_scene(&self, output: OutputId) -> Option<Receiver<Arc<Scene>>> {
        self.hub.watch(output)
    }
    fn blob_path(&self, hash: &BlobHash) -> Option<PathBuf> {
        let path = self.blobs.path().join(hash.to_string());
        path.is_file().then_some(path)
    }
    fn renderer(&self) -> Option<Arc<Renderer>> {
        self.renderer.clone()
    }
}

struct NoBlobs;
impl BlobSource for NoBlobs {
    fn path(&self, _: &BlobHash) -> Option<PathBuf> {
        None
    }
}

struct Fixture {
    app: Arc<App>,
    server: Server,
    output: OutputId,
    key: String,
}

async fn fixture(renderer: Option<Arc<Renderer>>) -> Fixture {
    let app = Arc::new(App {
        hub: SceneHub::new(),
        blobs: tempfile::tempdir().unwrap(),
        renderer,
    });
    let output = OutputId::generate();
    let key = app.hub.add_output(output);
    let server = Server::start(SocketAddr::from(([127, 0, 0, 1], 0)), app.clone())
        .await
        .unwrap();
    Fixture {
        app,
        server,
        output,
        key,
    }
}

impl Fixture {
    fn port(&self) -> u16 {
        self.server.local_addr().port()
    }

    async fn get(&self, path: &str) -> (u16, String, Vec<u8>) {
        let mut stream = tokio::net::TcpStream::connect(self.server.local_addr())
            .await
            .unwrap();
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.unwrap();
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let head = String::from_utf8_lossy(&raw[..split]).to_string();
        let status = head.split(' ').nth(1).unwrap().parse().unwrap();
        let mut body = raw[split + 4..].to_vec();
        if head
            .to_ascii_lowercase()
            .contains("transfer-encoding: chunked")
        {
            body = dechunk(&body);
        }
        (status, head, body)
    }

    fn url(&self, path: &str) -> String {
        format!("{path}k={}", self.key)
    }

    async fn ws(&self, output: &str, key: &str) -> Result<Ws, ()> {
        let url = format!("ws://127.0.0.1:{}/obs/{output}/ws?k={key}", self.port());
        match tokio_tungstenite::connect_async(url).await {
            Ok((ws, _)) => Ok(ws),
            Err(_) => Err(()),
        }
    }
}

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

fn dechunk(mut body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let Some(end) = body.windows(2).position(|w| w == b"\r\n") else {
            return out;
        };
        let size =
            usize::from_str_radix(std::str::from_utf8(&body[..end]).unwrap().trim(), 16).unwrap();
        if size == 0 {
            return out;
        }
        out.extend_from_slice(&body[end + 2..end + 2 + size]);
        body = &body[end + 2 + size + 2..];
    }
}

async fn next_json(ws: &mut Ws) -> serde_json::Value {
    let message = tokio::time::timeout(Duration::from_secs(10), ws.next())
        .await
        .expect("a message within 10 s")
        .expect("open")
        .expect("no error");
    match message {
        Message::Text(text) => serde_json::from_str(text.as_str()).unwrap(),
        other => panic!("expected text, got {other:?}"),
    }
}

fn node(id: NodeId, kind: NodeKind) -> Node {
    Node {
        id,
        transform: Transform::IDENTITY,
        opacity: 1.0,
        visible: true,
        kind,
    }
}

fn square(id: NodeId) -> Node {
    let p = |x, y| Point { x, y };
    node(
        id,
        NodeKind::Path(PathNode {
            commands: vec![
                PathCommand::MoveTo(p(10.0, 10.0)),
                PathCommand::LineTo(p(90.0, 10.0)),
                PathCommand::LineTo(p(90.0, 90.0)),
                PathCommand::Close,
            ],
            fill: Some(Paint::Solid(Color::rgb(200, 30, 30))),
            stroke: None,
            shadow: None,
        }),
    )
}

fn scene(width: f32, nodes: Vec<Node>) -> Arc<Scene> {
    Arc::new(Scene {
        width,
        height: 100.0,
        nodes,
    })
}

/// A deck slide as the compositor wraps it: one group with a compositor id
/// around content nodes.
fn deck() -> Arc<Scene> {
    scene(
        200.0,
        vec![node(
            COMPOSITOR_NODE_BASE,
            NodeKind::Group(vec![square(1), square(2)]),
        )],
    )
}

#[tokio::test]
async fn every_route_needs_the_outputs_key() {
    let f = fixture(None).await;
    let out = f.output;
    let other = OutputId::generate();
    let hash = BlobHash::of(b"x");
    let paths = [
        format!("/obs/{out}"),
        format!("/blobs/{hash}?o={out}"),
        format!("/frames/{out}/1.webp"),
    ];
    for path in paths {
        let sep = if path.contains('?') { '&' } else { '?' };
        for bad in [
            "",
            &format!("{sep}k=nope"),
            &format!("{sep}k={}", f.app.hub.add_output(other)),
        ] {
            let (status, _, _) = f.get(&format!("{path}{bad}")).await;
            assert_eq!(status, 404, "{path}{bad}");
        }
    }
    let (status, _, _) = f.get(&format!("/obs/not-an-id?k={}", f.key)).await;
    assert_eq!(status, 404);
    let (status, _, _) = f.get(&format!("/obs/{other}?k={}", f.key)).await;
    assert_eq!(status, 404, "another output's page needs that output's key");

    assert!(f.ws(&out.to_string(), "nope").await.is_err());
    assert!(f.ws(&out.to_string(), "").await.is_err());
    assert!(f.ws(&other.to_string(), &f.key).await.is_err());
    assert!(f.ws(&out.to_string(), &f.key).await.is_ok());
}

#[tokio::test]
async fn the_page_is_transparent_and_loads_its_script_and_fonts() {
    let f = fixture(None).await;
    let (status, head, body) = f.get(&f.url(&format!("/obs/{}?", f.output))).await;
    assert_eq!(status, 200);
    let head = head.to_ascii_lowercase();
    assert!(head.contains("text/html"), "{head}");
    assert!(head.contains("no-referrer"), "{head}");
    let html = String::from_utf8(body).unwrap();
    assert!(html.contains("background: transparent"));
    assert!(html.contains("/obs/assets/main.js") && html.contains("/obs/assets/fonts.css"));

    // Everything the page imports is served, and nothing else is.
    for file in ["main.js", "render.js", "paint.js", "fonts.js", "scene.js"] {
        let (status, head, body) = f.get(&format!("/obs/assets/{file}")).await;
        assert_eq!(status, 200, "{file}");
        assert!(head.to_ascii_lowercase().contains("text/javascript"));
        assert!(!body.is_empty());
    }
    assert_eq!(f.get("/obs/assets/nope.js").await.0, 404);

    let (status, _, css) = f.get("/obs/assets/fonts.css").await;
    assert_eq!(status, 200);
    let css = String::from_utf8(css).unwrap();
    for family in [
        "Noto Sans",
        "Noto Serif",
        "Carlito",
        "Caladea",
        "Arimo",
        "Tinos",
        "Cousine",
    ] {
        assert!(
            css.contains(&format!("font-family:\"{family}\"")),
            "{family}"
        );
    }
    assert!(css.contains("font-weight:700;font-style:italic"));
    // Every file the CSS names is the bundled font, byte for byte.
    let files: Vec<&str> = css
        .split("/obs/assets/fonts/")
        .skip(1)
        .map(|s| s.split('"').next().unwrap())
        .collect();
    assert_eq!(files.len(), lampstand_layout::bundled_font_files().len());
    for file in files {
        let (status, head, bytes) = f.get(&format!("/obs/assets/fonts/{file}")).await;
        assert_eq!(status, 200, "{file}");
        assert!(head.to_ascii_lowercase().contains("font/ttf"));
        let (_, expected) = lampstand_layout::bundled_font_files()
            .iter()
            .find(|(name, _)| *name == file)
            .unwrap();
        assert!(bytes == *expected, "{file}");
    }
    assert_eq!(f.get("/obs/assets/fonts/Evil.ttf").await.0, 404);
    assert_eq!(f.get("/obs/assets/fonts/..%2Fmain.js").await.0, 404);
}

#[tokio::test]
async fn pages_follow_the_scene_feed() {
    let f = fixture(None).await;
    let out = f.output.to_string();

    // Nothing on air yet: the socket opens and stays quiet.
    let mut early = f.ws(&out, &f.key).await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(300), early.next())
            .await
            .is_err()
    );

    f.app.hub.publish(f.output, scene(100.0, vec![square(5)]));
    let first = next_json(&mut early).await;
    assert_eq!(first["seq"], 1);
    assert_eq!(first["scene"]["width"], 100.0);
    assert_eq!(first["scene"]["nodes"][0]["id"], 5);
    // The JSON is the scene as serde writes it, which is what the page reads.
    assert!(first["scene"]["nodes"][0]["kind"]["Path"]["commands"][0]["MoveTo"].is_object());
    assert_eq!(
        first["scene"]["nodes"][0]["kind"]["Path"]["commands"][3],
        "Close"
    );
    assert!(first["scene"]["nodes"][0]["kind"]["Path"]["fill"]["Solid"]["r"] == 200);

    // A page that connects later starts from the scene on air.
    let mut late = f.ws(&out, &f.key).await.unwrap();
    let joined = next_json(&mut late).await;
    assert_eq!(joined["scene"]["width"], 100.0);

    f.app.hub.publish(f.output, scene(150.0, Vec::new()));
    let mut seqs = Vec::new();
    for ws in [&mut early, &mut late] {
        let latest = next_json(ws).await;
        assert_eq!(latest["scene"]["width"], 150.0);
        seqs.push(latest["seq"].as_u64().unwrap());
    }
    // Both pages follow the same numbering.
    assert_eq!(seqs[0], seqs[1]);
    assert!(seqs[0] >= 2);
}

#[tokio::test]
async fn blobs_are_served_with_their_type() {
    let f = fixture(None).await;
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let hash = BlobHash::of(&png);
    std::fs::write(f.app.blobs.path().join(hash.to_string()), &png).unwrap();
    let out = f.output;
    let (status, head, body) = f.get(&f.url(&format!("/blobs/{hash}?o={out}&"))).await;
    assert_eq!(status, 200);
    assert!(head.to_ascii_lowercase().contains("image/png"));
    assert_eq!(body, png);

    let missing = BlobHash::of(b"missing");
    assert_eq!(
        f.get(&f.url(&format!("/blobs/{missing}?o={out}&"))).await.0,
        404
    );
    assert_eq!(f.get(&f.url("/blobs/not-a-hash?")).await.0, 404);
    // The key is for the output named in `o`.
    assert_eq!(f.get(&f.url(&format!("/blobs/{hash}?"))).await.0, 404);
}

#[tokio::test]
async fn without_a_renderer_a_deck_is_drawn_from_the_scene() {
    let f = fixture(None).await;
    f.app.hub.publish(f.output, deck());
    let mut ws = f.ws(&f.output.to_string(), &f.key).await.unwrap();
    let msg = next_json(&mut ws).await;
    assert!(msg["scene"].is_object() && msg.get("frame").is_none());
    assert_eq!(
        f.get(&f.url(&format!("/frames/{}/1.webp?", f.output)))
            .await
            .0,
        404
    );
}

#[tokio::test]
async fn deck_slides_come_as_webp_frames_from_render_offscreen() {
    let renderer = match Renderer::with_options(
        Arc::new(NoBlobs),
        RendererOptions {
            system_fonts: false,
            ..RendererOptions::default()
        },
    ) {
        Ok(r) => Arc::new(r),
        Err(RenderError::NoAdapter) => {
            eprintln!("SKIPPED: no GPU or software adapter on this machine");
            return;
        }
        Err(e) => panic!("renderer: {e:?}"),
    };
    let f = fixture(Some(renderer.clone())).await;
    let out = f.output.to_string();
    let mut ws = f.ws(&out, &f.key).await.unwrap();

    // Text stays a scene, even with a renderer.
    f.app.hub.publish(f.output, scene(100.0, vec![square(7)]));
    assert!(next_json(&mut ws).await["scene"].is_object());

    let slide = deck();
    f.app.hub.publish(f.output, slide.clone());
    let msg = next_json(&mut ws).await;
    assert!(msg.get("scene").is_none(), "{msg}");
    let (n, w, h) = (
        msg["frame"].as_u64().unwrap(),
        msg["width"].as_f64().unwrap(),
        msg["height"].as_f64().unwrap(),
    );
    assert_eq!((w, h), (200.0, 100.0));
    assert_eq!(n, msg["seq"].as_u64().unwrap());

    let (status, head, body) = f.get(&f.url(&format!("/frames/{out}/{n}.webp?"))).await;
    assert_eq!(status, 200);
    assert!(head.to_ascii_lowercase().contains("image/webp"));
    let frame = image::load_from_memory_with_format(&body, image::ImageFormat::WebP)
        .unwrap()
        .into_rgba8();
    let expected = renderer.render_offscreen(&slide, 200, 100).unwrap();
    assert_eq!(frame.dimensions(), (200, 100));
    // Lossless: the page shows exactly what the renderer drew, alpha included.
    assert!(
        frame.as_raw() == &expected.pixels,
        "the frame differs from render_offscreen"
    );
    assert!(
        frame.pixels().any(|p| p[3] == 0),
        "unpainted pixels stay transparent"
    );

    // Only the current scene has a frame.
    assert_eq!(
        f.get(&f.url(&format!("/frames/{out}/{}.webp?", n + 1)))
            .await
            .0,
        404
    );
    f.app.hub.publish(f.output, scene(100.0, Vec::new()));
    next_json(&mut ws).await;
    assert_eq!(
        f.get(&f.url(&format!("/frames/{out}/{n}.webp?"))).await.0,
        404
    );
    assert_eq!(
        f.get(&f.url(&format!("/frames/{out}/x.webp?"))).await.0,
        404
    );
}
