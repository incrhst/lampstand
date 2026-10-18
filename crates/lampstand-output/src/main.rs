//! lampstand-output: the projector process. See the crate docs and `--help`.

use std::sync::Arc;
use std::time::Duration;

use lampstand_output::args::{default_displays, Args, USAGE};
use lampstand_output::calibration;
use lampstand_output::frame::{accept_revision, prefetch_urls, Frame};
use lampstand_output::images::{fetch, ImageCache, ImageKey, Lookup};
use lampstand_output::keys::{action, Action};
use lampstand_output::net;
use lampstand_output::Event;
use lampstand_render::calibration::Edge;
use lampstand_render::window::{displays, open_outputs, DisplayInfo, OutputWindow};
use lampstand_render::winit::application::ApplicationHandler;
use lampstand_render::winit::event::{ElementState, KeyEvent, WindowEvent};
use lampstand_render::winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use lampstand_render::winit::keyboard::{Key, ModifiersState, NamedKey};
use lampstand_render::winit::window::WindowId;
use lampstand_render::{Content, Output, SafeArea, Slide};
use lampstand_show::protocol::{Command, Snapshot};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

/// Size of a `--windowed` output, and of the window used when there is only
/// the operator's display.
const WINDOW_SIZE: (u32, u32) = (1280, 720);
/// Decoded images kept per window: live, next, preview, logo and a few back.
const IMAGES_PER_WINDOW: usize = 8;
/// Click-to-frame budget; slower state-to-frame times are logged.
const BUDGET: Duration = Duration::from_millis(50);

macro_rules! log {
    ($($t:tt)*) => { eprintln!("lampstand-output: {}", format!($($t)*)) };
}

struct Screen {
    window: OutputWindow,
    /// Key in the calibration file.
    name: String,
    safe: SafeArea,
    /// The edge being adjusted while the calibration grid is up.
    calibrating: Option<Edge>,
    /// The frame on screen, and whether its image had arrived.
    shown: Option<(Frame, bool)>,
}

impl Screen {
    fn width(&self) -> u32 {
        self.window.output().width
    }

    fn set_slide(&mut self, slide: Slide) {
        if let Err(e) = self.window.set_slide(slide) {
            log!("render: {e}");
        }
    }

    fn set_safe_area(&mut self) {
        let output = Output { safe_area: self.safe, ..self.window.output().clone() };
        if let Err(e) = self.window.set_output(output) {
            log!("render: {e}");
        }
    }

    fn show_calibration(&mut self, edge: Edge) {
        self.set_safe_area();
        let label = format!(
            "{}\nArrows move the white edge · Tab next edge · R reset\nC or Esc: save and leave calibration",
            self.name
        );
        self.set_slide(Slide { content: Content::Calibration { label, active: Some(edge) }, ..Default::default() });
    }
}

struct App {
    args: Args,
    http: String,
    proxy: EventLoopProxy<Event>,
    runtime: tokio::runtime::Handle,
    commands: UnboundedSender<Command>,
    screens: Vec<Screen>,
    snapshot: Option<Snapshot>,
    revision: Option<u64>,
    images: ImageCache,
    modifiers: ModifiersState,
}

impl App {
    fn open(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let connected = displays(event_loop);
        let primary = event_loop.primary_monitor();
        let primary_index = connected.iter().position(|d| Some(&d.monitor) == primary.as_ref());
        let (w, h) = WINDOW_SIZE;
        let preview = |index| DisplayInfo::preview(event_loop, index, w, h).ok_or("no displays found".to_string());

        let mut chosen = Vec::new();
        if self.args.displays.is_empty() {
            let defaults = default_displays(connected.len(), primary_index);
            if defaults.is_empty() {
                log!("only the operator's display is connected; showing the output in a window");
                chosen.push(preview(0)?);
            }
            for i in defaults {
                chosen.push(if self.args.windowed { preview(i)? } else { connected[i].clone() });
            }
        } else {
            for &i in &self.args.displays {
                match connected.iter().find(|d| d.index == i) {
                    _ if self.args.windowed => chosen.push(preview(i)?),
                    Some(d) => chosen.push(d.clone()),
                    None => log!("display {i} is not connected; skipping it (see --list)"),
                }
            }
        }
        if chosen.is_empty() {
            return Err("no output displays".into());
        }

        let targets: Vec<_> = chosen
            .into_iter()
            .map(|display| {
                let safe = calibration::load(&self.args.calibration, &display.name).unwrap_or_default();
                let output = Output::new(display.size.0, display.size.1).with_safe_area(safe);
                (display, output, Slide::default())
            })
            .collect();
        let (_, windows) = open_outputs(event_loop, targets).map_err(|e| format!("could not open outputs: {e}"))?;
        for window in windows {
            let name = window.display().name.clone();
            log!("showing on {name} ({}x{})", window.output().width, window.output().height);
            let safe = window.output().safe_area;
            self.screens.push(Screen { window, name, safe, calibrating: None, shown: None });
        }
        self.images = ImageCache::new(IMAGES_PER_WINDOW * self.screens.len());
        if let Some(first) = self.screens.first() {
            first.window.window().focus_window();
        }
        Ok(())
    }

