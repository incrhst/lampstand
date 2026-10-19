//! Drives three output windows from a script: lyrics with every transition
//! kind, a deck build step with each animation effect, the safe-area
//! overlay, calibration, a render-thread panic, an unplugged and replugged
//! output, and a click-to-photon measurement.
//!
//! cargo run -p lampstand-render --example outputs [-- --fullscreen] [--loop] [--snapshots DIR]
//!
//! `--snapshots DIR` saves each output's composited picture (as
//! `Outputs::snapshot` sees it) 350 ms into every step, mid-transition for
//! most of them, as PNGs.
//!
//! With `--fullscreen`, outputs go borderless fullscreen on the displays
//! other than the primary one (matched with `assign_displays`), and any
//! output without a display opens as a normal window. Close any window or
//! press Escape to stop.

#[path = "../tests/support/windows.rs"]
mod support;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lampstand_core::geom::{Color, Point, Rect, Transform};
use lampstand_core::id::BlobHash;
use lampstand_core::output::{DisplayMatch, OutputId};
use lampstand_core::safe_area::SafeArea;
use lampstand_core::scene::*;
use lampstand_render::output::assign_displays;
use lampstand_render::{
    BlobSource, CalibrationPattern, OutputCommand, OutputEvent, OutputOptions, OutputSink, Outputs,
    Renderer,
};
use support::{Latency, OutputWindow};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::WindowId;

struct NoBlobs;

impl BlobSource for NoBlobs {
    fn path(&self, _: &BlobHash) -> Option<PathBuf> {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Role {
    Audience,
    Stage,
    Stream,
}

const ROLES: [Role; 3] = [Role::Audience, Role::Stage, Role::Stream];

const LYRICS: [&str; 6] = [
    "Amazing grace, how sweet the sound\nThat saved a wretch like me",
    "I once was lost, but now am found\nWas blind, but now I see",
    "'Twas grace that taught my heart to fear\nAnd grace my fears relieved",
    "How precious did that grace appear\nThe hour I first believed",
    "Through many dangers, toils and snares\nI have already come",
    "'Tis grace hath brought me safe thus far\nAnd grace will lead me home",
];

const BACKGROUNDS: [Color; 6] = [
    Color::rgb(24, 38, 92),
    Color::rgb(92, 30, 48),
    Color::rgb(20, 80, 64),
    Color::rgb(70, 50, 110),
    Color::rgb(110, 70, 20),
    Color::rgb(30, 30, 30),
];

const W: f32 = 1920.0;
const H: f32 = 1080.0;

fn node(id: NodeId, kind: NodeKind) -> Node {
    Node {
        id,
        transform: Transform::IDENTITY,
        opacity: 1.0,
        visible: true,
        kind,
    }
}

fn rect_path(r: Rect, color: Color) -> NodeKind {
    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.width, r.y + r.height);
    NodeKind::Path(PathNode {
        commands: vec![
            PathCommand::MoveTo(Point { x: x0, y: y0 }),
            PathCommand::LineTo(Point { x: x1, y: y0 }),
            PathCommand::LineTo(Point { x: x1, y: y1 }),
            PathCommand::LineTo(Point { x: x0, y: y1 }),
            PathCommand::Close,
        ],
        fill: Some(Paint::Solid(color)),
        stroke: None,
        shadow: None,
    })
}

fn text(id: NodeId, rect: Rect, lines: &str, size: f32, color: Color) -> Node {
    node(
        id,
        NodeKind::Text(TextBlock {
            rect,
            valign: VAlign::Middle,
            paragraphs: lines
                .lines()
                .map(|line| Paragraph {
                    align: HAlign::Center,
                    line_spacing: 1.2,
                    space_before: 0.0,
                    space_after: 0.0,
                    indent: 0.0,
                    bullet: None,
                    runs: vec![TextRun {
                        text: line.into(),
                        font_family: "Noto Sans".into(),
                        size,
                        weight: 600,
                        italic: false,
                        underline: false,
                        strike: false,
                        color,
                    }],
                })
                .collect(),
            shadow: Some(Shadow {
                color: Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 160,
                },
                offset: Point { x: 3.0, y: 3.0 },
                blur: 6.0,
            }),
            outline: None,
        }),
    )
}

