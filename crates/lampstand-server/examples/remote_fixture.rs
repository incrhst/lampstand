//! Serves Lampstand Remote with a stand-in for the app, so the Playwright
//! test (`web/tests/remote.spec.ts`) can pair a page, press its buttons and
//! see the page follow. Also handy for looking at the page by hand.
//!
//! cargo run -p lampstand-server --example remote_fixture
//!
//! Prints `READY <port>`, then reads lines from stdin:
//! - `pin` shows a fresh PIN for a phone that runs the service and prints
//!   `PIN <digits>`;
//! - `speaker` shows one for a speaker's phone for "Sermon" and prints
//!   `PIN <digits>`;
//! - `next` moves the live slide on, as the operator at the laptop would,
//!   and prints `MOVED`;
//! - `show <item> <slide>` puts that slide live (items counted from 0, as
//!   listed below) and prints `MOVED`;
//! - `close` closes the service, which ends the speakers' phones, and
//!   prints `CLOSED`;
//! - `revoke` forgets every paired phone and prints `REVOKED`.
//!
//! The stand-in has one service, in two groups: "Gathering" with a
//! music-only item and "Amazing Grace" (two verses, a backing track), and
//! "Word" with two decks (one slide with builds each, and speaker notes) and
//! a closing music-only item. It moves through it as the live engine does
//! (Next stops on music-only items and steps through builds), answers live
//! and sound requests and slide pictures (a drawn card with the slide's
//! name) as the app would, and refuses everything else, as the app's gate
//! does. A speaker's phone gets only Next, Back and Blank, inside its item,
//! while that item is live.

use std::io::BufRead;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use lampstand_core::id::BlobHash;
use lampstand_core::output::OutputId;
use lampstand_core::scene::Scene;
use lampstand_server::{Backend, RemoteHub, RemoteKind, Server};
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

/// One item: its ID, title, and slides as (label, builds, notes). A header
/// (the groups in `sections`) has no slides; a music-only item has `music`.
struct Item {
    id: &'static str,
    title: &'static str,
    slides: &'static [(&'static str, usize, &'static str)],
    music: Option<(&'static str, &'static str)>,
}

const ITEMS: &[Item] = &[
    Item {
        id: "00000000-0000-4000-8000-000000000009",
        title: "Gathering",
        slides: &[],
        music: None,
    },
    Item {
        id: "00000000-0000-4000-8000-000000000010",
        title: "Walk-in",
        slides: &[],
        music: Some(("Walk-in music", "Waits")),
    },
    Item {
        id: "00000000-0000-4000-8000-000000000011",
        title: "Amazing Grace",
        slides: &[("V1", 0, ""), ("V2", 0, "")],
        music: None,
    },
    Item {
        id: "00000000-0000-4000-8000-000000000019",
        title: "Word",
        slides: &[],
        music: None,
    },
    Item {
        id: "00000000-0000-4000-8000-000000000012",
        title: "Notices",
        slides: &[("Slide 1", 0, ""), ("Slide 2", 2, "")],
        music: None,
    },
    Item {
        id: "00000000-0000-4000-8000-000000000014",
        title: "Sermon",
        slides: &[
            ("Slide 1", 0, "Welcome everyone.\nThank the band."),
            ("Slide 2", 1, "Three points:\nGrace, peace, hope."),
            ("Slide 3", 0, "Close in prayer."),
        ],
        music: None,
    },
    Item {
        id: "00000000-0000-4000-8000-000000000013",
        title: "Walk-out",
        slides: &[],
        music: Some(("Walk-out music", "OnItsOwn")),
    },
];

const SERMON: usize = 5;

/// Where the live cursor is: (item, slide, step).
type At = (usize, usize, usize);

#[derive(Default)]
struct Engine {
    at: Option<At>,
    /// The slide the screens show while a music-only item is live.
    shown: Option<At>,
    overlay: &'static str,
    playing: Option<(String, String, bool)>,
    open: bool,
}

fn is_text(label: &str) -> bool {
    label.starts_with('V')
}

fn loaded() -> Value {
    let items: Vec<Value> = ITEMS
        .iter()
        .map(|item| {
            let slides: Vec<Value> = item
                .slides
                .iter()
                .map(|(label, steps, notes)| {
                    let lines: Vec<String> = if is_text(label) {
                        vec![
                            format!("{}, {label}, line one", item.title),
                            "and line two".into(),
                        ]
                    } else {
                        Vec::new()
                    };
                    json!({ "label": label, "lines": lines, "footer": null, "steps": steps, "notes": notes })
                })
                .collect();
            let sounds = match (item.title, item.music) {
                ("Amazing Grace", _) => json!([{ "name": "Grace track", "backing": true, "length_ms": 200000, "start": "Waits" }]),
                (_, Some((name, start))) => json!([{ "name": name, "backing": false, "length_ms": 185000, "start": start }]),
                _ => json!([]),
            };
            json!({ "id": item.id, "title": item.title, "slides": slides, "sounds": sounds })
        })
        .collect();
    let sections = json!([
        { "header": ITEMS[0].id, "title": ITEMS[0].title, "seconds": null, "items": [ITEMS[1].id, ITEMS[2].id] },
        { "header": ITEMS[3].id, "title": ITEMS[3].title, "seconds": null, "items": [ITEMS[4].id, ITEMS[5].id, ITEMS[6].id] },
    ]);
    json!({ "type": "Loaded", "data": { "service": SERVICE, "title": "Sunday morning", "items": items, "sections": sections } })
}

