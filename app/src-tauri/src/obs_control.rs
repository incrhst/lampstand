//! Running OBS from Lampstand (T89; docs/decisions/0005): where OBS is,
//! whether it is connected, streaming or recording, and, when it isn't
//! connected, why.
//!
//! - **Settings** (host, port, password) live in `obs.json` in the app-data
//!   folder, written owner-only on Unix. They never sync, and the password
//!   never goes to the UI: `ObsView` only says whether one is saved.
//! - **One connection** through `connectors::obs`, started with the app.
//!   It runs on Tauri's async runtime and never locks `App`: whether any
//!   output streams comes from `App::streaming`, an atomic flag.
//! - **The view** (`ObsView`) is pushed as `Event::Obs` whenever it
//!   changes. While OBS streams or records, it is refreshed every two
//!   seconds for the timecode and dropped frames.
//! - **Why OBS isn't connected.** When OBS is on this machine and doesn't
//!   answer, Lampstand looks for it ([`detect`]) and says whether it isn't
//!   installed, isn't running, or is running with its WebSocket server off
//!   ([`diagnose`]). It looks again every few seconds while disconnected,
//!   because starting OBS with its server off changes the answer without
//!   any connection.
//! - **Launch** starts OBS minimised to the tray. Lampstand never edits
//!   OBS's own settings.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lampstand_api::{
    Event, ObsConnection, ObsOutputView, ObsProblem, ObsRequest, ObsView, Response,
};
use lampstand_connectors::obs::{Obs, ObsConfig, ObsEvent, ObsStatus};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::handler::EventSink;

/// How long "Save and test" waits for the connection to settle.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(6);
/// How often the timecode and dropped frames refresh while OBS is busy.
const BUSY_REFRESH: Duration = Duration::from_secs(2);
/// How often to look for OBS again while it isn't connected.
const DETECT_REFRESH: Duration = Duration::from_secs(5);

/// `obs.json`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObsSettings {
    pub host: String,
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

impl Default for ObsSettings {
    fn default() -> Self {
        let config = ObsConfig::default();
        Self {
            host: config.host,
            port: config.port,
            password: None,
        }
    }
}

impl std::fmt::Debug for ObsSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObsSettings")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("password", &self.password.as_ref().map(|_| "…"))
            .finish()
    }
}

impl ObsSettings {
    fn config(&self) -> ObsConfig {
        ObsConfig {
            host: self.host.clone(),
            port: self.port,
            password: self.password.clone(),
        }
    }
}

/// `obs.json` in the app-data folder. `saved` is false until someone saves
/// settings, so a church that never set OBS up gets no OBS warnings.
pub struct SettingsStore {
    path: PathBuf,
    settings: ObsSettings,
    saved: bool,
}

impl SettingsStore {
    /// Reads `path`, or the defaults if it is missing or unreadable (the
    /// operator can type the settings again).
    pub fn load(path: PathBuf) -> Self {
        let (settings, saved) = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(settings) => (settings, true),
                Err(e) => {
                    eprintln!(
                        "lampstand: {} is unreadable ({e}); using defaults",
                        path.display()
                    );
                    (ObsSettings::default(), false)
                }
            },
            Err(_) => (ObsSettings::default(), false),
        };
        Self {
            path,
            settings,
            saved,
        }
    }

    pub fn settings(&self) -> &ObsSettings {
        &self.settings
    }

    pub fn saved(&self) -> bool {
        self.saved
    }

    /// Saves new settings. `password: None` keeps the saved password, and
    /// an empty one clears it.
    pub fn put(&mut self, host: String, port: u16, password: Option<String>) -> io::Result<()> {
        let password = match password {
            None => self.settings.password.clone(),
            Some(p) if p.is_empty() => None,
            Some(p) => Some(p),
        };
        let settings = ObsSettings {
            host: host.trim().to_string(),
            port,
            password,
        };
        let json = serde_json::to_vec_pretty(&settings).map_err(io::Error::other)?;
        write_private(&self.path, &json)?;
        self.settings = settings;
        self.saved = true;
        Ok(())
    }
}