fn full() -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        width: W,
        height: H,
    }
}

/// One lyric slide as each role shows it.
fn lyric_scene(role: Role, slide: usize) -> Arc<Scene> {
    let slide = slide % LYRICS.len();
    let background = node(1, rect_path(full(), BACKGROUNDS[slide]));
    let nodes = match role {
        Role::Audience => vec![
            background,
            text(
                2,
                Rect {
                    x: 120.0,
                    y: 200.0,
                    width: 1680.0,
                    height: 680.0,
                },
                LYRICS[slide],
                84.0,
                Color::WHITE,
            ),
        ],
        Role::Stage => vec![
            node(1, rect_path(full(), Color::BLACK)),
            text(
                2,
                Rect {
                    x: 60.0,
                    y: 60.0,
                    width: 1800.0,
                    height: 640.0,
                },
                LYRICS[slide],
                96.0,
                Color::WHITE,
            ),
            text(
                3,
                Rect {
                    x: 60.0,
                    y: 760.0,
                    width: 1800.0,
                    height: 260.0,
                },
                LYRICS[(slide + 1) % LYRICS.len()],
                52.0,
                Color::rgb(255, 200, 60),
            ),
        ],
        Role::Stream => vec![
            node(1, rect_path(full(), Color::rgb(0, 120, 0))),
            node(
                4,
                rect_path(
                    Rect {
                        x: 0.0,
                        y: 780.0,
                        width: W,
                        height: 240.0,
                    },
                    Color {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 190,
                    },
                ),
            ),
            text(
                2,
                Rect {
                    x: 80.0,
                    y: 790.0,
                    width: 1760.0,
                    height: 220.0,
                },
                LYRICS[slide],
                60.0,
                Color::WHITE,
            ),
        ],
    };
    Arc::new(Scene {
        width: W,
        height: H,
        nodes,
    })
}

/// A deck slide whose shapes a build reveals: IDs 10 to 14 start hidden.
fn build_scene() -> Arc<Scene> {
    let shape = |id, x, color| {
        let mut n = node(
            id,
            rect_path(
                Rect {
                    x,
                    y: 420.0,
                    width: 260.0,
                    height: 260.0,
                },
                color,
            ),
        );
        n.visible = false;
        n
    };
    Arc::new(Scene {
        width: W,
        height: H,
        nodes: vec![
            node(1, rect_path(full(), Color::rgb(240, 240, 236))),
            text(
                2,
                Rect {
                    x: 100.0,
                    y: 80.0,
                    width: 1720.0,
                    height: 200.0,
                },
                "Build animations",
                90.0,
                Color::rgb(30, 30, 60),
            ),
            shape(10, 140.0, Color::rgb(220, 60, 60)),
            shape(11, 480.0, Color::rgb(60, 160, 80)),
            shape(12, 820.0, Color::rgb(60, 90, 220)),
            shape(13, 1160.0, Color::rgb(230, 170, 30)),
            shape(14, 1500.0, Color::rgb(150, 60, 170)),
        ],
    })
}

fn anim(node: NodeId, effect: Effect, delay_ms: u32, duration_ms: u32) -> Animation {
    Animation {
        node,
        effect,
        delay_ms,
        duration_ms,
        easing: Easing::EaseOut,
    }
}

#[derive(Debug, Clone)]
enum Step {
    Lyric(usize, Transition),
    Build,
    Animate(Vec<Animation>),
    SafeArea(Option<SafeArea>),
    Calibrate(Option<CalibrationPattern>),
    Panic(Role),
    Unplug(Role),
    Replug(Role),
    Latency(usize),
    End,
}