fn cursor(at: Option<At>) -> Value {
    at.map_or(
        Value::Null,
        |(i, s, st)| json!({ "item": ITEMS[i].id, "slide": s, "step": st }),
    )
}

/// A drawn card for a deck slide: its item, name and build.
fn picture(item: usize, slide: usize, step: usize) -> String {
    let (label, steps, _) = ITEMS[item].slides[slide];
    let build = if steps > 0 {
        format!("build {step} of {steps}")
    } else {
        String::new()
    };
    let svg = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 160 90'>\
         <rect width='160' height='90' fill='#26408b'/>\
         <rect x='8' y='8' width='144' height='18' rx='3' fill='#f0c75e'/>\
         <text x='80' y='50' font-family='sans-serif' font-size='14' fill='white' text-anchor='middle'>{}</text>\
         <text x='80' y='68' font-family='sans-serif' font-size='11' fill='#cfd8ff' text-anchor='middle'>{label} {build}</text>\
         </svg>",
        ITEMS[item].title
    );
    let encoded: String = svg
        .chars()
        .map(|c| match c {
            '#' => "%23".to_string(),
            '<' => "%3C".to_string(),
            '>' => "%3E".to_string(),
            ' ' => "%20".to_string(),
            c => c.to_string(),
        })
        .collect();
    format!("data:image/svg+xml,{encoded}")
}

impl Engine {
    fn live_event(&self) -> Value {
        json!({ "type": "Live", "data": {
            "live": cursor(self.at),
            "shown": cursor(self.shown),
            "preview": null,
            "idle": self.at.is_none(),
            "overlay": self.overlay,
        } })
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

    /// The first stop at or after `from`: an item with slides, or music.
    fn first_stop(from: usize) -> Option<At> {
        (from..ITEMS.len())
            .find(|&i| !ITEMS[i].slides.is_empty() || ITEMS[i].music.is_some())
            .map(|i| (i, 0, 0))
    }

    fn next_target(&self) -> Option<At> {
        let Some((i, s, st)) = self.at else {
            return Self::first_stop(0);
        };
        let slides = ITEMS[i].slides;
        if let Some((_, steps, _)) = slides.get(s) {
            if st < *steps {
                return Some((i, s, st + 1));
            }
            if s + 1 < slides.len() {
                return Some((i, s + 1, 0));
            }
        }
        Self::first_stop(i + 1)
    }

    fn previous_target(&self) -> Option<At> {
        let (i, s, st) = self.at?;
        if st > 0 {
            return Some((i, s, st - 1));
        }
        if s > 0 {
            return Some((i, s - 1, ITEMS[i].slides[s - 1].1));
        }
        (0..i)
            .rev()
            .find(|&j| !ITEMS[j].slides.is_empty())
            .map(|j| {
                let last = ITEMS[j].slides.len() - 1;
                (j, last, ITEMS[j].slides[last].1)
            })
    }

    /// Takes `at` live, as the engine does: Idle lifts, Black stays.
    fn take(&mut self, at: At) {
        if self.overlay == "Idle" {
            self.overlay = "None";
        }
        if !ITEMS[at.0].slides.is_empty() {
            self.shown = Some(at);
        }
        self.at = Some(at);
    }

    /// The speaker's rule, as the app's gate has it: its item on air, not
    /// under Idle, and Next or Back staying inside it.
    fn speaker_refusal(&self, own: usize, command: &str) -> Option<&'static str> {
        if self.at.map(|a| a.0) != Some(own) {
            return Some("Next and Back work only while your slides are on the screen.");
        }
        if self.overlay == "Idle" {
            return Some("The service team has put up the idle screen.");
        }
        match command {
            "ToggleBlack" => None,
            "Next" if self.next_target().is_some_and(|t| t.0 == own) => None,
            "Next" => Some("That is the last slide. The service team takes it from here."),
            "Previous" if self.previous_target().is_some_and(|t| t.0 == own) => None,
            "Previous" => Some("That is your first slide."),
            _ => Some("A speaker's phone can only change its own slides."),
        }
    }
}

fn ok() -> Value {
    json!({ "type": "Ok" })
}

fn error(message: &str) -> Value {
    json!({ "type": "Error", "data": { "message": message } })
}

