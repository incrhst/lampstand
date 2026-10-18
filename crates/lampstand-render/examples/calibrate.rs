//! Title-safety calibration on a real projector or TV (TS-1, TS-2, TS-3, TS-8).
//!
//!     cargo run -p lampstand-render --release --example calibrate -- --list
//!     cargo run -p lampstand-render --release --example calibrate -- --display 1
//!
//! Options: --display N (default 0), --file PATH (default
//! lampstand-calibration.json in the current directory), --window WxH.
//!
//! If display N is not connected, or --window is given, the output is
//! previewed in an ordinary window (default 1280x720) instead of fullscreen.
//! Resize it to try other aspect ratios.
//!
//! How to calibrate: look at the screen from the back of the room. On each
//! edge, move the white line in until everything outside it is cut off or
//! hard to read, and everything inside is fully visible. The numbers on the
//! rings help: the smallest number you can read on an edge is a good margin.
//!
//! Keys:
//!   Arrow keys     move the white (active) edge the way the arrow points;
//!                  an arrow along the other axis makes that edge active
//!                  (Shift: bigger steps)
//!   Tab            next edge
//!   Mouse          drag any edge (in the preview window)
//!   T              test slide: sample lyrics inside the safe area
//!   B              test slide background: full screen / safe area only
//!   S              test slide: safe-area outline on / off
//!   R              reset all margins to 0
//!   Enter          save
//!   Esc / Q        save and quit
//!
//! Margins are saved per display name and loaded again next time.

use std::path::PathBuf;

use lampstand_render::calibration::Edge;
use lampstand_render::window::{displays, open_outputs, DisplayInfo, OutputWindow};
use lampstand_render::winit::application::ApplicationHandler;
use lampstand_render::winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use lampstand_render::winit::event_loop::{ActiveEventLoop, EventLoop};
use lampstand_render::winit::keyboard::{Key, ModifiersState, NamedKey};
use lampstand_render::winit::window::WindowId;
use lampstand_render::{Background, BackgroundMode, Color, Content, Output, SafeArea, Slide, TextSlide};
use serde_json::{json, Value};

const LYRIC: &str = "Amazing grace! How sweet the sound\nThat saved a wretch like me!\nI once was lost, but now am found;\nWas blind, but now I see.";

struct App {
    display: usize,
    window_size: Option<(u32, u32)>,
    file: PathBuf,
    window: Option<OutputWindow>,
    name: String,
    safe: SafeArea,
    edge: Edge,
    lyric: bool,
    bleed: bool,
    outline: bool,
    modifiers: ModifiersState,
    cursor: (f32, f32),
    dragging: bool,
    status: String,
}

impl App {
    fn update(&mut self) {
        let slide = if self.lyric {
            let s = self.safe;
            Slide::text(TextSlide::new(LYRIC).with_footer(format!(
                "Margins: top {:.1}%  right {:.1}%  bottom {:.1}%  left {:.1}%  ·  background: {}  ·  T: back to grid",
                s.top * 100.0,
                s.right * 100.0,
                s.bottom * 100.0,
                s.left * 100.0,
                if self.bleed { "full screen" } else { "safe area only" }
            )))
            .with_background(Background::Color(Color::rgb(20, 40, 120)))
        } else {
            let label = format!(
                "{}\nArrows move the white edge · Tab next edge · drag with mouse\nT test slide · Enter save · Esc save and quit\n{}",
                self.name, self.status
            );
            Slide { content: Content::Calibration { label, active: Some(self.edge) }, ..Default::default() }
        };
        let safe_area = self.safe;
        let show_safe_area = self.lyric && self.outline;
        let background_mode = if self.bleed { BackgroundMode::Bleed } else { BackgroundMode::Fit };
        let Some(window) = self.window.as_mut() else { return };
        let output = Output { safe_area, background_mode, show_safe_area, ..window.output().clone() };
        if let Err(e) = window.set_output(output) {
            eprintln!("render: {e}");
        }
        if let Err(e) = window.set_slide(slide) {
            eprintln!("render: {e}");
        }
    }