fn script() -> Vec<(u64, Step)> {
    use Direction::*;
    let t = |kind, duration_ms| Transition { kind, duration_ms };
    let area = SafeArea {
        top: 0.05,
        right: 0.04,
        bottom: 0.05,
        left: 0.04,
    };
    let mut steps = vec![(0, Step::Lyric(0, Transition::CUT))];
    let kinds = [
        TransitionKind::Fade,
        TransitionKind::FadeThroughBlack,
        TransitionKind::Push(Right),
        TransitionKind::Wipe(Left),
        TransitionKind::Cover(Down),
        TransitionKind::Zoom,
        TransitionKind::Push(Up),
        TransitionKind::Cut,
    ];
    for (i, kind) in kinds.into_iter().enumerate() {
        steps.push((1800, Step::Lyric(i + 1, t(kind, 700))));
    }
    steps.extend([
        (1800, Step::Build),
        (
            800,
            Step::Animate(vec![
                anim(10, Effect::FlyIn(Left), 0, 600),
                anim(11, Effect::FadeIn, 200, 600),
                anim(12, Effect::WipeIn(Up), 400, 600),
                anim(13, Effect::ZoomIn, 600, 600),
                anim(14, Effect::Appear, 800, 0),
            ]),
        ),
        (
            1800,
            Step::Animate(vec![
                anim(10, Effect::Spin(360.0), 0, 900),
                anim(11, Effect::Pulse, 0, 900),
                anim(12, Effect::GrowShrink(1.3), 0, 900),
                anim(13, Effect::Transparency(0.6), 0, 900),
            ]),
        ),
        (
            1800,
            Step::Animate(vec![
                anim(10, Effect::FlyOut(Down), 0, 600),
                anim(11, Effect::FadeOut, 150, 600),
                anim(12, Effect::WipeOut(Right), 300, 600),
                anim(13, Effect::ZoomOut, 450, 600),
                anim(14, Effect::Disappear, 600, 0),
            ]),
        ),
        (1500, Step::Lyric(0, t(TransitionKind::Fade, 500))),
        (1200, Step::SafeArea(Some(area))),
        (
            1500,
            Step::Calibrate(Some(CalibrationPattern {
                current: area,
                max_percent: 10,
            })),
        ),
        (1500, Step::Calibrate(None)),
        (500, Step::SafeArea(None)),
        (800, Step::Panic(Role::Stream)),
        (1500, Step::Unplug(Role::Stream)),
        (600, Step::Lyric(1, t(TransitionKind::Fade, 400))),
        (1500, Step::Replug(Role::Stream)),
        (1500, Step::Latency(40)),
        (500, Step::End),
    ]);
    steps
}

struct App {
    outputs: Outputs,
    ids: HashMap<Role, OutputId>,
    windows: HashMap<Role, OutputWindow>,
    fullscreen: bool,
    looping: bool,
    script: Vec<(u64, Step)>,
    next: usize,
    next_at: Option<Instant>,
    latency: Latency,
    measuring: Option<Measure>,
    rng: u64,
    slide: usize,
    snapshots: Option<PathBuf>,
    snap_at: Option<(Instant, usize)>,
}

struct Measure {
    left: usize,
    sent: Option<Instant>,
    waiting: usize,
    next_at: Instant,
}

impl App {
    fn send_all(&self, f: impl Fn(Role) -> OutputCommand) {
        for role in ROLES {
            self.outputs.send(self.ids[&role], f(role));
        }
    }

    fn open(&mut self, event_loop: &ActiveEventLoop, role: Role) {
        let index = ROLES.iter().position(|r| *r == role).unwrap();
        let mut monitor = None;
        if self.fullscreen {
            let displays = support::displays(event_loop);
            let primary = event_loop.primary_monitor();
            // Outputs claim the non-primary displays in order; in the app
            // the `DisplayMatch` comes from the saved output config.
            let candidates: Vec<_> = displays
                .iter()
                .filter(|(_, m)| Some(m) != primary.as_ref())
                .collect();
            if let Some((info, _)) = candidates.get(index) {
                let wants: Vec<(OutputId, DisplayMatch)> = vec![(self.ids[&role], info.to_match())];
                let infos: Vec<_> = displays.iter().map(|(i, _)| i.clone()).collect();
                if let Some(&d) = assign_displays(&wants, &infos).get(&self.ids[&role]) {
                    monitor = Some(displays[d].1.clone());
                }
            }
        }
        let size = match role {
            Role::Audience => (800, 450),
            _ => (560, 315),
        };
        let x = [40, 860, 1440][index];
        match support::open(
            event_loop,
            &self.outputs,
            self.ids[&role],
            &format!("Lampstand {role:?}"),
            monitor,
            size,
            Some((x, 60)),
        ) {
            Ok(w) => {
                self.windows.insert(role, w);
            }
            Err(e) => eprintln!("could not open the {role:?} window: {e:?}"),
        }
    }