    fn on_state(&mut self, snapshot: Snapshot) {
        if !accept_revision(self.revision, snapshot.revision) {
            return;
        }
        self.revision = Some(snapshot.revision);
        self.images.retry_failed();
        self.snapshot = Some(snapshot);
        self.refresh_all();
        self.prefetch();
    }

    fn prefetch(&mut self) {
        let Some(snapshot) = &self.snapshot else { return };
        let urls = prefetch_urls(snapshot);
        let widths: Vec<u32> = self.screens.iter().map(Screen::width).collect();
        for url in urls {
            for &width in &widths {
                self.request(ImageKey { url: url.clone(), width });
            }
        }
    }

    /// Start fetching `key` unless it is cached or on its way.
    fn request(&mut self, key: ImageKey) {
        if !self.images.start(&key) {
            return;
        }
        let url = key.request_url(&self.http);
        let proxy = self.proxy.clone();
        self.runtime.spawn_blocking(move || {
            let result = fetch(&url).map(Arc::new);
            let _ = proxy.send_event(Event::Image { key, result });
        });
    }

    fn refresh_all(&mut self) {
        for i in 0..self.screens.len() {
            self.refresh(i);
        }
    }

    /// Put the current frame on screen `i`. An image frame whose image is
    /// still on its way leaves the previous frame up until it arrives.
    fn refresh(&mut self, i: usize) {
        if self.screens[i].calibrating.is_some() {
            return;
        }
        let frame = self.snapshot.as_ref().map_or(Frame::Black, Frame::from_snapshot);
        let (image, resolved) = match frame.image_url() {
            None => (None, true),
            Some(url) => {
                let key = ImageKey { url: url.to_string(), width: self.screens[i].width() };
                match self.images.get(&key) {
                    Lookup::Ready(image) => (Some(image), true),
                    Lookup::Failed => (None, false),
                    Lookup::Pending => return,
                    Lookup::Missing => return self.request(key),
                }
            }
        };
        let screen = &mut self.screens[i];
        if screen.shown.as_ref() == Some(&(frame.clone(), resolved)) {
            return;
        }
        screen.set_slide(frame.slide(image));
        screen.shown = Some((frame, resolved));
    }