fn item_index(id: &str) -> Option<usize> {
    ITEMS.iter().position(|i| i.id == id)
}

fn thumbnail(what: &Value, only: Option<usize>) -> Value {
    let t = &what["SlideThumbnail"];
    let (Some(item), Some(slide), Some(step)) = (
        t["item"].as_str().and_then(item_index),
        t["slide"].as_u64(),
        t["step"].as_u64(),
    ) else {
        return error("no such slide");
    };
    if only.is_some_and(|own| own != item) {
        return error("A speaker's phone can only change its own slides.");
    }
    let (slide, step) = (slide as usize, step as usize);
    match ITEMS[item].slides.get(slide) {
        Some((label, _, _)) if !is_text(label) => {
            json!({ "type": "Thumbnail", "data": picture(item, slide, step) })
        }
        _ => error("no picture for this slide"),
    }
}

fn handle(engine: &Mutex<Engine>, hub: &RemoteHub, kind: &RemoteKind, request: Value) -> Value {
    let mut e = engine.lock().unwrap();
    if !e.open {
        return error("No service is open on the laptop.");
    }
    let area = request["area"].as_str().unwrap_or("");
    let what = &request["request"];
    // A speaker's phone: the gate's rule, checked and applied under the one lock.
    if let RemoteKind::Speaker { item, .. } = kind {
        let Some(own) = item_index(item) else {
            return error("This phone is not set up right.");
        };
        if area == "Service" && what.get("SlideThumbnail").is_some() {
            return thumbnail(what, Some(own));
        }
        let command = what.as_str().unwrap_or("");
        if area != "Live" || !matches!(command, "Next" | "Previous" | "ToggleBlack") {
            return error("A speaker's phone can only change its own slides.");
        }
        if let Some(why) = e.speaker_refusal(own, command) {
            return error(why);
        }
    }
    match (area, what.as_str()) {
        ("Live", Some("Next")) => {
            if let Some(t) = e.next_target() {
                e.take(t);
            }
        }
        ("Live", Some("Previous")) => {
            if let Some(t) = e.previous_target() {
                e.take(t);
            }
        }
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
        ("Live", None) if what.get("Show").is_some() => {
            let show = &what["Show"];
            let (Some(item), Some(slide), Some(step)) = (
                show["item"].as_str().and_then(item_index),
                show["slide"].as_u64(),
                show["step"].as_u64(),
            ) else {
                return error("no such slide");
            };
            e.take((item, slide as usize, step as usize));
        }
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
            let name = item_index(&item)
                .and_then(|i| ITEMS[i].music.map(|m| m.0))
                .unwrap_or("Grace track");
            e.playing = Some((item, name.into(), false));
        }
        ("Service", None) if what.get("SlideThumbnail").is_some() => {
            return thumbnail(what, None);
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
        open: true,
        ..Engine::default()
    }));
    hub.publish("Loaded", &loaded().to_string());
    {
        let e = engine.lock().unwrap();
        hub.publish("Live", &e.live_event().to_string());
        hub.publish("Sound", &e.sound_event().to_string());
    }
    let (h, eng) = (Arc::downgrade(&hub), engine.clone());
    hub.set_handler(Arc::new(move |kind, request| {
        let answer = match h.upgrade() {
            Some(hub) => handle(&eng, &hub, &kind, request),
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
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["pin"] => println!("PIN {}", hub.start_pairing().pin),
            ["speaker"] => {
                let kind = RemoteKind::Speaker {
                    service: SERVICE.into(),
                    item: ITEMS[SERMON].id.into(),
                    title: ITEMS[SERMON].title.into(),
                };
                println!("PIN {}", hub.start_pairing_as(kind).pin);
            }
            ["next"] => {
                let mut e = engine.lock().unwrap();
                if let Some(t) = e.next_target() {
                    e.take(t);
                }
                hub.publish("Live", &e.live_event().to_string());
                println!("MOVED");
            }
            ["show", item, slide] => {
                let mut e = engine.lock().unwrap();
                if let (Ok(item), Ok(slide)) = (item.parse::<usize>(), slide.parse::<usize>()) {
                    if item < ITEMS.len() {
                        e.take((item, slide, 0));
                    }
                }
                hub.publish("Live", &e.live_event().to_string());
                println!("MOVED");
            }
            ["close"] => {
                let mut e = engine.lock().unwrap();
                e.open = false;
                e.at = None;
                e.shown = None;
                hub.publish("Loaded", r#"{"type":"Unloaded"}"#);
                hub.publish("Live", &e.live_event().to_string());
                hub.end_speakers(|s| s == SERVICE);
                println!("CLOSED");
            }
            ["revoke"] => {
                for s in hub.sessions() {
                    hub.revoke(&s.id);
                }
                println!("REVOKED");
            }
            _ => {}
        }
    }
}
