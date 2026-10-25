//! The two opt-in switches that reach past Lampstand's own windows (#140):
//! "Start Lampstand when this computer starts" and "Keep this computer
//! awake while it is showing the screens". Both are off by default, kept on
//! this device, and the second follows whether a screen is showing. A
//! recording `Machine` stands in for the operating system; what the real
//! one does on macOS and Windows needs those computers.

use std::path::Path;
use std::sync::{Arc, Mutex};

use lampstand_api::{DeviceSettings, OutputRequest, Request, Response, SettingsRequest};
use lampstand_app::handler::{App, Machine, Placement, WindowHost};
use lampstand_core::output::{BackgroundMode, OutputConfig, OutputId, OutputRole, OutputTarget};
use lampstand_core::safe_area::SafeArea;
use lampstand_layout::Fonts;
use lampstand_render::output::DisplayInfo;
use lampstand_render::{OutputCommand, OutputSink};

struct Sink;
impl OutputSink for Sink {
    fn send(&self, _: OutputId, _: OutputCommand) {}
}

struct Host;
impl WindowHost for Host {
    fn displays(&self) -> Vec<DisplayInfo> {
        Vec::new()
    }
    fn open(&mut self, _: OutputId, _: &str, _: Placement) -> Result<(), String> {
        Ok(())
    }
    fn close(&mut self, _: OutputId) {}
}

/// What the computer was asked, in order.
#[derive(Default)]
struct Asked {
    login: Vec<bool>,
    awake: Vec<bool>,
    /// The next startup change fails with this.
    refuse: Option<String>,
}

#[derive(Clone, Default)]
struct Computer(Arc<Mutex<Asked>>);

impl Machine for Computer {
    fn set_start_at_login(&mut self, on: bool) -> Result<(), String> {
        let mut asked = self.0.lock().unwrap();
        match asked.refuse.take() {
            Some(why) => Err(why),
            None => {
                asked.login.push(on);
                Ok(())
            }
        }
    }
    fn set_awake(&mut self, on: bool) {
        self.0.lock().unwrap().awake.push(on);
    }
}

fn open(dir: &Path, computer: &Computer) -> App {
    let mut app = App::open(
        dir,
        Fonts::bundled_only(),
        Arc::new(Sink),
        Box::new(Host),
        Arc::new(|_| {}),
    )
    .unwrap();
    app.set_machine(Box::new(computer.clone()));
    app
}

fn output(target: OutputTarget) -> OutputConfig {
    OutputConfig {
        id: OutputId::generate(),
        name: "Rehearsal".into(),
        role: OutputRole::Audience { primary: true },
        target,
        safe_area: SafeArea::default(),
        background: BackgroundMode::Bleed,
        theme: None,
        idle: None,
        enabled: true,
    }
}

fn window() -> OutputConfig {
    output(OutputTarget::Window {
        width: 960,
        height: 540,
    })
}

fn settings(app: &mut App, request: SettingsRequest) -> Result<DeviceSettings, String> {
    match app.handle(Request::Settings(request)) {
        Response::Settings(s) => Ok(s),
        Response::Error { message } => Err(message),
        other => panic!("{other:?}"),
    }
}

fn put(app: &mut App, output: OutputConfig) {
    if let Response::Error { message } = app.handle(Request::Outputs(OutputRequest::Put(output))) {
        panic!("{message}");
    }
}

#[test]
fn both_switches_are_off_by_default_and_ask_nothing_of_the_computer() {
    let dir = tempfile::tempdir().unwrap();
    let computer = Computer::default();
    let mut app = open(dir.path(), &computer);
    put(&mut app, window());
    let s = settings(&mut app, SettingsRequest::Get).unwrap();
    assert!(!s.start_at_login && !s.keep_awake);
    let asked = computer.0.lock().unwrap();
    assert!(asked.login.is_empty() && asked.awake.is_empty());
}

