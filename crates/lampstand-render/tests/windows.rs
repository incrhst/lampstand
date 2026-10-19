//! Real output windows: three windows on the render thread, and the
//! click-to-photon latency measured from frame timestamps (NFR: under
//! 50 ms).
//!
//! Needs a display, so it skips (and passes) on CI unless
//! `LAMPSTAND_WINDOW_TESTS=1`, on Linux without `DISPLAY` or
//! `WAYLAND_DISPLAY`, and wherever no window or GPU adapter can be made.
//! Run it by hand with:
//!
//! cargo test -p lampstand-render --test windows
//!
//! Each round sends one `Show` to all three outputs at a random moment
//! relative to vsync, as a click would arrive, and records send→present for
//! each. Click-to-photon is that plus at most one refresh (FIFO).

#[path = "support/windows.rs"]
mod support;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lampstand_core::geom::{Color, Point, Transform};
use lampstand_core::id::BlobHash;
use lampstand_core::output::OutputId;
use lampstand_core::scene::*;
use lampstand_render::{
    BlobSource, OutputCommand, OutputEvent, OutputOptions, OutputSink, Outputs, RenderError,
    Renderer,
};
use support::{Latency, OutputWindow};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::WindowId;

const ROUNDS: usize = 60;
/// A covered window (or a locked screen) never presents on macOS; give up
/// rather than hang.
const DEADLINE: Duration = Duration::from_secs(60);
const BUDGET_MS: f64 = 50.0;

struct NoBlobs;

impl BlobSource for NoBlobs {
    fn path(&self, _: &BlobHash) -> Option<PathBuf> {
        None
    }
}

fn skip_reason() -> Option<&'static str> {
    if std::env::var_os("LAMPSTAND_WINDOW_TESTS").is_some() {
        return None;
    }
    if std::env::var_os("CI").is_some() {
        return Some("CI (set LAMPSTAND_WINDOW_TESTS=1 to run)");
    }
    if cfg!(target_os = "linux")
        && std::env::var_os("DISPLAY").is_none()
        && std::env::var_os("WAYLAND_DISPLAY").is_none()
    {
        return Some("no display");
    }
    None
}

fn scene(color: Color, mark: f32) -> Arc<Scene> {
    let rect = |x: f32, y: f32, w: f32, h: f32, color| Node {
        id: 1,
        transform: Transform::IDENTITY,
        opacity: 1.0,
        visible: true,
        kind: NodeKind::Path(PathNode {
            commands: vec![
                PathCommand::MoveTo(Point { x, y }),
                PathCommand::LineTo(Point { x: x + w, y }),
                PathCommand::LineTo(Point { x: x + w, y: y + h }),
                PathCommand::LineTo(Point { x, y: y + h }),
                PathCommand::Close,
            ],
            fill: Some(Paint::Solid(color)),
            stroke: None,
            shadow: None,
        }),
    };
    Arc::new(Scene {
        width: 1920.0,
        height: 1080.0,
        nodes: vec![
            rect(0.0, 0.0, 1920.0, 1080.0, color),
            rect(mark, 440.0, 200.0, 200.0, Color::WHITE),
        ],
    })
}

enum Phase {
    Opening,
    Settling(Instant),
    Measuring {
        round: usize,
        sent: Option<Instant>,
        waiting: usize,
        next_at: Instant,
    },
}

struct App {
    outputs: Outputs,
    windows: Vec<OutputWindow>,
    phase: Phase,
    latency: Latency,
    per_round_max: Latency,
    round_max: f64,
    rng: u64,
    refresh_ms: f64,
    failure: Option<String>,
    started: Instant,
}

impl App {
    fn random_ms(&mut self, max: u64) -> u64 {
        // xorshift: enough to spread sends across the refresh interval.
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng % max
    }

    fn send_round(&mut self, round: usize) {
        let color = if round.is_multiple_of(2) {
            Color::rgb(20, 40, 120)
        } else {
            Color::rgb(120, 30, 30)
        };
        let s = scene(color, 100.0 + 25.0 * round as f32);
        for w in &self.windows {
            self.outputs.send(
                w.id,
                OutputCommand::Show {
                    scene: s.clone(),
                    transition: Transition::CUT,
                },
            );
        }
    }
}

