//! A stand-in for lampstand-server, for testing the output process: the
//! WebSocket at `/ws` and PNGs under `/media/`, on one port.
//!
//! It serves whatever [`FakeServer::set`] last put up, records every client
//! message and HTTP path, and can drop all connections to test reconnects.
//! Used by `tests/fake_server.rs` and `examples/fake_server.rs`.

#![allow(dead_code)]

use std::collections::HashMap;
use std::io::Cursor;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use futures_util::{SinkExt, StreamExt};
use image::{ImageFormat, Rgba, RgbaImage};
use lampstand_show::protocol::{ClientMessage, CueView, Mode, ServerMessage, Snapshot};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Message;

pub const SLIDE_URL: &str = "/media/deck/demo/0/0.png";
pub const SLIDE2_URL: &str = "/media/deck/demo/1/0.png";
pub const LOGO_URL: &str = "/media/logo.png";

type PngCache = HashMap<(u32, u32), Arc<Vec<u8>>>;

#[derive(Clone)]
pub struct FakeServer {
    pub addr: SocketAddr,
    state: watch::Sender<(u64, Option<Snapshot>)>,
    /// Client → server messages, in order.
    pub received: Arc<Mutex<Vec<ClientMessage>>>,
    /// Paths with query of every HTTP request.
    pub requests: Arc<Mutex<Vec<String>>>,
    /// Bumping this drops every WebSocket.
    kick: watch::Sender<u64>,
    slide: Option<Arc<Vec<u8>>>,
    /// Generated PNGs by (variant, width): encoding one takes a while in a
    /// debug build, longer than a scripted step.
    pngs: Arc<Mutex<PngCache>>,
}

impl FakeServer {
    /// Listen on `addr` (port 0 for any). `slide_png` replaces the generated
    /// deck slide.
    pub async fn start(addr: &str, slide_png: Option<Vec<u8>>) -> std::io::Result<FakeServer> {
        let listener = TcpListener::bind(addr).await?;
        let server = FakeServer {
            addr: listener.local_addr()?,
            state: watch::channel((0, None)).0,
            received: Default::default(),
            requests: Default::default(),
            kick: watch::channel(0).0,
            slide: slide_png.map(Arc::new),
            pngs: Default::default(),
        };
        let s = server.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let s = s.clone();
                tokio::spawn(async move {
                    let _ = s.serve(stream).await;
                });
            }
        });
        Ok(server)
    }

    pub fn ws_url(&self) -> String {
        format!("ws://{}/ws", self.addr)
    }

    /// Put up a new state; every connected client gets it.
    pub fn set(&self, snapshot: Snapshot) {
        self.state.send_modify(|(n, s)| {
            *n += 1;
            *s = Some(snapshot);
        });
    }

    /// Drop every WebSocket connection.
    pub fn kick(&self) {
        self.kick.send_modify(|n| *n += 1);
    }

    pub fn hellos(&self) -> usize {
        self.received.lock().unwrap().iter().filter(|m| matches!(m, ClientMessage::Hello { .. })).count()
    }

    async fn serve(&self, stream: TcpStream) -> std::io::Result<()> {
        // Peek at the request head to tell a WebSocket upgrade from a GET.
        let mut buf = vec![0; 8192];
        let head = loop {
            let n = stream.peek(&mut buf).await?;
            let text = String::from_utf8_lossy(&buf[..n]).to_string();
            if text.contains("\r\n\r\n") || n == buf.len() {
                break text;
            }
            if n == 0 {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        };
        if head.to_ascii_lowercase().contains("upgrade: websocket") {
            self.websocket(stream).await;
            Ok(())
        } else {
            self.http(stream, &head).await
        }
    }

    async fn websocket(&self, stream: TcpStream) {
        let Ok(ws) = tokio_tungstenite::accept_async(stream).await else { return };
        let (mut tx, mut rx) = ws.split();
        let mut state = self.state.subscribe();
        let mut kick = self.kick.subscribe();
        state.mark_changed();
        loop {
            tokio::select! {
                changed = state.changed() => {
                    if changed.is_err() { return; }
                    let snapshot = state.borrow_and_update().1.clone();
                    if let Some(snapshot) = snapshot {
                        let json = serde_json::to_string(&ServerMessage::State(Box::new(snapshot))).unwrap();
                        if tx.send(Message::text(json)).await.is_err() { return; }
                    }
                }
                message = rx.next() => match message {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(m) = serde_json::from_str::<ClientMessage>(text.as_str()) {
                            self.received.lock().unwrap().push(m);
                        }
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return,
                    _ => {}
                },
                _ = kick.changed() => {
                    let _ = tx.close().await;
                    return;
                }
            }
        }
    }

    async fn http(&self, mut stream: TcpStream, head: &str) -> std::io::Result<()> {
        let mut discard = vec![0; head.find("\r\n\r\n").map_or(head.len(), |i| i + 4)];
        stream.read_exact(&mut discard).await?;
        let target = head.split_whitespace().nth(1).unwrap_or("/").to_string();
        self.requests.lock().unwrap().push(target.clone());
        let (path, query) = target.split_once('?').unwrap_or((&target, ""));
        let width = query.split('&').find_map(|kv| kv.strip_prefix("w=")).and_then(|w| w.parse::<u32>().ok());
        let width = width.unwrap_or(1280);
        let body = match path {
            SLIDE_URL if self.slide.is_some() => self.slide.clone(),
            SLIDE_URL => Some(self.png(0, width, || demo_slide(width, 0))),
            SLIDE2_URL => Some(self.png(1, width, || demo_slide(width, 1))),
            LOGO_URL => Some(self.png(2, 0, demo_logo)),
            _ => None,
        };
        let response = match body {
            Some(body) => {
                let mut r = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .into_bytes();
                r.extend(body.iter());
                r
            }
            None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        };
        stream.write_all(&response).await?;
        stream.shutdown().await
    }
}