    fn snapshot(&self, step: usize) {
        let Some(dir) = &self.snapshots else { return };
        for role in ROLES {
            if !self.windows.contains_key(&role) {
                continue;
            }
            match self.outputs.snapshot(self.ids[&role]) {
                Ok(image) => {
                    let path = dir.join(format!("step{step:02}-{role:?}.png").to_lowercase());
                    if let Err(e) = image::save_buffer(
                        &path,
                        &image.pixels,
                        image.width,
                        image.height,
                        image::ColorType::Rgba8,
                    ) {
                        eprintln!("cannot save {}: {e}", path.display());
                    }
                }
                Err(e) => eprintln!("snapshot of {role:?} failed: {e:?}"),
            }
        }
    }

    fn run(&mut self, event_loop: &ActiveEventLoop, step: Step) {
        println!("step {}: {step:?}", self.next);
        if self.snapshots.is_some() && !matches!(step, Step::Latency(_) | Step::End) {
            self.snap_at = Some((Instant::now() + Duration::from_millis(350), self.next));
        }
        match step {
            Step::Lyric(slide, transition) => {
                self.slide = slide;
                self.send_all(|role| OutputCommand::Show {
                    scene: lyric_scene(role, slide),
                    transition,
                });
            }
            Step::Build => {
                let scene = build_scene();
                self.send_all(|_| OutputCommand::Show {
                    scene: scene.clone(),
                    transition: Transition {
                        kind: TransitionKind::Fade,
                        duration_ms: 500,
                    },
                });
            }
            Step::Animate(animations) => {
                self.send_all(|_| OutputCommand::Animate(animations.clone()));
            }
            Step::SafeArea(area) => self.outputs.send(
                self.ids[&Role::Audience],
                OutputCommand::SafeAreaOverlay(area),
            ),
            Step::Calibrate(pattern) => self
                .outputs
                .send(self.ids[&Role::Stage], OutputCommand::Calibrate(pattern)),
            Step::Panic(role) => {
                self.outputs.inject_panic(Some(self.ids[&role]));
                // Any command makes it draw, and panic.
                self.outputs.send(
                    self.ids[&role],
                    OutputCommand::Show {
                        scene: lyric_scene(role, self.slide),
                        transition: Transition::CUT,
                    },
                );
            }
            Step::Unplug(role) => {
                // As when its display disappears: the window goes, the
                // others must not notice.
                self.outputs.detach(self.ids[&role]);
                self.windows.remove(&role);
            }
            Step::Replug(role) => self.open(event_loop, role),
            Step::Latency(rounds) => {
                self.measuring = Some(Measure {
                    left: rounds,
                    sent: None,
                    waiting: 0,
                    next_at: Instant::now(),
                });
            }
            Step::End => {
                let refresh = self
                    .windows
                    .values()
                    .next()
                    .map_or(1000.0 / 60.0, |w| support::refresh_ms(&w.window));
                self.latency
                    .report("click-to-photon, three outputs", refresh);
                if self.looping {
                    self.next = 0;
                    self.latency = Latency::default();
                } else {
                    event_loop.exit();
                }
            }
        }
    }

    fn random_ms(&mut self, max: u64) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng % max
    }
}