impl ApplicationHandler<OutputEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !self.windows.is_empty() {
            return;
        }
        for i in 0..3 {
            let id = OutputId::generate();
            self.outputs.send(
                id,
                OutputCommand::Show {
                    scene: scene(Color::rgb(30, 30, 30), 100.0),
                    transition: Transition::CUT,
                },
            );
            let position = Some((40 + 660 * i, 80));
            match support::open(
                event_loop,
                &self.outputs,
                id,
                &format!("Lampstand window test {}", i + 1),
                None,
                (640, 360),
                position,
            ) {
                Ok(w) => self.windows.push(w),
                Err(e) => {
                    self.failure = Some(format!("could not open a window: {e:?}"));
                    event_loop.exit();
                    return;
                }
            }
        }
        self.refresh_ms = support::refresh_ms(&self.windows[0].window);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: OutputEvent) {
        match event {
            OutputEvent::FirstFrame(id) => {
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                    w.on_first_frame();
                }
                if matches!(self.phase, Phase::Opening) && self.windows.iter().all(|w| w.shown) {
                    self.phase = Phase::Settling(Instant::now() + Duration::from_millis(800));
                }
            }
            OutputEvent::Presented(timing) => {
                let Phase::Measuring {
                    sent: Some(sent),
                    waiting,
                    ..
                } = &mut self.phase
                else {
                    return;
                };
                if timing.sent < *sent || *waiting == 0 {
                    return;
                }
                *waiting -= 1;
                let done = *waiting == 0;
                self.latency.push(&timing);
                self.round_max = self.round_max.max(timing.latency().as_secs_f64() * 1000.0);
                if done {
                    self.per_round_max
                        .samples
                        .push(std::mem::take(&mut self.round_max));
                    let gap = 30 + self.random_ms(50);
                    if let Phase::Measuring {
                        round,
                        sent,
                        next_at,
                        ..
                    } = &mut self.phase
                    {
                        *round += 1;
                        *sent = None;
                        *next_at = Instant::now() + Duration::from_millis(gap);
                        if *round >= ROUNDS {
                            event_loop.exit();
                        }
                    }
                }
            }
            OutputEvent::SurfaceLost { output, reason } => {
                self.failure = Some(format!("surface lost on {output:?}: {reason}"));
                event_loop.exit();
            }
            OutputEvent::Restarted { message, .. } => {
                self.failure = Some(format!("render thread panicked: {message}"));
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(w) = self.windows.iter().find(|w| w.window.id() == id) {
                    self.outputs.resize(w.id, size.width, size.height);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if now - self.started > DEADLINE {
            self.failure = Some(format!(
                "gave up after {DEADLINE:?} with {} frames measured; are the windows covered \
                 or the screen locked?",
                self.latency.samples.len()
            ));
            event_loop.exit();
            return;
        }
        match &mut self.phase {
            Phase::Opening => {}
            Phase::Settling(until) => {
                if now >= *until {
                    self.phase = Phase::Measuring {
                        round: 0,
                        sent: None,
                        waiting: 0,
                        next_at: now,
                    };
                }
            }
            Phase::Measuring {
                round,
                sent,
                waiting,
                next_at,
            } => {
                if sent.is_none() && now >= *next_at {
                    *sent = Some(Instant::now());
                    *waiting = self.windows.len();
                    let r = *round;
                    self.send_round(r);
                }
            }
        }
        let wake = match &self.phase {
            Phase::Measuring {
                next_at,
                sent: None,
                ..
            } => *next_at,
            Phase::Settling(until) => *until,
            _ => now + Duration::from_millis(50),
        };
        event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
    }
}

fn main() {
    if let Some(reason) = skip_reason() {
        println!("windows: skipped ({reason})");
        return;
    }
    let event_loop = match EventLoop::<OutputEvent>::with_user_event().build() {
        Ok(e) => e,
        Err(e) => {
            println!("windows: skipped (no event loop: {e})");
            return;
        }
    };
    let renderer = match Renderer::new(Arc::new(NoBlobs)) {
        Ok(r) => r,
        Err(RenderError::NoAdapter) => {
            println!("windows: skipped (no GPU adapter)");
            return;
        }
        Err(e) => panic!("renderer: {e:?}"),
    };
    println!("windows: rendering on {:?}", renderer.adapter_info());
    let proxy = event_loop.create_proxy();
    let outputs = Outputs::spawn(renderer, OutputOptions::default(), move |e| {
        let _ = proxy.send_event(e);
    })
    .unwrap();
    let mut app = App {
        outputs,
        windows: Vec::new(),
        phase: Phase::Opening,
        latency: Latency::default(),
        per_round_max: Latency::default(),
        round_max: 0.0,
        rng: 0x9E37_79B9_7F4A_7C15,
        refresh_ms: 1000.0 / 60.0,
        failure: None,
        started: Instant::now(),
    };
    event_loop.run_app(&mut app).unwrap();

    for w in &app.windows {
        app.outputs.detach(w.id);
    }
    if let Some(failure) = app.failure {
        panic!("{failure}");
    }
    assert!(
        app.latency.samples.len() >= ROUNDS * app.windows.len(),
        "only {} frames measured",
        app.latency.samples.len()
    );
    app.latency.report("windows: each output", app.refresh_ms);
    let bound = app
        .per_round_max
        .report("windows: slowest of three per click", app.refresh_ms);
    assert!(
        bound < BUDGET_MS,
        "click-to-photon p95 bound {bound:.1} ms is over the {BUDGET_MS} ms budget"
    );
    println!("windows: ok");
}
