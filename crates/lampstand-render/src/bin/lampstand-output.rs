//! Shows a song on an output screen, one section at a time.
//!
//! Usage: lampstand-output [--windowed] [--safe TOP,RIGHT,BOTTOM,LEFT] [SONG.json]
//!
//! Keys: Right, Down, Space or Page Down for the next slide; Left, Up or
//! Page Up for the previous one; B or . for black; C or W to clear the text;
//! S to show the safe area. Ctrl+Q (Cmd+Q on macOS) quits; Esc also quits
//! in a window.

use std::sync::Arc;
use std::time::Instant;

use lampstand_core::safe_area::SafeArea;
use lampstand_core::song::Song;
use lampstand_render::live::{song_slides, Live};
use lampstand_render::output::Output;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};

const DEMO_SONG: &str = include_str!("../../examples/amazing-grace.json");

struct Options {
    windowed: bool,
    safe_area: SafeArea,
    song: Song,
}

fn parse_args() -> Result<Options, String> {
    let mut windowed = false;
    let mut safe_area = SafeArea::default();
    let mut path = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--windowed" => windowed = true,
            "--safe" => {
                let value = args.next().ok_or("--safe needs TOP,RIGHT,BOTTOM,LEFT")?;
                let m: Vec<f32> = value
                    .split(',')
                    .map(|v| {
                        v.trim()
                            .parse::<f32>()
                            .map_err(|e| format!("--safe {value}: {e}"))
                    })
                    .collect::<Result<_, _>>()?;
                let [top, right, bottom, left] = m[..] else {
                    return Err("--safe needs four margins, such as 0.05,0.05,0.2,0.05".into());
                };
                safe_area = SafeArea {
                    top,
                    right,
                    bottom,
                    left,
                };
            }
            "-h" | "--help" => return Err(
                "usage: lampstand-output [--windowed] [--safe TOP,RIGHT,BOTTOM,LEFT] [SONG.json]"
                    .into(),
            ),
            _ => path = Some(arg),
        }
    }
    let json = match &path {
        Some(path) => std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?,
        None => DEMO_SONG.to_string(),
    };
    let song = serde_json::from_str(&json)
        .map_err(|e| format!("{}: {e}", path.as_deref().unwrap_or("demo song")))?;
    Ok(Options {
        windowed,
        safe_area,
        song,
    })
}

struct App {
    options: Options,
    live: Live,
    output: Option<Output>,
    modifiers: ModifiersState,
    /// When the last key arrived, to report key-to-frame time.
    pressed_at: Option<Instant>,
    report_timing: bool,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.output.is_some() {
            return;
        }
        let mut attributes = Window::default_attributes()
            .with_title(format!("Lampstand: {}", self.options.song.title));
        if self.options.windowed {
            attributes = attributes.with_inner_size(LogicalSize::new(1280.0, 720.0));
        } else {
            // Prefer a screen other than the operator's, as a projector would be.
            let primary = event_loop.primary_monitor();
            let monitor = event_loop
                .available_monitors()
                .find(|m| Some(m) != primary.as_ref())
                .or(primary);
            attributes = attributes.with_fullscreen(Some(Fullscreen::Borderless(monitor)));
        }
        let window = Arc::new(event_loop.create_window(attributes).expect("create window"));
        if !self.options.windowed {
            window.set_cursor_visible(false);
        }
        let mut output = pollster::block_on(Output::new(window, self.options.safe_area));
        output.renderer.layout(&self.live);
        output.window().request_redraw();
        self.output = Some(output);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(output) = &mut self.output else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                output.resize(size.width, size.height);
                output.renderer.layout(&self.live);
                output.window().request_redraw();
            }
            WindowEvent::Occluded(false) => output.window().request_redraw(),
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                let before = (self.live.index(), self.live.screen());
                match logical_key.as_ref() {
                    Key::Named(
                        NamedKey::ArrowRight
                        | NamedKey::ArrowDown
                        | NamedKey::Space
                        | NamedKey::PageDown,
                    ) => self.live.next(),
                    Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowUp | NamedKey::PageUp) => {
                        self.live.previous()
                    }
                    Key::Named(NamedKey::Escape) if self.options.windowed => event_loop.exit(),
                    Key::Character(c) => match c.to_ascii_lowercase().as_str() {
                        "q" if self.modifiers.control_key() || self.modifiers.super_key() => {
                            event_loop.exit()
                        }
                        "b" | "." => self.live.toggle_black(),
                        "c" | "w" | "," => self.live.toggle_blank(),
                        "s" => output.renderer.show_safe_area = !output.renderer.show_safe_area,
                        _ => {}
                    },
                    _ => {}
                }
                if self.live.index() != before.0 {
                    output.renderer.layout(&self.live);
                }
                self.pressed_at = Some(Instant::now());
                output.window().request_redraw();
            }
            WindowEvent::RedrawRequested => {
                if !output.render(&self.live) {
                    return;
                }
                if let (Some(at), true) = (self.pressed_at.take(), self.report_timing) {
                    let fit = output.renderer.fit();
                    eprintln!(
                        "slide {}/{} {:?}: key to present {:.1} ms, text {:.0} px{}",
                        self.live.index() + 1,
                        self.live.len(),
                        self.live.screen(),
                        at.elapsed().as_secs_f64() * 1000.0,
                        fit.map_or(0.0, |f| f.size),
                        if fit.is_some_and(|f| f.overflows) {
                            " (overflows)"
                        } else {
                            ""
                        },
                    );
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let options = match parse_args() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let slides = match song_slides(&options.song) {
        Ok(slides) => slides,
        Err(e) => {
            eprintln!("{}: {e:?}", options.song.title);
            std::process::exit(1);
        }
    };
    let mut app = App {
        options,
        live: Live::new(slides),
        output: None,
        modifiers: ModifiersState::empty(),
        pressed_at: None,
        report_timing: std::env::var_os("LAMPSTAND_TIMING").is_some(),
    };
    EventLoop::new()
        .expect("event loop")
        .run_app(&mut app)
        .expect("run");
}