impl ApplicationHandler<OutputEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !self.windows.is_empty() {
            return;
        }
        for role in ROLES {
            self.open(event_loop, role);
        }
        self.next_at = Some(Instant::now() + Duration::from_millis(500));
    }

    fn user_event(&mut self, _: &ActiveEventLoop, event: OutputEvent) {
        match event {
            OutputEvent::FirstFrame(id) => {
                if let Some(w) = self.windows.values_mut().find(|w| w.id == id) {
                    w.on_first_frame();
                }
            }
            OutputEvent::Presented(timing) => {
                let Some(m) = &mut self.measuring else { return };
                let Some(sent) = m.sent else { return };
                if timing.sent < sent || m.waiting == 0 {
                    return;
                }
                self.latency.push(&timing);
                m.waiting -= 1;
                if m.waiting == 0 {
                    m.sent = None;
                    m.left -= 1;
                    let gap = 40 + self.random_ms(60);
                    let m = self.measuring.as_mut().unwrap();
                    m.next_at = Instant::now() + Duration::from_millis(gap);
                    if m.left == 0 {
                        self.measuring = None;
                    }
                }
            }
            OutputEvent::SurfaceLost { output, reason } => {
                println!("surface lost on {output:?}: {reason}");
            }
            OutputEvent::Restarted {
                output,
                message,
                blanked,
            } => println!("render thread restarted ({output:?}, blanked: {blanked}): {message}"),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested
            | WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(w) = self.windows.values().find(|w| w.window.id() == id) {
                    self.outputs.resize(w.id, size.width, size.height);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if let Some((at, step)) = self.snap_at {
            if now >= at {
                self.snap_at = None;
                self.snapshot(step);
            }
        }
        if let Some(m) = &mut self.measuring {
            if m.sent.is_none() && now >= m.next_at {
                m.sent = Some(now);
                m.waiting = self.windows.len();
                self.slide += 1;
                let slide = self.slide;
                self.send_all(|role| OutputCommand::Show {
                    scene: lyric_scene(role, slide),
                    transition: Transition::CUT,
                });
            }
        } else if let Some(at) = self.next_at {
            if now >= at && self.next < self.script.len() {
                let (_, step) = self.script[self.next].clone();
                self.next += 1;
                self.run(event_loop, step);
                self.next_at = self
                    .script
                    .get(self.next)
                    .map(|(delay, _)| Instant::now() + Duration::from_millis(*delay));
            }
        }
        let mut wake = match (&self.measuring, self.next_at) {
            (Some(m), _) if m.sent.is_none() => m.next_at,
            (Some(_), _) => now + Duration::from_millis(100),
            (None, Some(at)) => at,
            (None, None) => now + Duration::from_millis(250),
        };
        if let Some((at, _)) = self.snap_at {
            wake = wake.min(at);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(wake.max(now)));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let event_loop = EventLoop::<OutputEvent>::with_user_event()
        .build()
        .expect("an event loop (is there a display?)");
    let renderer = Renderer::new(Arc::new(NoBlobs)).expect("a GPU or software adapter");
    println!("rendering on {:?}", renderer.adapter_info());
    let proxy = event_loop.create_proxy();
    let outputs = Outputs::spawn(renderer, OutputOptions::default(), move |e| {
        let _ = proxy.send_event(e);
    })
    .expect("the render thread");
    let ids: HashMap<Role, OutputId> = ROLES.iter().map(|r| (*r, OutputId::generate())).collect();
    // Commands may go before the windows exist; each window's first frame
    // shows them.
    for role in ROLES {
        outputs.send(
            ids[&role],
            OutputCommand::Show {
                scene: lyric_scene(role, 0),
                transition: Transition::CUT,
            },
        );
    }
    let mut app = App {
        outputs,
        ids,
        windows: HashMap::new(),
        fullscreen: args.iter().any(|a| a == "--fullscreen"),
        looping: args.iter().any(|a| a == "--loop"),
        script: script(),
        next: 0,
        next_at: None,
        latency: Latency::default(),
        measuring: None,
        rng: 0x2545_F491_4F6C_DD1D,
        slide: 0,
        snapshots: args
            .iter()
            .position(|a| a == "--snapshots")
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from),
        snap_at: None,
    };
    if let Some(dir) = &app.snapshots {
        std::fs::create_dir_all(dir).expect("the snapshot directory");
    }
    event_loop.run_app(&mut app).expect("the event loop");
    let ids: Vec<OutputId> = app.windows.values().map(|w| w.id).collect();
    for id in ids {
        app.outputs.detach(id);
    }
}