/// Writes `bytes` to `path` through a temporary file, readable only by
/// this user on Unix, because it holds the OBS password.
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write as _;
    let tmp = path.with_extension("tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, path)
}

// Finding OBS on this machine.

/// The operating systems OBS is looked for on. A parameter, so tests cover
/// every one on any machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Windows,
    Linux,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            Os::Linux
        }
    }

    /// `dir` and then `parts`, joined with this OS's separator rather than
    /// the host's, so another OS's paths come out as that OS writes them.
    fn join(self, dir: impl Into<OsString>, parts: &[&str]) -> PathBuf {
        let sep = match self {
            Os::Windows => '\\',
            Os::MacOs | Os::Linux => '/',
        };
        let mut path = dir.into().to_string_lossy().into_owned();
        for part in parts {
            if !path.ends_with(sep) {
                path.push(sep);
            }
            path.push_str(part);
        }
        PathBuf::from(path)
    }
}

/// How to start the OBS that was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launcher {
    /// `open -a <app>`.
    MacApp(PathBuf),
    /// The executable. On Windows OBS must start in its own folder.
    Exe(PathBuf),
    /// `flatpak run com.obsproject.Studio`.
    Flatpak,
}

const FLATPAK_ID: &str = "com.obsproject.Studio";

/// What [`detect`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// `None` when OBS isn't in any usual place.
    pub launcher: Option<Launcher>,
    /// `None` when the process list couldn't be read.
    pub running: Option<bool>,
}

/// The machine, as far as detection needs it. [`System`] is the real one.
pub trait Probe: Send + Sync {
    fn exists(&self, path: &Path) -> bool;
    fn env(&self, key: &str) -> Option<OsString>;
    /// Whether a process with this name is running; `None` if the process
    /// list can't be read.
    fn running(&self, os: Os, name: &str) -> Option<bool>;
    fn spawn(&self, program: &Path, args: &[&str], cwd: Option<&Path>) -> io::Result<()>;
}

/// The real machine.
pub struct System;

impl Probe for System {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn env(&self, key: &str) -> Option<OsString> {
        std::env::var_os(key)
    }

    fn running(&self, os: Os, name: &str) -> Option<bool> {
        use std::process::{Command, Stdio};
        match os {
            Os::Windows => {
                let out = Command::new("tasklist")
                    .args(["/FI", &format!("IMAGENAME eq {name}"), "/NH"])
                    .stderr(Stdio::null())
                    .output()
                    .ok()?;
                let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
                Some(text.contains(&name.to_lowercase()))
            }
            Os::MacOs | Os::Linux => {
                let status = Command::new("pgrep")
                    .args(["-x", name])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .ok()?;
                // pgrep: 0 found, 1 none, anything else an error.
                match status.code() {
                    Some(0) => Some(true),
                    Some(1) => Some(false),
                    _ => None,
                }
            }
        }
    }

    fn spawn(&self, program: &Path, args: &[&str], cwd: Option<&Path>) -> io::Result<()> {
        let mut command = std::process::Command::new(program);
        command.args(args);
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        command.spawn().map(drop)
    }
}