    /// Arrow keys move the active edge the way they point. An arrow along the
    /// other axis switches to the edge it points at.
    fn arrow(&mut self, key: NamedKey) {
        let step = if self.modifiers.shift_key() { 0.02 } else { 0.005 };
        let (towards, inward) = match key {
            NamedKey::ArrowUp => (Edge::Top, Edge::Bottom),
            NamedKey::ArrowDown => (Edge::Bottom, Edge::Top),
            NamedKey::ArrowLeft => (Edge::Left, Edge::Right),
            NamedKey::ArrowRight => (Edge::Right, Edge::Left),
            _ => return,
        };
        let delta = if self.edge == towards {
            -step // towards its own side of the screen: out
        } else if self.edge == inward {
            step // towards the opposite side: in
        } else {
            self.edge = towards;
            return;
        };
        let m = self.edge.margin_mut(&mut self.safe);
        *m = (*m + delta).clamp(0.0, 0.45);
    }

    fn size(&self) -> (f32, f32) {
        self.window.as_ref().map_or((1.0, 1.0), |w| (w.output().width as f32, w.output().height as f32))
    }

    /// The edge nearest the cursor, if it is close enough to grab.
    fn edge_at(&self, (x, y): (f32, f32)) -> Option<Edge> {
        let (w, h) = self.size();
        let r = self.safe.rect(w, h);
        let reach = w.min(h) * 0.04;
        let distances = [
            (Edge::Top, (y - r.y).abs()),
            (Edge::Bottom, (y - (r.y + r.height)).abs()),
            (Edge::Left, (x - r.x).abs()),
            (Edge::Right, (x - (r.x + r.width)).abs()),
        ];
        distances.into_iter().filter(|(_, d)| *d < reach).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(e, _)| e)
    }

    fn drag(&mut self) {
        let (w, h) = self.size();
        let (x, y) = self.cursor;
        let value = match self.edge {
            Edge::Top => y / h,
            Edge::Bottom => 1.0 - y / h,
            Edge::Left => x / w,
            Edge::Right => 1.0 - x / w,
        };
        // Snap to half a percent, like the arrow keys.
        *self.edge.margin_mut(&mut self.safe) = ((value * 200.0).round() / 200.0).clamp(0.0, 0.45);
    }

    fn load(&mut self) {
        let Ok(text) = std::fs::read_to_string(&self.file) else { return };
        let Ok(all) = serde_json::from_str::<Value>(&text) else { return };
        if let Some(saved) = all.get(&self.name).and_then(|d| d.get("safe_area")) {
            for edge in Edge::ALL {
                if let Some(v) = saved.get(edge.name().to_lowercase()).and_then(Value::as_f64) {
                    *edge.margin_mut(&mut self.safe) = v as f32;
                }
            }
            self.status = format!("Loaded saved margins from {}", self.file.display());
        }
    }

    fn save(&mut self) {
        let mut all = std::fs::read_to_string(&self.file)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        let (width, height) = self.window.as_ref().map(|w| (w.output().width, w.output().height)).unwrap_or_default();
        let round = |m: f32| (m as f64 * 1000.0).round() / 1000.0;
        let s = self.safe;
        all[&self.name] = json!({
            "width": width,
            "height": height,
            "safe_area": { "top": round(s.top), "right": round(s.right), "bottom": round(s.bottom), "left": round(s.left) },
        });
        let result = std::fs::write(&self.file, serde_json::to_string_pretty(&all).unwrap() + "\n");
        self.status = match result {
            Ok(()) => format!("Saved to {}", self.file.display()),
            Err(e) => format!("Could not save: {e}"),
        };
        println!(
            "{}: top {:.1}%  right {:.1}%  bottom {:.1}%  left {:.1}%",
            self.status,
            s.top * 100.0,
            s.right * 100.0,
            s.bottom * 100.0,
            s.left * 100.0
        );
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let connected = displays(event_loop).into_iter().find(|d| d.index == self.display);
        let display = match (connected, self.window_size) {
            (Some(d), None) => d,
            (connected, size) => {
                if connected.is_none() {
                    println!("display {} is not connected; previewing it in a window", self.display);
                }
                let (w, h) = size.unwrap_or((1280, 720));
                match DisplayInfo::preview(event_loop, self.display, w, h) {
                    Some(d) => d,
                    None => {
                        eprintln!("no displays found");
                        event_loop.exit();
                        return;
                    }
                }
            }
        };
        self.name = display.name.clone();
        self.load();
        let output = Output::new(display.size.0, display.size.1).with_safe_area(self.safe);
        match open_outputs(event_loop, [(display, output, Slide::default())]) {
            Ok((_, mut windows)) => {
                let window = windows.remove(0);
                window.window().focus_window();
                self.window = Some(window);
                self.update();
            }
            Err(e) => {
                eprintln!("could not open output: {e}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let mut changed = true;
        match &event {
            WindowEvent::CloseRequested => {
                self.save();
                event_loop.exit();
                return;
            }
            WindowEvent::ModifiersChanged(m) => {
                self.modifiers = m.state();
                changed = false;
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as f32, position.y as f32);
                changed = self.dragging && !self.lyric;
                if changed {
                    self.drag();
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => match state {
                ElementState::Pressed if !self.lyric => match self.edge_at(self.cursor) {
                    Some(edge) => {
                        self.edge = edge;
                        self.dragging = true;
                    }
                    None => changed = false,
                },
                _ => {
                    changed = self.dragging;
                    self.dragging = false;
                }
            },
            WindowEvent::KeyboardInput { event: KeyEvent { logical_key, state: ElementState::Pressed, .. }, .. } => {
                match logical_key {
                    Key::Named(NamedKey::Escape) => {
                        self.save();
                        event_loop.exit();
                        return;
                    }
                    Key::Named(NamedKey::Tab) => {
                        let i = Edge::ALL.iter().position(|e| *e == self.edge).unwrap_or(0);
                        let step = if self.modifiers.shift_key() { 3 } else { 1 };
                        self.edge = Edge::ALL[(i + step) % 4];
                    }
                    Key::Named(key @ (NamedKey::ArrowUp | NamedKey::ArrowDown | NamedKey::ArrowLeft | NamedKey::ArrowRight)) => {
                        if self.lyric {
                            self.lyric = false;
                        } else {
                            self.arrow(*key);
                        }
                    }
                    Key::Named(NamedKey::Enter) => self.save(),
                    Key::Character(c) => match c.to_lowercase().as_str() {
                        "q" => {
                            self.save();
                            event_loop.exit();
                            return;
                        }
                        "r" => self.safe = SafeArea::default(),
                        "t" => self.lyric = !self.lyric,
                        "b" => self.bleed = !self.bleed,
                        "s" => self.outline = !self.outline,
                        _ => changed = false,
                    },
                    _ => changed = false,
                }
            }
            _ => changed = false,
        }
        if changed {
            self.update();
        }
        if let Some(window) = self.window.as_mut() {
            if let Err(e) = window.handle_event(&event) {
                eprintln!("render: {e}");
            }
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut app = App {
        display: 0,
        window_size: None,
        file: PathBuf::from("lampstand-calibration.json"),
        window: None,
        name: String::new(),
        safe: SafeArea::default(),
        edge: Edge::Top,
        lyric: false,
        bleed: true,
        outline: true,
        modifiers: ModifiersState::empty(),
        cursor: (0.0, 0.0),
        dragging: false,
        status: String::new(),
    };
    let mut list = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--list" => list = true,
            "--display" => match args.next().and_then(|v| v.parse().ok()) {
                Some(n) => app.display = n,
                None => return eprintln!("--display needs a number; see --list"),
            },
            "--window" => match args.next().and_then(|v| {
                let (w, h) = v.split_once('x')?;
                Some((w.parse().ok()?, h.parse().ok()?))
            }) {
                Some(size) => app.window_size = Some(size),
                None => return eprintln!("--window needs a size like 1024x768"),
            },
            "--file" => match args.next() {
                Some(path) => app.file = path.into(),
                None => return eprintln!("--file needs a path"),
            },
            _ => return eprintln!("usage: calibrate [--list] [--display N] [--window WxH] [--file PATH]"),
        }
    }
    let event_loop = EventLoop::new().expect("event loop");
    if list {
        event_loop.run_app(&mut Lister).expect("run");
        return;
    }
    event_loop.run_app(&mut app).expect("run");
}

/// Prints the displays and exits.
struct Lister;

impl ApplicationHandler for Lister {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        for d in displays(event_loop) {
            println!(
                "--display {}  {}  {}x{} at {:?}, scale {}, {} Hz",
                d.index,
                d.name,
                d.size.0,
                d.size.1,
                d.position,
                d.scale_factor,
                d.refresh_hz.map_or("?".into(), |r| format!("{r:.0}"))
            );
        }
        event_loop.exit();
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}
