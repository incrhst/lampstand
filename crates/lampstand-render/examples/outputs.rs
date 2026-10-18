//! Opens one borderless fullscreen output per display (manual test).
//!
//!     cargo run -p lampstand-render --example outputs --release
//!     cargo run -p lampstand-render --example outputs -- --display 1 --exit-after 5
//!     cargo run -p lampstand-render --example outputs -- --cycle 2
//!
//! `--cycle N` advances to the next slide every N seconds without input.
//!
//! Keys (with any output focused):
//!   Space / Right  next slide: calibration, lyric, scripture, image, overflow
//!   Left           previous slide
//!   B              background bleed / fit (TS-3)
//!   S              safe-area overlay on / off (TS-8)
//!   [ / ]          shrink / grow all margins by 1%
//!   Esc / Q        quit
//!
//! Each slide change prints how long it took to reach the screen.

use std::sync::Arc;
use std::time::{Duration, Instant};

use lampstand_render::window::{displays, open_outputs, OutputWindow};
use lampstand_render::winit::application::ApplicationHandler;
use lampstand_render::winit::event::{ElementState, KeyEvent, WindowEvent};
use lampstand_render::winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use lampstand_render::winit::keyboard::{Key, NamedKey};
use lampstand_render::winit::window::WindowId;
use lampstand_render::{Background, BackgroundMode, Color, Image, Output, SafeArea, Slide, TextSlide};

const LYRIC: &str = "Amazing grace! How sweet the sound\nThat saved a wretch like me!\nI once was lost, but now am found;\nWas blind, but now I see.";
const SCRIPTURE: &str = "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.";

struct App {
    only: Option<usize>,
    exit_at: Option<Instant>,
    cycle: Option<Duration>,
    next_at: Option<Instant>,
    outputs: Vec<OutputWindow>,
    slide: usize,
    margin: f32,
    bleed: bool,
    overlay: bool,
    image: Arc<Image>,
}

impl App {
    fn slides(&self, label: &str) -> Vec<Slide> {
        let bg = Background::Color(Color::rgb(20, 40, 120));
        vec![
            Slide::calibration(label),
            Slide::text(TextSlide::new(LYRIC).with_footer("John Newton, 1779")).with_background(bg.clone()),
            Slide::text(TextSlide::new(SCRIPTURE).with_footer("John 3:16 (KJV)")).with_background(bg.clone()),
            Slide::image(self.image.clone()),
            Slide::text(TextSlide::new(SCRIPTURE.repeat(5))).with_background(bg),
        ]
    }

    fn output(&self, base: &Output) -> Output {
        let m = self.margin;
        Output {
            safe_area: SafeArea { top: m, right: m, bottom: m, left: m },
            background_mode: if self.bleed { BackgroundMode::Bleed } else { BackgroundMode::Fit },
            show_safe_area: self.overlay,
            ..base.clone()
        }
    }

    fn label(window: &OutputWindow) -> String {
        let d = window.display();
        format!("Output {}: {}", d.index + 1, d.name)
    }

    fn refresh(&mut self) {
        for i in 0..self.outputs.len() {
            let output = self.output(self.outputs[i].output());
            let slides = self.slides(&Self::label(&self.outputs[i]));
            let slide = slides[self.slide % slides.len()].clone();
            let window = &mut self.outputs[i];
            window.set_output(output).expect("set output");
            let layout = window.set_slide(slide).expect("set slide");
            if layout.overflow() {
                println!("{}: text overflows the safe area at minimum size", Self::label(window));
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !self.outputs.is_empty() {
            return;
        }
        let found = displays(event_loop);
        for d in &found {
            println!(
                "display {}: {} at {:?}, {}x{} @{}x, {:?} Hz",
                d.index, d.name, d.position, d.size.0, d.size.1, d.scale_factor, d.refresh_hz
            );
        }
        let targets: Vec<_> = found
            .into_iter()
            .filter(|d| self.only.is_none_or(|i| i == d.index))
            .map(|d| {
                let label = format!("Output {}: {}", d.index + 1, d.name);
                let output = self.output(&Output::new(d.size.0, d.size.1));
                (d, output, Slide::calibration(label))
            })
            .collect();
        if targets.is_empty() {
            eprintln!("no matching display");
            event_loop.exit();
            return;
        }
        let (gpu, outputs) = open_outputs(event_loop, targets).expect("open outputs");
        let info = gpu.adapter.get_info();
        println!("GPU: {} ({:?}, {:?})", info.name, info.device_type, info.backend);
        self.outputs = outputs;
        self.next_at = self.cycle.map(|c| Instant::now() + c);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        match &event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput { event: KeyEvent { logical_key, state: ElementState::Pressed, .. }, .. } => {
                match logical_key {
                    Key::Named(NamedKey::Escape) => event_loop.exit(),
                    Key::Named(NamedKey::Space | NamedKey::ArrowRight) => self.slide += 1,
                    Key::Named(NamedKey::ArrowLeft) => self.slide = self.slide.saturating_sub(1),
                    Key::Character(c) => match c.as_str() {
                        "q" => event_loop.exit(),
                        "b" => self.bleed = !self.bleed,
                        "s" => self.overlay = !self.overlay,
                        "[" => self.margin = (self.margin - 0.01).max(0.0),
                        "]" => self.margin = (self.margin + 0.01).min(0.3),
                        _ => return,
                    },
                    _ => return,
                }
                self.refresh();
            }
            _ => {}
        }
        if let Some(window) = self.outputs.iter_mut().find(|w| w.id() == id) {
            match window.handle_event(&event) {
                Ok(Some(latency)) => println!("{}: slide on screen after {latency:?}", Self::label(window)),
                Ok(None) => {}
                Err(e) => eprintln!("{}: {e}", Self::label(window)),
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if self.exit_at.is_some_and(|at| now >= at) {
            event_loop.exit();
            return;
        }
        if let (Some(at), Some(cycle)) = (self.next_at, self.cycle) {
            if now >= at {
                self.slide += 1;
                self.refresh();
                self.next_at = Some(now + cycle);
            }
        }
        match self.next_at.into_iter().chain(self.exit_at).min() {
            Some(at) => event_loop.set_control_flow(ControlFlow::WaitUntil(at)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }
}

/// A gradient standing in for a slide rendered elsewhere (16:9).
fn sample_image() -> Image {
    let (w, h) = (1600u32, 900u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let border = x < 8 || y < 8 || x >= w - 8 || y >= h - 8;
            let c = if border { [255, 255, 255] } else { [(x * 255 / w) as u8, (y * 255 / h) as u8, 160] };
            rgba.extend([c[0], c[1], c[2], 255]);
        }
    }
    Image::new(w, h, rgba)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut only, mut exit_after, mut cycle) = (None, None, None);
    while let Some(arg) = args.next() {
        let value = args.next().and_then(|v| v.parse::<u64>().ok());
        match arg.as_str() {
            "--display" => only = value.map(|v| v as usize),
            "--exit-after" => exit_after = value.map(Duration::from_secs),
            "--cycle" => cycle = value.map(Duration::from_secs),
            _ => {
                eprintln!("usage: outputs [--display N] [--exit-after SECONDS] [--cycle SECONDS]");
                return;
            }
        }
    }
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App {
        only,
        exit_at: exit_after.map(|d| Instant::now() + d),
        cycle,
        next_at: None,
        outputs: Vec::new(),
        slide: 0,
        margin: 0.05,
        bleed: true,
        overlay: false,
        image: Arc::new(sample_image()),
    };
    event_loop.run_app(&mut app).expect("run");
}
