//! Serves Lampstand Remote with a stand-in for the app, so the Playwright
//! test (`web/tests/remote.spec.ts`) can pair a page, press its buttons and
//! see the page follow.
//!
//! cargo run -p lampstand-server --example remote_fixture
//!
//! Prints `READY <port>`, then reads lines from stdin:
//! - `pin` shows a fresh PIN and prints `PIN <digits>`;
//! - `next` moves the live slide on, as the operator at the laptop would,
//!   and prints `MOVED`;
//! - `revoke` forgets every paired phone and prints `REVOKED`.
//!
//! The stand-in has one service: a heading, "Amazing Grace" (two verses, a
//! backing track), a deck slide and a music-only item. It answers live and
//! sound requests as the app would, and refuses everything else, as the
//! app's gate does.

use std::io::BufRead;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use lampstand_core::id::BlobHash;
use lampstand_core::output::OutputId;
use lampstand_core::scene::Scene;
use lampstand_server::{Backend, RemoteHub, Server};
use serde_json::{json, Value};

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

const SERVICE: &str = "00000000-0000-4000-8000-000000000001";
/// Item IDs and their slides' labels, in order.
const ITEMS: &[(&str, &str, &[&str])] = &[
    ("00000000-0000-4000-8000-000000000010", "Welcome", &[]),
    (
        "00000000-0000-4000-8000-000000000011",
        "Amazing Grace",
        &["V1", "V2"],
    ),
    (
        "00000000-0000-4000-8000-000000000012",
        "Notices",
        &["Slide 1"],
    ),
    ("00000000-0000-4000-8000-000000000013", "Walk-out", &[]),
];

#[derive(Default)]
struct Engine {
    /// Index into the flattened slides; `None` before anything is live.
    at: Option<usize>,
    overlay: &'static str,
    playing: Option<(String, String, bool)>,
}

fn slides() -> Vec<(usize, usize)> {
    ITEMS
        .iter()
        .enumerate()
        .flat_map(|(i, (_, _, s))| (0..s.len()).map(move |n| (i, n)))
        .collect()
}

fn loaded() -> Value {
    let items: Vec<Value> = ITEMS
        .iter()
        .map(|(id, title, labels)| {
            let slides: Vec<Value> = labels
                .iter()
                .map(|l| {
                    let lines: Vec<String> = if l.starts_with('V') {
                        vec![format!("{title}, {l}, line one"), "and line two".into()]
                    } else {
                        Vec::new()
                    };
                    json!({ "label": l, "lines": lines, "footer": null, "steps": 0, "notes": "" })
                })
                .collect();
            let sounds = match *title {
                "Amazing Grace" => json!([{ "name": "Grace track", "backing": true, "length_ms": 200000, "start": "Waits" }]),
                "Walk-out" => json!([{ "name": "Walk-out music", "backing": false, "length_ms": 185000, "start": "Waits" }]),
                _ => json!([]),
            };
            json!({ "id": id, "title": title, "slides": slides, "sounds": sounds })
        })
        .collect();
    json!({ "type": "Loaded", "data": { "service": SERVICE, "title": "Sunday morning", "items": items, "sections": [] } })
}

impl Engine {
    fn live_event(&self) -> Value {
        let live = self.at.map(|n| {
            let (item, slide) = slides()[n];
            json!({ "item": ITEMS[item].0, "slide": slide, "step": 0 })
        });
        json!({ "type": "Live", "data": { "live": live, "preview": null, "idle": false, "overlay": self.overlay } })
    }

    fn sound_event(&self) -> Value {
        let playing = self.playing.as_ref().map(|(item, name, paused)| {
            json!({ "item": item, "name": name, "paused": paused, "ending": false, "position_ms": 0, "length_ms": 185000 })
        });
        json!({ "type": "Sound", "data": {
            "here": true, "holder": "", "away": false, "nobody": false, "output_ok": true,
            "playing": playing, "problem": null, "problem_item": null,
        } })
    }