    fn command(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    fn leave_calibration(&mut self, i: usize) {
        let screen = &mut self.screens[i];
        if screen.calibrating.take().is_none() {
            return;
        }
        let (w, h) = (screen.window.output().width, screen.window.output().height);
        match calibration::save(&self.args.calibration, &screen.name, w, h, screen.safe) {
            Ok(()) => log!("saved margins for {} to {}", screen.name, self.args.calibration.display()),
            Err(e) => log!("could not save margins to {}: {e}", self.args.calibration.display()),
        }
        screen.set_safe_area();
        screen.shown = None;
        self.refresh(i);
    }

    fn key(&mut self, i: usize, key: &Key, repeat: bool) {
        let shift = self.modifiers.shift_key();
        if let Some(mut edge) = self.screens[i].calibrating {
            let screen = &mut self.screens[i];
            match key {
                Key::Named(NamedKey::Escape) => return self.leave_calibration(i),
                Key::Character(c) if c.eq_ignore_ascii_case("c") => return self.leave_calibration(i),
                Key::Character(c) if c.eq_ignore_ascii_case("r") => screen.safe = SafeArea::default(),
                Key::Named(NamedKey::Tab) => edge = calibration::next_edge(edge, shift),
                Key::Named(k @ (NamedKey::ArrowUp | NamedKey::ArrowDown | NamedKey::ArrowLeft | NamedKey::ArrowRight)) => {
                    calibration::arrow(&mut screen.safe, &mut edge, *k, shift)
                }
                _ => return,
            }
            screen.calibrating = Some(edge);
            screen.show_calibration(edge);
            return;
        }
        // Holding a key down must not run through the service.
        if repeat {
            return;
        }
        match action(key) {
            Some(Action::Command(command)) => self.command(command),
            Some(Action::Calibrate) => {
                let screen = &mut self.screens[i];
                screen.calibrating = Some(Edge::Top);
                screen.show_calibration(Edge::Top);
            }
            None => {}
        }
    }
}

impl ApplicationHandler<Event> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !self.screens.is_empty() {
            return;
        }
        if let Err(e) = self.open(event_loop) {
            log!("{e}");
            event_loop.exit();
        }
    }

    fn user_event(&mut self, _: &ActiveEventLoop, event: Event) {
        match event {
            Event::Connected => {
                log!("connected to {}", self.args.server);
                // A restarted server may count revisions from zero again.
                self.revision = None;
            }
            Event::Disconnected => log!("server went away; keeping the last frame and reconnecting"),
            Event::State(snapshot) => self.on_state(*snapshot),
            Event::Image { key, result } => {
                if let Err(e) = &result {
                    log!("image: {e}");
                }
                self.images.finish(key, result);
                self.refresh_all();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(i) = self.screens.iter().position(|s| s.window.id() == id) else { return };
        match &event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                return;
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event: KeyEvent { logical_key, state: ElementState::Pressed, repeat, .. }, .. } => {
                self.key(i, logical_key, *repeat)
            }
            _ => {}
        }
        let resized = matches!(event, WindowEvent::Resized(_));
        match self.screens[i].window.handle_event(&event) {
            Ok(Some(took)) if took > BUDGET => log!("frame took {} ms to reach the screen", took.as_millis()),
            Ok(Some(took)) if self.args.verbose => {
                let screen = &self.screens[i];
                let what = screen.shown.as_ref().map_or("calibration".to_string(), |(f, _)| format!("{f:?}"));
                log!("{}: {what} on screen in {:.1} ms", screen.name, took.as_secs_f64() * 1000.0)
            }
            Ok(_) => {}
            Err(e) => log!("render: {e}"),
        }
        if resized {
            // Images are fetched at the window's width.
            self.screens[i].shown = None;
            self.refresh(i);
            self.prefetch();
        }
    }

    /// However the process ends, keep margins being adjusted.
    fn exiting(&mut self, _: &ActiveEventLoop) {
        for i in 0..self.screens.len() {
            self.leave_calibration(i);
        }
    }
}

/// Prints the displays and exits.
struct Lister;

impl ApplicationHandler<Event> for Lister {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let primary = event_loop.primary_monitor();
        for d in displays(event_loop) {
            println!(
                "--display {}  {}  {}x{} at {:?}, scale {}, {} Hz{}",
                d.index,
                d.name,
                d.size.0,
                d.size.1,
                d.position,
                d.scale_factor,
                d.refresh_hz.map_or("?".into(), |r| format!("{r:.0}")),
                if Some(&d.monitor) == primary.as_ref() { "  (primary: the operator's)" } else { "" }
            );
        }
        event_loop.exit();
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}

fn main() {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(e) if e == USAGE => return println!("{USAGE}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let event_loop = match EventLoop::<Event>::with_user_event().build() {
        Ok(event_loop) => event_loop,
        Err(e) => {
            log!("no event loop: {e}");
            std::process::exit(1);
        }
    };
    if args.list {
        event_loop.run_app(&mut Lister).expect("run");
        return;
    }
    let http = match net::http_base(&args.server) {
        Ok(http) => http,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(4)
        .enable_all()
        .build()
        .expect("tokio runtime");
    let (commands, receiver) = unbounded_channel();
    let proxy = event_loop.create_proxy();
    let sender = proxy.clone();
    let name = std::env::var("HOSTNAME").map_or_else(|_| "Projector".to_string(), |h| format!("Projector on {h}"));
    runtime.spawn(net::run(args.server.clone(), name, receiver, move |event| sender.send_event(event).is_ok()));

    let mut app = App {
        args,
        http,
        proxy,
        runtime: runtime.handle().clone(),
        commands,
        screens: Vec::new(),
        snapshot: None,
        revision: None,
        images: ImageCache::new(IMAGES_PER_WINDOW),
        modifiers: ModifiersState::empty(),
    };
    let result = event_loop.run_app(&mut app);
    let failed = app.screens.is_empty();
    drop(app);
    runtime.shutdown_timeout(Duration::from_millis(200));
    if let Err(e) = result {
        log!("{e}");
        std::process::exit(1);
    }
    if failed {
        std::process::exit(1);
    }
}