impl FakeServer {
    fn png(&self, variant: u32, width: u32, make: impl FnOnce() -> Vec<u8>) -> Arc<Vec<u8>> {
        if let Some(png) = self.pngs.lock().unwrap().get(&(variant, width)) {
            return png.clone();
        }
        let png = Arc::new(make());
        self.pngs.lock().unwrap().insert((variant, width), png.clone());
        png
    }
}

fn png(img: &RgbaImage) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

/// A 16:9 test slide at `width`: four coloured quadrants (orange, teal on
/// slide 0; purple, green on slide 1) with a white border.
pub fn demo_slide(width: u32, variant: u32) -> Vec<u8> {
    let width = width.clamp(16, 4096);
    let height = width * 9 / 16;
    let colours = if variant == 0 {
        [[240, 120, 30], [20, 160, 160], [20, 160, 160], [240, 120, 30]]
    } else {
        [[130, 60, 200], [40, 180, 70], [40, 180, 70], [130, 60, 200]]
    };
    let border = (width / 100).max(2);
    let img = RgbaImage::from_fn(width, height, |x, y| {
        if x < border || y < border || x >= width - border || y >= height - border {
            return Rgba([255, 255, 255, 255]);
        }
        let q = (x >= width / 2) as usize + 2 * (y >= height / 2) as usize;
        let [r, g, b] = colours[q];
        Rgba([r, g, b, 255])
    });
    png(&img)
}

/// A square gold ring "logo" on transparent.
pub fn demo_logo() -> Vec<u8> {
    let n = 512;
    let img = RgbaImage::from_fn(n, n, |x, y| {
        let (dx, dy) = (x as f32 - n as f32 / 2.0, y as f32 - n as f32 / 2.0);
        let d = (dx * dx + dy * dy).sqrt() / (n as f32 / 2.0);
        if (0.55..0.9).contains(&d) || d < 0.2 {
            Rgba([230, 190, 60, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });
    png(&img)
}

/// A snapshot with nothing live, for building the script.
pub fn snapshot(revision: u64) -> Snapshot {
    Snapshot {
        service_name: "Fake service".into(),
        items: Vec::new(),
        preview: None,
        live: None,
        mode: Mode::Normal,
        preview_cue: None,
        live_cue: None,
        next_cue: None,
        logo: Some(LOGO_URL.into()),
        service_started_at: None,
        item_started_at: None,
        revision,
    }
}

fn text_cue() -> CueView {
    CueView::Text {
        lines: vec![
            "Amazing grace! How sweet the sound".into(),
            "That saved a wretch like me!".into(),
            "I once was lost, but now am found;".into(),
            "Was blind, but now I see.".into(),
        ],
        footer: Some("Amazing Grace · John Newton · Public domain".into()),
        label: "V1".into(),
        notes: None,
    }
}

fn image_cue(url: &str, label: &str) -> CueView {
    CueView::Image { url: url.into(), aspect: 16.0 / 9.0, label: label.into(), notes: None }
}

/// The scripted show: (what it is, the state).
pub fn script() -> Vec<(&'static str, Snapshot)> {
    let steps: Vec<(&str, Mode, Option<CueView>, Option<CueView>)> = vec![
        ("nothing live: black", Mode::Normal, None, Some(text_cue())),
        ("text cue (next: deck slide 1)", Mode::Normal, Some(text_cue()), Some(image_cue(SLIDE_URL, "Slide 1"))),
        ("deck slide 1 (orange/teal)", Mode::Normal, Some(image_cue(SLIDE_URL, "Slide 1")), Some(image_cue(SLIDE2_URL, "Slide 2"))),
        ("deck slide 2 (purple/green)", Mode::Normal, Some(image_cue(SLIDE2_URL, "Slide 2")), None),
        ("black", Mode::Black, Some(image_cue(SLIDE2_URL, "Slide 2")), None),
        ("logo", Mode::Logo, Some(image_cue(SLIDE2_URL, "Slide 2")), None),
        ("clear: background only", Mode::Clear, Some(text_cue()), None),
        ("text cue again", Mode::Normal, Some(text_cue()), None),
    ];
    steps
        .into_iter()
        .enumerate()
        .map(|(i, (what, mode, live_cue, next_cue))| {
            let mut s = snapshot(i as u64 + 1);
            s.mode = mode;
            s.live = live_cue.as_ref().map(|_| lampstand_show::protocol::Pos { item: 0, cue: i });
            s.live_cue = live_cue;
            s.next_cue = next_cue;
            (what, s)
        })
        .collect()
}