/// Looks for OBS in the usual places, and whether it is running.
pub fn detect(probe: &dyn Probe, os: Os) -> Found {
    let home = probe.env("HOME");
    let launcher = match os {
        Os::MacOs => {
            let mut candidates = vec![PathBuf::from("/Applications/OBS.app")];
            if let Some(home) = &home {
                candidates.push(os.join(home, &["Applications", "OBS.app"]));
            }
            candidates
                .into_iter()
                .find(|p| probe.exists(p))
                .map(Launcher::MacApp)
        }
        Os::Windows => ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"]
            .iter()
            .filter_map(|key| probe.env(key))
            .map(|dir| os.join(dir, &["obs-studio", "bin", "64bit", "obs64.exe"]))
            .find(|p| probe.exists(p))
            .map(Launcher::Exe),
        Os::Linux => {
            // Split on Linux's `:`, not the host's separator.
            let on_path = probe.env("PATH").and_then(|path| {
                path.to_string_lossy()
                    .split(':')
                    .filter(|dir| !dir.is_empty())
                    .map(|dir| os.join(dir, &["obs"]))
                    .find(|p| probe.exists(p))
            });
            let mut flatpaks = vec![os.join("/var/lib/flatpak/app", &[FLATPAK_ID])];
            if let Some(home) = &home {
                flatpaks.push(os.join(home, &[".local", "share", "flatpak", "app", FLATPAK_ID]));
            }
            on_path.map(Launcher::Exe).or_else(|| {
                flatpaks
                    .iter()
                    .any(|p| probe.exists(p))
                    .then_some(Launcher::Flatpak)
            })
        }
    };
    let process = match os {
        Os::MacOs => "OBS",
        Os::Windows => "obs64.exe",
        Os::Linux => "obs",
    };
    Found {
        launcher,
        running: probe.running(os, process),
    }
}

/// Starts OBS minimised to the tray.
pub fn launch(probe: &dyn Probe, os: Os) -> Result<(), String> {
    let Some(launcher) = detect(probe, os).launcher else {
        return Err("OBS isn't installed on this computer. Get it from obsproject.com.".into());
    };
    const ARGS: &[&str] = &["--minimize-to-tray"];
    let started = match &launcher {
        Launcher::MacApp(app) => {
            let app = app.to_string_lossy();
            probe.spawn(Path::new("open"), &["-a", &app, "--args", ARGS[0]], None)
        }
        Launcher::Exe(exe) => probe.spawn(exe, ARGS, exe.parent()),
        Launcher::Flatpak => probe.spawn(Path::new("flatpak"), &["run", FLATPAK_ID, ARGS[0]], None),
    };
    started.map_err(|e| format!("Could not start OBS: {e}"))
}