#[test]
fn the_computer_stays_awake_only_while_a_screen_is_showing() {
    let dir = tempfile::tempdir().unwrap();
    let computer = Computer::default();
    let mut app = open(dir.path(), &computer);
    let on = |app: &mut App, keep_awake| {
        let mut s = settings(app, SettingsRequest::Get).unwrap();
        s.keep_awake = keep_awake;
        settings(app, SettingsRequest::Put(s)).unwrap();
    };

    // Switched on with no screen showing: nothing to keep awake for.
    on(&mut app, true);
    assert!(computer.0.lock().unwrap().awake.is_empty());

    // A rehearsal window opens: awake. A second one changes nothing.
    let first = window();
    put(&mut app, first.clone());
    assert_eq!(computer.0.lock().unwrap().awake, [true]);
    let second = window();
    put(&mut app, second.clone());
    assert_eq!(computer.0.lock().unwrap().awake, [true]);

    // One of two closes: a screen is still showing.
    app.output_closed(first.id);
    assert_eq!(computer.0.lock().unwrap().awake, [true]);

    // The last one closes: the computer may sleep again.
    app.output_closed(second.id);
    assert_eq!(computer.0.lock().unwrap().awake, [true, false]);

    // A browser-source picture counts as showing.
    let stream = output(OutputTarget::BrowserSource {
        width: 1920,
        height: 1080,
    });
    put(&mut app, stream.clone());
    assert_eq!(computer.0.lock().unwrap().awake, [true, false, true]);

    // Switching it off lets the computer sleep at once, screens or not.
    on(&mut app, false);
    assert_eq!(computer.0.lock().unwrap().awake, [true, false, true, false]);
    put(&mut app, window());
    assert_eq!(computer.0.lock().unwrap().awake.len(), 4);
}

#[test]
fn a_saved_keep_awake_applies_when_the_app_starts_with_screens_showing() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut app = open(dir.path(), &Computer::default());
        put(&mut app, window());
        let mut s = settings(&mut app, SettingsRequest::Get).unwrap();
        s.keep_awake = true;
        settings(&mut app, SettingsRequest::Put(s)).unwrap();
    }
    let computer = Computer::default();
    let _app = open(dir.path(), &computer);
    assert_eq!(computer.0.lock().unwrap().awake, [true]);
}

#[test]
fn start_at_login_is_asked_of_the_computer_when_changed_and_kept_on_this_device() {
    let dir = tempfile::tempdir().unwrap();
    let computer = Computer::default();
    let mut app = open(dir.path(), &computer);
    let mut s = settings(&mut app, SettingsRequest::Get).unwrap();
    s.start_at_login = true;
    s.keep_awake = true;
    assert_eq!(
        settings(&mut app, SettingsRequest::Put(s.clone())).unwrap(),
        s
    );
    // Saving again with the same value does not ask again.
    settings(&mut app, SettingsRequest::Put(s.clone())).unwrap();
    assert_eq!(computer.0.lock().unwrap().login, [true]);

    // In this device's own file, not in anything that syncs.
    let saved = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();
    assert!(saved.contains("\"start_at_login\": true"), "{saved}");
    assert!(saved.contains("\"keep_awake\": true"), "{saved}");
    drop(app);

    // Still on after a restart, and the startup item is made so again (the
    // app may have moved, or been updated).
    let computer = Computer::default();
    let mut app = open(dir.path(), &computer);
    assert_eq!(settings(&mut app, SettingsRequest::Get).unwrap(), s);
    assert_eq!(computer.0.lock().unwrap().login, [true]);

    s.start_at_login = false;
    settings(&mut app, SettingsRequest::Put(s)).unwrap();
    assert_eq!(computer.0.lock().unwrap().login, [true, false]);
}

#[test]
fn a_computer_that_refuses_the_startup_item_leaves_the_switch_off() {
    let dir = tempfile::tempdir().unwrap();
    let computer = Computer::default();
    let mut app = open(dir.path(), &computer);
    computer.0.lock().unwrap().refuse = Some("permission denied".into());
    let mut s = settings(&mut app, SettingsRequest::Get).unwrap();
    s.start_at_login = true;
    let message = settings(&mut app, SettingsRequest::Put(s)).unwrap_err();
    assert!(message.contains("could not be set to start"), "{message}");
    assert!(message.contains("permission denied"), "{message}");
    assert!(
        !settings(&mut app, SettingsRequest::Get)
            .unwrap()
            .start_at_login
    );
    drop(app);
    let mut app = open(dir.path(), &Computer::default());
    assert!(
        !settings(&mut app, SettingsRequest::Get)
            .unwrap()
            .start_at_login
    );
}