    /// Next and Previous, which lift Idle (Black stays), as the engine does.
    fn step(&mut self, by: isize) {
        if self.overlay == "Idle" {
            self.overlay = "None";
        }
        let last = slides().len() - 1;
        self.at = Some(match self.at {
            None => 0,
            Some(n) => (n as isize + by).clamp(0, last as isize) as usize,
        });
    }
}

fn ok() -> Value {
    json!({ "type": "Ok" })
}

fn error(message: &str) -> Value {
    json!({ "type": "Error", "data": { "message": message } })
}

fn handle(engine: &Mutex<Engine>, hub: &RemoteHub, request: Value) -> Value {
    let mut e = engine.lock().unwrap();
    let area = request["area"].as_str().unwrap_or("");
    let what = &request["request"];
    match (area, what.as_str()) {
        ("Live", Some("Next")) => e.step(1),
        ("Live", Some("Previous")) => e.step(-1),
        ("Live", Some("ToggleBlack")) => {
            e.overlay = if e.overlay == "Black" {
                "None"
            } else {
                "Black"
            }
        }
        ("Live", Some("ToggleIdle")) => {
            e.overlay = if e.overlay == "Idle" { "None" } else { "Idle" }
        }
        ("Live", Some("ClearText")) => {}
        ("Sound", Some("Pause")) => {
            if let Some(p) = &mut e.playing {
                p.2 = true;
            }
        }
        ("Sound", Some("Resume")) => {
            if let Some(p) = &mut e.playing {
                p.2 = false;
            }
        }
        ("Sound", Some("Stop" | "Panic")) => e.playing = None,
        ("Sound", None) if what.get("Play").is_some() => {
            let item = what["Play"]["item"].as_str().unwrap_or("").to_string();
            let name = ITEMS
                .iter()
                .find(|(id, _, _)| *id == item)
                .map_or("", |(_, title, _)| *title);
            let name = if name == "Walk-out" {
                "Walk-out music"
            } else {
                "Grace track"
            };
            e.playing = Some((item, name.into(), false));
        }
        ("Service", None) if what.get("SlideThumbnail").is_some() => {
            return error("no picture for this slide");
        }
        _ => return error("A remote can only run the slides and the sound."),
    }
    if area == "Live" {
        hub.publish("Live", &e.live_event().to_string());
    } else {
        hub.publish("Sound", &e.sound_event().to_string());
    }
    ok()
}

#[tokio::main]
async fn main() {
    let dir = tempfile::tempdir().unwrap();
    let hub = Arc::new(RemoteHub::open(dir.path().join("remotes.json")).unwrap());
    let engine = Arc::new(Mutex::new(Engine {
        overlay: "None",
        ..Engine::default()
    }));
    hub.publish("Loaded", &loaded().to_string());
    {
        let e = engine.lock().unwrap();
        hub.publish("Live", &e.live_event().to_string());
        hub.publish("Sound", &e.sound_event().to_string());
    }
    let (h, eng) = (Arc::downgrade(&hub), engine.clone());
    hub.set_handler(Arc::new(move |request| {
        let answer = match h.upgrade() {
            Some(hub) => handle(&eng, &hub, request),
            None => error("gone"),
        };
        Box::pin(async move { answer })
    }));
    let server = Server::start(
        SocketAddr::from(([127, 0, 0, 1], 0)),
        Arc::new(App { hub: hub.clone() }),
    )
    .await
    .unwrap();
    println!("READY {}", server.local_addr().port());

    let (lines, mut read) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { return };
            if lines.send(line).is_err() {
                return;
            }
        }
    });
    while let Some(line) = read.recv().await {
        match line.trim() {
            "pin" => println!("PIN {}", hub.start_pairing().pin),
            "next" => {
                let mut e = engine.lock().unwrap();
                e.step(1);
                hub.publish("Live", &e.live_event().to_string());
                println!("MOVED");
            }
            "revoke" => {
                for s in hub.sessions() {
                    hub.revoke(&s.id);
                }
                println!("REVOKED");
            }
            _ => {}
        }
    }
}