/// Whether `host` is this machine, where detection means something.
pub fn is_local(host: &str) -> bool {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Why OBS isn't connected, for the operator. `found` is only looked at
/// when OBS is on this machine and doesn't answer.
pub fn diagnose(
    connection: &ObsConnection,
    local: bool,
    found: Option<&Found>,
) -> Option<ObsProblem> {
    match connection {
        ObsConnection::Connecting | ObsConnection::Connected { .. } => None,
        ObsConnection::PasswordRequired => Some(ObsProblem::PasswordRequired),
        ObsConnection::WrongPassword => Some(ObsProblem::WrongPassword),
        ObsConnection::Disconnected { .. } if !local => Some(ObsProblem::Unreachable),
        ObsConnection::Disconnected { .. } => {
            let found = found?;
            Some(match (found.running, &found.launcher) {
                (Some(true), _) => ObsProblem::ServerOff,
                (_, None) => ObsProblem::NotInstalled,
                (_, Some(_)) => ObsProblem::NotRunning,
            })
        }
    }
}

fn connection(status: &ObsStatus) -> ObsConnection {
    match status {
        ObsStatus::Connecting => ObsConnection::Connecting,
        ObsStatus::Connected {
            obs_version,
            websocket_version,
        } => ObsConnection::Connected {
            obs_version: obs_version.clone(),
            websocket_version: websocket_version.clone(),
        },
        ObsStatus::Disconnected { reason } => ObsConnection::Disconnected {
            reason: reason.clone(),
        },
        ObsStatus::PasswordRequired => ObsConnection::PasswordRequired,
        ObsStatus::WrongPassword => ObsConnection::WrongPassword,
    }
}

/// "01:02:03.456" → "01:02:03".
fn whole_seconds(timecode: &str) -> String {
    timecode.split('.').next().unwrap_or_default().to_string()
}

// The controller.

/// OBS for the app: settings, the connection, and the view pushed to the UI.
pub struct ObsControl {
    obs: Obs,
    settings: Mutex<SettingsStore>,
    streaming: Arc<AtomicBool>,
    probe: Arc<dyn Probe>,
    os: Os,
    emit: EventSink,
    /// The last status other than `Connecting`. Each retry passes through
    /// `Connecting`; showing it would flicker the pill every few seconds,
    /// so a retry keeps showing this. Cleared by new settings.
    settled: Mutex<Option<ObsStatus>>,
    /// The last detection, while OBS isn't connected.
    found: Mutex<Option<Found>>,
    /// Streaming and recording, while connected.
    outputs: Mutex<(Option<ObsOutputView>, Option<ObsOutputView>)>,
    /// The last view pushed, so an unchanged one isn't sent again.
    told: Mutex<Option<ObsView>>,
}

impl ObsControl {
    /// Reads `obs.json` from `data_dir`, starts connecting and starts the
    /// watcher that pushes `Event::Obs`. Call inside the Tokio runtime.
    pub fn start(
        data_dir: &Path,
        streaming: Arc<AtomicBool>,
        probe: Arc<dyn Probe>,
        os: Os,
        emit: EventSink,
    ) -> Arc<Self> {
        let settings = SettingsStore::load(data_dir.join("obs.json"));
        let obs = Obs::start(settings.settings().config());
        let control = Arc::new(Self {
            obs,
            settings: Mutex::new(settings),
            streaming,
            probe,
            os,
            emit,
            settled: Mutex::new(None),
            found: Mutex::new(None),
            outputs: Mutex::new((None, None)),
            told: Mutex::new(None),
        });
        tokio::spawn(watch_obs(Arc::downgrade(&control), control.obs.status()));
        control
    }

    /// The connection, for tests and the typed calls later tickets add.
    pub fn obs(&self) -> &Obs {
        &self.obs
    }

    pub async fn handle(&self, request: ObsRequest) -> Response {
        let result = match request {
            ObsRequest::Status => {
                self.refresh_found().await;
                Ok(())
            }
            ObsRequest::Configure {
                host,
                port,
                password,
            } => self.configure(host, port, password).await,
            ObsRequest::Launch => self.launch().await,
        };
        match result {
            Ok(()) => Response::Obs(self.view()),
            Err(message) => Response::Error { message },
        }
    }

    async fn configure(
        &self,
        host: String,
        port: u16,
        password: Option<String>,
    ) -> Result<(), String> {
        if host.trim().is_empty() {
            return Err("Type where OBS is: localhost if it runs on this computer.".into());
        }
        if port == 0 {
            return Err("Type OBS's WebSocket port (4455 unless you changed it).".into());
        }
        let config = {
            let mut store = lock(&self.settings);
            store
                .put(host, port, password)
                .map_err(|e| format!("Could not save the OBS settings: {e}"))?;
            store.settings().config()
        };
        let mut status = self.obs.status();
        status.mark_unchanged();
        *lock(&self.settled) = None;
        self.obs.reconfigure(config);
        // The connection task reports Connecting first, then the outcome.
        let _ = tokio::time::timeout(SETTLE_TIMEOUT, async {
            let _ = status.changed().await;
            let _ = status
                .wait_for(|s| !matches!(s, ObsStatus::Connecting))
                .await;
        })
        .await;
        self.refresh_found().await;
        self.publish();
        Ok(())
    }

    async fn launch(&self) -> Result<(), String> {
        let probe = self.probe.clone();
        let os = self.os;
        tokio::task::spawn_blocking(move || launch(&*probe, os))
            .await
            .map_err(|e| e.to_string())??;
        // OBS takes a few seconds to open its WebSocket server; the
        // connection's own retries pick it up, and the view follows.
        self.refresh_found().await;
        Ok(())
    }

    /// Looks for OBS again if it isn't connected and is meant to be here.
    async fn refresh_found(&self) {
        let disconnected = matches!(self.shown_status(), ObsStatus::Disconnected { .. });
        let local = is_local(&lock(&self.settings).settings().host);
        let found = if disconnected && local {
            let probe = self.probe.clone();
            let os = self.os;
            tokio::task::spawn_blocking(move || detect(&*probe, os))
                .await
                .ok()
        } else {
            None
        };
        *lock(&self.found) = found;
    }

    /// Streaming and recording status, while connected. Failures leave the
    /// last known values: a slow answer mustn't flash the pill.
    async fn refresh_outputs(&self) {
        if !matches!(*self.obs.status().borrow(), ObsStatus::Connected { .. }) {
            *lock(&self.outputs) = (None, None);
            return;
        }
        let stream = self.obs.stream_status().await.ok().map(|s| ObsOutputView {
            active: s.active,
            reconnecting: s.reconnecting,
            timecode: if s.active {
                whole_seconds(&s.timecode)
            } else {
                String::new()
            },
            skipped_frames: s.skipped_frames,
            total_frames: s.total_frames,
        });
        let record = self.obs.record_status().await.ok().map(|r| ObsOutputView {
            active: r.active,
            reconnecting: false,
            timecode: if r.active {
                whole_seconds(&r.timecode)
            } else {
                String::new()
            },
            skipped_frames: 0,
            total_frames: 0,
        });
        let mut outputs = lock(&self.outputs);
        if stream.is_some() {
            outputs.0 = stream;
        }
        if record.is_some() {
            outputs.1 = record;
        }
    }

    pub fn view(&self) -> ObsView {
        let (settings, saved) = {
            let store = lock(&self.settings);
            (store.settings().clone(), store.saved())
        };
        let connection = connection(&self.shown_status());
        let found = lock(&self.found).clone();
        let problem = diagnose(&connection, is_local(&settings.host), found.as_ref());
        let (stream, record) = if matches!(connection, ObsConnection::Connected { .. }) {
            lock(&self.outputs).clone()
        } else {
            (None, None)
        };
        ObsView {
            host: settings.host,
            port: settings.port,
            has_password: settings.password.is_some(),
            connection,
            problem,
            relevant: saved || self.streaming.load(Ordering::Relaxed),
            stream,
            record,
        }
    }

    /// The status to show: the connection's, except that a retry keeps
    /// showing how the last attempt ended.
    fn shown_status(&self) -> ObsStatus {
        let status = self.obs.status().borrow().clone();
        let mut settled = lock(&self.settled);
        match status {
            ObsStatus::Connecting => settled.clone().unwrap_or(ObsStatus::Connecting),
            status => {
                *settled = Some(status.clone());
                status
            }
        }
    }

    /// Pushes the view if it changed.
    fn publish(&self) {
        let view = self.view();
        let mut told = lock(&self.told);
        if told.as_ref() != Some(&view) {
            *told = Some(view.clone());
            drop(told);
            (self.emit)(Event::Obs(view));
        }
    }

    fn busy(&self) -> bool {
        let outputs = lock(&self.outputs);
        outputs.0.as_ref().is_some_and(|s| s.active) || outputs.1.as_ref().is_some_and(|r| r.active)
    }
}

/// Follows the connection and OBS's events, and pushes the view when it
/// changes. Ends when the controller is dropped.
async fn watch_obs(control: std::sync::Weak<ObsControl>, mut status: watch::Receiver<ObsStatus>) {
    let mut events = match control.upgrade() {
        Some(c) => c.obs.events(),
        None => return,
    };
    let mut tick = tokio::time::interval(BUSY_REFRESH);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut since_detect = Duration::ZERO;
    loop {
        // What happened decides what to refresh.
        let (outputs, detect) = tokio::select! {
            changed = status.changed() => {
                if changed.is_err() {
                    return;
                }
                (true, true)
            }
            event = events.recv() => match event {
                Ok(ObsEvent::StreamStateChanged { .. } | ObsEvent::RecordStateChanged { .. }) => (true, false),
                Ok(_) => continue,
                // Missed some: refresh everything.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => (true, false),
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            },
            _ = tick.tick() => {
                since_detect += BUSY_REFRESH;
                let Some(c) = control.upgrade() else { return };
                let redetect = since_detect >= DETECT_REFRESH;
                (c.busy(), redetect)
            }
        };
        let Some(c) = control.upgrade() else { return };
        if detect {
            since_detect = Duration::ZERO;
            c.refresh_found().await;
        }
        if outputs {
            c.refresh_outputs().await;
        }
        c.publish();
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    /// A program started: what, with which arguments, in which folder.
    type Spawned = (PathBuf, Vec<String>, Option<PathBuf>);

    #[derive(Default)]
    struct Fake {
        files: HashSet<PathBuf>,
        env: HashMap<&'static str, OsString>,
        running: Option<bool>,
        spawned: Mutex<Vec<Spawned>>,
    }

    impl Fake {
        fn with_file(mut self, path: impl Into<PathBuf>) -> Self {
            self.files.insert(path.into());
            self
        }
        fn with_env(mut self, key: &'static str, value: &str) -> Self {
            self.env.insert(key, value.into());
            self
        }
        fn running(mut self, running: Option<bool>) -> Self {
            self.running = running;
            self
        }
    }

    impl Probe for Fake {
        fn exists(&self, path: &Path) -> bool {
            self.files.contains(path)
        }
        fn env(&self, key: &str) -> Option<OsString> {
            self.env.get(key).cloned()
        }
        fn running(&self, _: Os, _: &str) -> Option<bool> {
            self.running
        }
        fn spawn(&self, program: &Path, args: &[&str], cwd: Option<&Path>) -> io::Result<()> {
            self.spawned.lock().unwrap().push((
                program.to_path_buf(),
                args.iter().map(|a| a.to_string()).collect(),
                cwd.map(Path::to_path_buf),
            ));
            Ok(())
        }
    }

    #[test]
    fn finds_obs_on_macos() {
        let fake = Fake::default()
            .with_env("HOME", "/Users/pat")
            .with_file("/Users/pat/Applications/OBS.app")
            .running(Some(false));
        assert_eq!(
            detect(&fake, Os::MacOs),
            Found {
                launcher: Some(Launcher::MacApp("/Users/pat/Applications/OBS.app".into())),
                running: Some(false),
            }
        );
        launch(&fake, Os::MacOs).unwrap();
        assert_eq!(
            fake.spawned.lock().unwrap()[0],
            (
                PathBuf::from("open"),
                vec![
                    "-a".to_string(),
                    "/Users/pat/Applications/OBS.app".into(),
                    "--args".into(),
                    "--minimize-to-tray".into(),
                ],
                None,
            )
        );
    }

    #[test]
    fn finds_obs_on_windows_and_starts_it_in_its_folder() {
        let fake = Fake::default()
            .with_env("ProgramFiles", r"C:\Program Files")
            .with_file(r"C:\Program Files\obs-studio\bin\64bit\obs64.exe");
        let exe = PathBuf::from(r"C:\Program Files\obs-studio\bin\64bit\obs64.exe");
        assert_eq!(
            detect(&fake, Os::Windows).launcher,
            Some(Launcher::Exe(exe.clone()))
        );
        launch(&fake, Os::Windows).unwrap();
        let spawned = fake.spawned.lock().unwrap();
        assert_eq!(spawned[0].0, exe);
        assert_eq!(spawned[0].2.as_deref(), exe.parent());
    }

    #[test]
    fn finds_obs_on_linux_on_path_or_as_a_flatpak() {
        let on_path = Fake::default()
            .with_env("PATH", "/usr/local/bin:/usr/bin")
            .with_file("/usr/bin/obs");
        assert_eq!(
            detect(&on_path, Os::Linux).launcher,
            Some(Launcher::Exe("/usr/bin/obs".into()))
        );

        let flatpak = Fake::default()
            .with_env("PATH", "/usr/bin")
            .with_env("HOME", "/home/pat")
            .with_file("/home/pat/.local/share/flatpak/app/com.obsproject.Studio");
        assert_eq!(
            detect(&flatpak, Os::Linux).launcher,
            Some(Launcher::Flatpak)
        );
        launch(&flatpak, Os::Linux).unwrap();
        assert_eq!(
            flatpak.spawned.lock().unwrap()[0].1,
            ["run", "com.obsproject.Studio", "--minimize-to-tray"]
        );
    }

    #[test]
    fn launching_without_obs_says_where_to_get_it() {
        let err = launch(&Fake::default(), Os::MacOs).unwrap_err();
        assert!(err.contains("obsproject.com"), "{err}");
    }

    #[test]
    fn diagnoses_each_outcome() {
        let down = ObsConnection::Disconnected {
            reason: "nothing is listening".into(),
        };
        let installed = |running| Found {
            launcher: Some(Launcher::Flatpak),
            running,
        };
        let missing = |running| Found {
            launcher: None,
            running,
        };
        let cases = [
            (
                down.clone(),
                true,
                Some(installed(Some(false))),
                Some(ObsProblem::NotRunning),
            ),
            (
                down.clone(),
                true,
                Some(installed(None)),
                Some(ObsProblem::NotRunning),
            ),
            (
                down.clone(),
                true,
                Some(installed(Some(true))),
                Some(ObsProblem::ServerOff),
            ),
            // Running from somewhere unusual still means the server is off.
            (
                down.clone(),
                true,
                Some(missing(Some(true))),
                Some(ObsProblem::ServerOff),
            ),
            (
                down.clone(),
                true,
                Some(missing(Some(false))),
                Some(ObsProblem::NotInstalled),
            ),
            (down.clone(), true, None, None),
            (down.clone(), false, None, Some(ObsProblem::Unreachable)),
            (
                ObsConnection::PasswordRequired,
                true,
                None,
                Some(ObsProblem::PasswordRequired),
            ),
            (
                ObsConnection::WrongPassword,
                false,
                None,
                Some(ObsProblem::WrongPassword),
            ),
            (
                ObsConnection::Connecting,
                true,
                Some(missing(Some(false))),
                None,
            ),
        ];
        for (connection, local, found, want) in cases {
            assert_eq!(
                diagnose(&connection, local, found.as_ref()),
                want,
                "{connection:?} local={local} {found:?}"
            );
        }
    }

    #[test]
    fn local_hosts() {
        for host in [
            "localhost",
            "LocalHost",
            "127.0.0.1",
            "::1",
            "[::1]",
            " localhost ",
        ] {
            assert!(is_local(host), "{host}");
        }
        for host in ["192.168.1.20", "obs-pc.local", "example.org"] {
            assert!(!is_local(host), "{host}");
        }
    }

    #[test]
    fn settings_keep_the_password_unless_told() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("obs.json");
        let mut store = SettingsStore::load(path.clone());
        assert!(!store.saved());
        assert_eq!(store.settings(), &ObsSettings::default());

        store
            .put("localhost".into(), 4455, Some("secret".into()))
            .unwrap();
        store.put(" 192.168.1.9 ".into(), 4460, None).unwrap();
        let again = SettingsStore::load(path.clone());
        assert!(again.saved());
        assert_eq!(again.settings().host, "192.168.1.9");
        assert_eq!(again.settings().port, 4460);
        assert_eq!(again.settings().password.as_deref(), Some("secret"));

        let mut again = again;
        again
            .put("localhost".into(), 4455, Some(String::new()))
            .unwrap();
        assert_eq!(SettingsStore::load(path).settings().password, None);
    }

    #[cfg(unix)]
    #[test]
    fn settings_are_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("obs.json");
        SettingsStore::load(path.clone())
            .put("localhost".into(), 4455, Some("secret".into()))
            .unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn the_password_stays_out_of_debug() {
        let settings = ObsSettings {
            password: Some("hunter2".into()),
            ..ObsSettings::default()
        };
        assert!(!format!("{settings:?}").contains("hunter2"));
    }

    #[test]
    fn timecodes_drop_milliseconds() {
        assert_eq!(whole_seconds("01:02:03.456"), "01:02:03");
        assert_eq!(whole_seconds(""), "");
    }
}
