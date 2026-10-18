//! Title-safety calibration on a real projector or TV (TS-1, TS-2, TS-3, TS-8).
//!
//!     cargo run -p lampstand-render --release --example calibrate -- --list
//!     cargo run -p lampstand-render --release --example calibrate -- --display 1
//!
//! Options: --display N (default 0), --file PATH (default
//! lampstand-calibration.json in the current directory).
//!
//! Keys:
//!   Tab, or 1 2 3 4    pick the edge: top, right, bottom, left
//!   Up / Right         move that edge in by 0.5% (Shift: 2%)
//!   Down / Left        move it back out
//!   R                  reset all margins to 0
//!   T                  toggle calibration grid / sample lyric slide
//!   B                  lyric slide background: bleed / fit
//!   S                  safe-area outline on / off
//!   Enter              save margins to the file
//!   Esc / Q            save and quit
//!
//! Volunteer: on the grid, read the smallest number you can see fully on
//! each edge; set that edge's margin to that number (the cyan line moves
//! with it). Margins for each display are saved under the display's name and
//! loaded again next time.

use std::path::PathBuf;

use lampstand_render::window::{displays, open_outputs, OutputWindow};
use lampstand_render::winit::application::ApplicationHandler;
use lampstand_render::winit::event::{ElementState, KeyEvent, WindowEvent};
use lampstand_render::winit::event_loop::{ActiveEventLoop, EventLoop};
use lampstand_render::winit::keyboard::{Key, ModifiersState, NamedKey};
use lampstand_render::winit::window::WindowId;
use lampstand_render::{Background, BackgroundMode, Color, Output, SafeArea, Slide, TextSlide};
use serde_json::{json, Value};

const LYRIC: &str = "Amazing grace! How sweet the sound\nThat saved a wretch like me!\nI once was lost, but now am found;\nWas blind, but now I see.";
const EDGES: [&str; 4] = ["top", "right", "bottom", "left"];

struct App {
    display: usize,
    file: PathBuf,
    window: Option<OutputWindow>,
    name: String,
    margins: [f32; 4],
    edge: usize,
    lyric: bool,
    bleed: bool,
    outline: bool,
    modifiers: ModifiersState,
    status: String,
}

impl App {
    fn safe_area(&self) -> SafeArea {
        let [top, right, bottom, left] = self.margins;
        SafeArea { top, right, bottom, left }
    }

    fn margins_text(&self) -> String {
        EDGES
            .iter()
            .zip(self.margins)
            .enumerate()
            .map(|(i, (name, m))| {
                let mark = if i == self.edge { ">" } else { " " };
                format!("{mark}{} {:.1}%", name.to_uppercase(), m * 100.0)
            })
            .collect::<Vec<_>>()
            .join("   ")
    }

    fn update(&mut self) {
        let margins = {
            let [t, r, b, l] = self.margins.map(|m| m * 100.0);
            format!("T {t:.1}%  R {r:.1}%  B {b:.1}%  L {l:.1}%")
        };
        let slide = if self.lyric {
            Slide::text(TextSlide::new(LYRIC).with_footer(format!(
                "{margins}  ·  {}  ·  T grid  B bleed/fit  S outline",
                if self.bleed { "bleed" } else { "fit" }
            )))
            .with_background(Background::Color(Color::rgb(20, 40, 120)))
        } else {
            Slide::calibration(format!(
                "{}\n{}\nTab edge · arrows move · T lyric · Enter save · Esc quit\n{}",
                self.name,
                self.margins_text(),
                self.status
            ))
        };
        let (safe_area, show_safe_area) = (self.safe_area(), self.lyric && self.outline);
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

    fn load(&mut self) {
        let Ok(text) = std::fs::read_to_string(&self.file) else { return };
        let Ok(all) = serde_json::from_str::<Value>(&text) else { return };
        if let Some(saved) = all.get(&self.name).and_then(|d| d.get("safe_area")) {
            for (i, edge) in EDGES.iter().enumerate() {
                if let Some(v) = saved.get(*edge).and_then(Value::as_f64) {
                    self.margins[i] = v as f32;
                }
            }
            self.status = format!("loaded from {}", self.file.display());
        }
    }

    fn save(&mut self) {
        let mut all = std::fs::read_to_string(&self.file)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        let (width, height) = self.window.as_ref().map(|w| (w.output().width, w.output().height)).unwrap_or_default();
        let [top, right, bottom, left] = self.margins.map(|m| (m as f64 * 1000.0).round() / 1000.0);
        all[&self.name] = json!({
            "width": width,
            "height": height,
            "safe_area": { "top": top, "right": right, "bottom": bottom, "left": left },
        });
        let result = std::fs::write(&self.file, serde_json::to_string_pretty(&all).unwrap() + "\n");
        self.status = match result {
            Ok(()) => format!("saved to {}", self.file.display()),
            Err(e) => format!("could not save: {e}"),
        };
        println!("{}  ({})", self.status, self.margins_text());
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let Some(display) = displays(event_loop).into_iter().find(|d| d.index == self.display) else {
            eprintln!("no display {}; run with --list", self.display);
            event_loop.exit();
            return;
        };
        self.name = display.name.clone();
        self.load();
        let output = Output::new(display.size.0, display.size.1).with_safe_area(self.safe_area());
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
        match &event {
            WindowEvent::CloseRequested => {
                self.save();
                event_loop.exit();
                return;
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event: KeyEvent { logical_key, state: ElementState::Pressed, .. }, .. } => {
                let step = if self.modifiers.shift_key() { 0.02 } else { 0.005 };
                match logical_key {
                    Key::Named(NamedKey::Escape) => {
                        self.save();
                        event_loop.exit();
                        return;
                    }
                    Key::Named(NamedKey::Tab) => self.edge = (self.edge + 1) % 4,
                    Key::Named(NamedKey::ArrowUp | NamedKey::ArrowRight) => {
                        self.margins[self.edge] = (self.margins[self.edge] + step).min(0.45)
                    }
                    Key::Named(NamedKey::ArrowDown | NamedKey::ArrowLeft) => {
                        self.margins[self.edge] = (self.margins[self.edge] - step).max(0.0)
                    }
                    Key::Named(NamedKey::Enter) => self.save(),
                    Key::Character(c) => match c.to_lowercase().as_str() {
                        "1" | "2" | "3" | "4" => self.edge = c.parse::<usize>().unwrap() - 1,
                        "q" => {
                            self.save();
                            event_loop.exit();
                            return;
                        }
                        "r" => self.margins = [0.0; 4],
                        "t" => self.lyric = !self.lyric,
                        "b" => self.bleed = !self.bleed,
                        "s" => self.outline = !self.outline,
                        _ => return,
                    },
                    _ => return,
                }
                self.update();
            }
            _ => {}
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
        file: PathBuf::from("lampstand-calibration.json"),
        window: None,
        name: String::new(),
        margins: [0.0; 4],
        edge: 0,
        lyric: false,
        bleed: true,
        outline: true,
        modifiers: ModifiersState::empty(),
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
            "--file" => match args.next() {
                Some(path) => app.file = path.into(),
                None => return eprintln!("--file needs a path"),
            },
            _ => return eprintln!("usage: calibrate [--list] [--display N] [--file PATH]"),
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
