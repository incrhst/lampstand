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
//! - **OBS on another computer (#133).** When the OBS host isn't this
//!   computer, [`diagnose`] says whether the computer refused the
//!   connection (OBS closed there, its server off, another port) or never
//!   answered, and Setup › OBS offers "Use this address" (a pin kept in
//!   `obs.json`) and "Set up OBS": a "Lampstand" collection with starter
//!   scenes and a "Lampstand stream" browser source. The source's address is
//!   this computer's network address, the pinned one if set, and never
//!   `localhost` ([`address_for_obs`]). It only touches those names, so a
//!   re-run changes nothing but a stale address. With OBS on this computer
//!   none of it shows and the URL is left as it is.
//! - **Launch** starts OBS minimised to the tray. Lampstand never edits
//!   OBS's own settings.
//! - **Download** opens OBS's download page in the default browser.
//! - **Scenes follow the live item (T90).** `App` sends a [`SceneCue`]
//!   whenever an item goes live, forwards or back. If the item names a
//!   scene, OBS switches to it; moving between slides of one item never
//!   switches. A scene picked by hand, in Present or in OBS itself, holds
//!   until the next item goes live. A failed switch never blocks the
//!   service: it shows as `ObsView::scene_problem`, and a switch that
//!   couldn't happen because OBS was away happens when it connects.
//!   [`Follower`] holds these rules, apart from any I/O.
//! - **Present's OBS drawer (T91).** Scene buttons send `SetScene`; the
//!   live preview asks for `Screenshot`s of the program scene (or of a
//!   scene the operator points at) a few times a second while the drawer
//!   is open, and nothing while it is shut. The audio inputs and their
//!   mutes are read on connect, follow `InputMuteStateChanged`, and are
//!   read again when inputs are added, removed or renamed.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lampstand_api::{
    Event, ObsAudioInput, ObsConnection, ObsOutputView, ObsProblem, ObsRequest, ObsSceneFailure,
    ObsSceneProblem, ObsView, Response,
};
use lampstand_connectors::obs::{codes, Input, Obs, ObsConfig, ObsError, ObsEvent, ObsStatus};
use lampstand_core::service::ItemId;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

use crate::handler::EventSink;

/// How long "Save and test" waits for the connection to settle.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(6);
/// How often the timecode and dropped frames refresh while OBS is busy.
const BUSY_REFRESH: Duration = Duration::from_secs(2);
/// How often to look for OBS again while it isn't connected.
const DETECT_REFRESH: Duration = Duration::from_secs(5);
/// What "Set up OBS" makes in OBS, and nothing else (#133).
const SETUP_COLLECTION: &str = "Lampstand";
const SETUP_SCENES: [&str; 3] = ["Slides", "Worship", "Pastor and slides"];
const SETUP_SOURCE: &str = "Lampstand stream";

/// `obs.json`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObsSettings {
    pub host: String,
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// The address of this computer that OBS, on another computer, uses to
    /// reach it, when the computer has several (#133). Unused while OBS is
    /// on this computer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_address: Option<String>,
}

impl Default for ObsSettings {
    fn default() -> Self {
        let config = ObsConfig::default();
        Self {
            host: config.host,
            port: config.port,
            password: None,
            pinned_address: None,
        }
    }
}

impl std::fmt::Debug for ObsSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObsSettings")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("password", &self.password.as_ref().map(|_| "…"))
            .field("pinned_address", &self.pinned_address)
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
            pinned_address: self.settings.pinned_address.clone(),
        };
        self.write(settings)
    }

    /// Pins (or, with `None`, unpins) the address OBS uses to reach this
    /// computer.
    pub fn pin(&mut self, address: Option<String>) -> io::Result<()> {
        let settings = ObsSettings {
            pinned_address: address,
            ..self.settings.clone()
        };
        self.write(settings)
    }

    fn write(&mut self, settings: ObsSettings) -> io::Result<()> {
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

/// Where "Get OBS" goes.
pub const DOWNLOAD_URL: &str = "https://obsproject.com/download";

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
    /// This computer's network addresses (IPv4, not loopback), for the
    /// "Use this address" picker when OBS is on another computer.
    fn addresses(&self) -> Vec<String> {
        Vec::new()
    }
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

    fn addresses(&self) -> Vec<String> {
        let mut found: Vec<String> = if_addrs::get_if_addrs()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|interface| match interface.ip() {
                std::net::IpAddr::V4(ip)
                    if !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified() =>
                {
                    Some(ip.to_string())
                }
                _ => None,
            })
            .collect();
        found.sort();
        found.dedup();
        found
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

/// Opens [`DOWNLOAD_URL`] in the default browser.
pub fn open_download(probe: &dyn Probe, os: Os) -> Result<(), String> {
    open_url(probe, os, DOWNLOAD_URL)
}

/// Opens a web page in the default browser.
pub fn open_url(probe: &dyn Probe, os: Os, url: &str) -> Result<(), String> {
    let started = match os {
        Os::MacOs => probe.spawn(Path::new("open"), &[url], None),
        // Not `cmd /c start`, which would need the URL quoted for cmd.
        Os::Windows => probe.spawn(
            Path::new("rundll32"),
            &["url.dll,FileProtocolHandler", url],
            None,
        ),
        Os::Linux => probe.spawn(Path::new("xdg-open"), &[url], None),
    };
    started.map_err(|e| format!("Could not open {url}: {e}"))
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
    refused: bool,
) -> Option<ObsProblem> {
    match connection {
        ObsConnection::Connecting | ObsConnection::Connected { .. } => None,
        ObsConnection::PasswordRequired => Some(ObsProblem::PasswordRequired),
        ObsConnection::WrongPassword => Some(ObsProblem::WrongPassword),
        ObsConnection::Disconnected { .. } if !local => Some(if refused {
            ObsProblem::RemoteNotListening
        } else {
            ObsProblem::Unreachable
        }),
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

/// What to say when OBS is on another computer and this one has no network
/// address to give it.
const NO_ADDRESS: &str = "OBS is on another computer, but this computer has no network address \
OBS can use. Connect this computer to the network, or choose an address under “Use this address”.";

/// The address `host` as it goes in a URL (IPv6 in brackets).
fn url_host(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

/// The parts of a picture's address, as Lampstand makes them
/// (`http://host:port/obs/<id>?k=<key>`): host, port, and the rest.
/// Anything else, including an address with a user name in it, is refused,
/// so OBS is only ever pointed at one of Lampstand's own pictures.
fn picture_parts(url: &str) -> Result<(&str, Option<&str>, &str), String> {
    let bad = || "That isn't one of Lampstand's picture addresses.".to_string();
    let rest = url.strip_prefix("http://").ok_or_else(bad)?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    if authority.contains('@') {
        return Err(bad());
    }
    let (host, port) = match authority.strip_prefix('[') {
        Some(v6) => {
            let (host, after) = v6.split_once(']').ok_or_else(bad)?;
            (host, after.strip_prefix(':'))
        }
        None => match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        },
    };
    let id_ok = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    };
    let shaped = tail
        .strip_prefix("/obs/")
        .and_then(|t| t.split_once("?k="))
        .is_some_and(|(id, key)| id_ok(id) && id_ok(key));
    let port_ok = port.is_none_or(|p| p.parse::<u16>().is_ok());
    if host.is_empty() || !shaped || !port_ok {
        return Err(bad());
    }
    Ok((host, port, tail))
}

/// `url` as OBS should load it. With OBS on this computer it is `url`
/// as it stands. With OBS on another computer, the host in `url` is
/// replaced by `pinned`, if there is one, and a host that only means "this
/// computer" (`localhost`, `127.0.0.1`, `0.0.0.0`) is refused: OBS would
/// look for Lampstand on itself.
pub fn address_for_obs(url: &str, pinned: Option<&str>, remote: bool) -> Result<String, String> {
    let (host, port, tail) = picture_parts(url)?;
    if !remote {
        return Ok(url.to_string());
    }
    let host = pinned.unwrap_or(host);
    let unspecified = host
        .parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_unspecified());
    if host.is_empty() || unspecified || is_local(host) {
        return Err(NO_ADDRESS.into());
    }
    let port = port.map(|p| format!(":{p}")).unwrap_or_default();
    Ok(format!("http://{}{port}{tail}", url_host(host)))
}

/// A pin as picked: one of this computer's addresses (`here`), or a name
/// another computer can use to reach it. Returns it tidied, or what to tell
/// the person.
pub fn check_pin(address: &str, here: &[String]) -> Result<String, String> {
    let address = address.trim();
    let bad = || {
        format!(
            "“{address}” isn't an address OBS can use. Pick one of this computer's network addresses."
        )
    };
    if address.is_empty() {
        return Err(bad());
    }
    if let Ok(ip) = address.parse::<std::net::IpAddr>() {
        // Only an address this computer has, so a typo can't send OBS to
        // another machine; and not one that only works on the cable itself.
        let link_local = match ip {
            std::net::IpAddr::V4(v4) => v4.is_link_local(),
            std::net::IpAddr::V6(v6) => (v6.segments()[0] & 0xffc0) == 0xfe80,
        };
        let ours = here.iter().any(|a| a.parse() == Ok(ip));
        return if ip.is_loopback()
            || ip.is_unspecified()
            || ip.is_multicast()
            || link_local
            || !ours
        {
            Err(bad())
        } else {
            Ok(ip.to_string())
        };
    }
    let labels: Vec<&str> = address.split('.').collect();
    let label_ok = |l: &&str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    // A name has a letter in it: "999.1.1.1" is a mistyped address, not a name.
    let looks_like_address = labels.iter().all(|l| l.chars().all(|c| c.is_ascii_digit()));
    if address.len() <= 253
        && labels.iter().all(label_ok)
        && !looks_like_address
        && !is_local(address)
    {
        Ok(address.to_ascii_lowercase())
    } else {
        Err(bad())
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
        ObsStatus::Disconnected { reason, .. } => ObsConnection::Disconnected {
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

// Scenes per item (T90).

/// An item went live: its OBS scene, if it names one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneCue {
    pub item: ItemId,
    pub scene: Option<String>,
}

/// Where `App` sends cues. Unbounded: a cue is tiny, and the live engine
/// must never wait for OBS.
pub type SceneCues = mpsc::UnboundedSender<SceneCue>;

/// When to switch OBS's scene, apart from talking to OBS.
///
/// - A new item clears any hold and asks for its scene.
/// - A scene picked by hand holds until the next item.
/// - A switch that failed because OBS was away is tried again when it
///   connects; one OBS refused (no such scene) waits for the next item or
///   `resume`.
/// - OBS reports every program-scene change, including ours. Scenes this
///   side asked for are remembered until OBS echoes them, so only a change
///   nobody here asked for counts as a hand pick in OBS itself.
#[derive(Debug, Default)]
pub struct Follower {
    wanted: Option<String>,
    done: bool,
    held: bool,
    asked: VecDeque<String>,
    problem: Option<ObsSceneProblem>,
}

impl Follower {
    /// An item went live. Returns the scene to switch to now.
    pub fn item_live(&mut self, scene: Option<String>) -> Option<String> {
        self.wanted = scene;
        self.done = false;
        self.held = false;
        self.problem = None;
        self.next()
    }

    /// The scene to switch to now, if any.
    pub fn next(&self) -> Option<String> {
        if self.held || self.done {
            None
        } else {
            self.wanted.clone()
        }
    }

    /// About to ask OBS for `scene`.
    pub fn asking(&mut self, scene: &str) {
        self.asked.push_back(scene.to_string());
    }

    /// OBS answered a switch to `scene`.
    pub fn switched(&mut self, scene: &str, result: &Result<(), ObsError>) {
        if self.wanted.as_deref() != Some(scene) {
            // A hand pick, or a cue since replaced: nothing to track.
            if result.is_err() {
                self.forget(scene);
            }
            return;
        }
        let failure = match result {
            Ok(()) => {
                self.done = true;
                self.problem = None;
                return;
            }
            Err(ObsError::NotConnected | ObsError::Disconnected) => {
                // Tried again on connect.
                self.forget(scene);
                ObsSceneFailure::NotConnected
            }
            Err(ObsError::Request { code, .. }) if *code == codes::RESOURCE_NOT_FOUND => {
                self.done = true;
                self.forget(scene);
                ObsSceneFailure::Missing
            }
            Err(e) => {
                self.done = true;
                self.forget(scene);
                ObsSceneFailure::Failed {
                    message: e.to_string(),
                }
            }
        };
        self.problem = Some(ObsSceneProblem {
            scene: scene.to_string(),
            reason: failure,
        });
    }

    /// OBS reports `scene` on air.
    pub fn obs_switched(&mut self, scene: &str) {
        if let Some(i) = self.asked.iter().position(|s| s == scene) {
            // Our own switch (and any older ones OBS skipped past).
            self.asked.drain(..=i);
            return;
        }
        // Somebody switched in OBS itself.
        self.hand_pick();
    }

    /// The operator picked a scene by hand.
    pub fn hand_pick(&mut self) {
        self.held = true;
        self.problem = None;
    }

    /// Ends a hold: the live item's scene applies again.
    pub fn resume(&mut self) -> Option<String> {
        self.held = false;
        self.done = false;
        self.problem = None;
        self.next()
    }

    /// The connection dropped: echoes of what was asked won't come.
    pub fn disconnected(&mut self) {
        self.asked.clear();
    }

    pub fn held(&self) -> bool {
        self.held
    }

    pub fn problem(&self) -> Option<&ObsSceneProblem> {
        self.problem.as_ref()
    }

    fn forget(&mut self, scene: &str) {
        if let Some(i) = self.asked.iter().rposition(|s| s == scene) {
            self.asked.remove(i);
        }
    }
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
    /// This computer's network addresses, while OBS is on another computer.
    addresses: Mutex<Vec<String>>,
    /// Streaming and recording, while connected.
    outputs: Mutex<(Option<ObsOutputView>, Option<ObsOutputView>)>,
    /// The last view pushed, so an unchanged one isn't sent again.
    told: Mutex<Option<ObsView>>,
    /// Scenes per item (T90).
    follower: Mutex<Follower>,
    /// OBS's scenes (kept after a disconnect, for planning) and the one on
    /// air (while connected).
    scenes: Mutex<(Vec<String>, Option<String>)>,
    /// OBS's audio inputs and their mutes, for Present's toggles (T91).
    audio: Mutex<Vec<ObsAudioInput>>,
    cues: SceneCues,
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
        let (cues, cue_rx) = mpsc::unbounded_channel();
        let control = Arc::new(Self {
            obs,
            settings: Mutex::new(settings),
            streaming,
            probe,
            os,
            emit,
            settled: Mutex::new(None),
            found: Mutex::new(None),
            addresses: Mutex::new(Vec::new()),
            outputs: Mutex::new((None, None)),
            told: Mutex::new(None),
            follower: Mutex::new(Follower::default()),
            scenes: Mutex::new((Vec::new(), None)),
            audio: Mutex::new(Vec::new()),
            cues,
        });
        tokio::spawn(watch_obs(
            Arc::downgrade(&control),
            control.obs.status(),
            cue_rx,
        ));
        control
    }

    /// Where `App` sends an item's scene when it goes live.
    pub fn cues(&self) -> SceneCues {
        self.cues.clone()
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
            ObsRequest::Download => self.open_download().await,
            ObsRequest::SetScene { scene } => {
                {
                    let mut follower = lock(&self.follower);
                    follower.hand_pick();
                    follower.asking(&scene);
                }
                let result = self.obs.set_program_scene(&scene).await;
                lock(&self.follower).switched(&scene, &result);
                let done = result.map_err(|e| format!("OBS didn't switch to {scene}: {e}"));
                self.publish();
                done
            }
            ObsRequest::Resume => {
                let next = lock(&self.follower).resume();
                if let Some(scene) = next {
                    self.switch(scene).await;
                }
                self.publish();
                Ok(())
            }
            ObsRequest::Screenshot { scene, width } => {
                return match self.screenshot(scene, width).await {
                    Ok(image) => Response::Thumbnail(image),
                    Err(message) => Response::Error { message },
                };
            }
            ObsRequest::UseAddress { address } => {
                let done = self.use_address(address).await;
                self.publish();
                done
            }
            ObsRequest::SetUp { url, width, height } => {
                return match self.set_up(&url, width, height).await {
                    Ok(done) => Response::Text(done),
                    Err(message) => Response::Error { message },
                };
            }
            ObsRequest::SetMute { input, muted } => {
                let verb = if muted { "mute" } else { "unmute" };
                let done = self
                    .obs
                    .set_input_muted(&input, muted)
                    .await
                    .map_err(|e| format!("OBS didn't {verb} {input}: {e}"));
                if done.is_ok() {
                    // OBS's event says the same; this way the answer has it.
                    self.muted(&input, muted);
                }
                self.publish();
                done
            }
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

    /// Pins the address (checked first) or goes back to Lampstand's pick.
    async fn use_address(&self, address: Option<String>) -> Result<(), String> {
        self.refresh_found().await;
        let here = lock(&self.addresses).clone();
        let address = address.map(|a| check_pin(&a, &here)).transpose()?;
        lock(&self.settings)
            .pin(address)
            .map_err(|e| format!("Could not save the address: {e}"))?;
        Ok(())
    }

    /// Whether OBS has an output running that setup would disturb by
    /// switching collections: streaming, recording, the virtual camera or
    /// the replay buffer. Asks OBS, not what was last heard.
    async fn on_air(&self) -> bool {
        if self.busy() {
            return true;
        }
        let virtual_cam = self.obs.virtual_cam_active().await.unwrap_or(false);
        virtual_cam || self.obs.replay_buffer_active().await
    }

    /// Waits for OBS to finish loading `collection` (it answers the switch
    /// before the scenes are there), up to a few seconds.
    async fn collection_loaded(&self, collection: &str) -> bool {
        for _ in 0..50 {
            if let Ok((current, _)) = self.obs.scene_collections().await {
                if current == collection && self.obs.scene_list().await.is_ok() {
                    return true;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    }

    /// "Set up OBS" (#133): the "Lampstand" scene collection, its starter
    /// scenes and the browser source. Adds only what is missing, to that
    /// collection and with those names, and leaves every other collection
    /// alone. If it has to switch OBS to the collection and then fails, it
    /// switches back.
    async fn set_up(&self, url: &str, width: u32, height: u32) -> Result<String, String> {
        let (host, pinned) = {
            let store = lock(&self.settings);
            let settings = store.settings();
            (settings.host.clone(), settings.pinned_address.clone())
        };
        let remote = !is_local(&host);
        let url = address_for_obs(url, pinned.as_deref(), remote)?;
        if !matches!(self.shown_status(), ObsStatus::Connected { .. }) {
            return Err(
                "Lampstand isn't connected to OBS yet. Save and test the connection first.".into(),
            );
        }
        const ON_AIR: &str = "OBS is streaming, recording or sending its picture out right now. Set it up after the service, so nothing changes on air.";
        if self.on_air().await {
            return Err(ON_AIR.into());
        }
        let obs = &self.obs;
        let mut changes: Vec<String> = Vec::new();

        let (current, collections) = obs.scene_collections().await.map_err(|e| failed(&e))?;
        let mut switched = false;
        if current != SETUP_COLLECTION {
            // Switching changes what OBS shows: look again just before.
            if self.on_air().await {
                return Err(ON_AIR.into());
            }
            if collections.iter().any(|c| c == SETUP_COLLECTION) {
                obs.set_scene_collection(SETUP_COLLECTION)
                    .await
                    .map_err(|e| failed(&e))?;
            } else {
                obs.create_scene_collection(SETUP_COLLECTION)
                    .await
                    .map_err(|e| failed(&e))?;
                changes.push(format!("made the “{SETUP_COLLECTION}” collection"));
            }
            switched = true;
        }
        if let Err(message) = self
            .fill_collection(&url, width, height, &mut changes)
            .await
        {
            if switched {
                // Put OBS back where it was; if even that fails, say so.
                let back = obs.set_scene_collection(&current).await.is_ok();
                return Err(if back {
                    format!("{message} OBS is back on “{current}”.")
                } else {
                    format!("{message} OBS is still on the “{SETUP_COLLECTION}” collection; switch back to “{current}” in OBS.")
                });
            }
            return Err(message);
        }

        self.refresh_scenes().await;
        self.publish();
        let on = if switched {
            format!(" OBS is now showing the “{SETUP_COLLECTION}” collection.")
        } else {
            String::new()
        };
        Ok(if changes.is_empty() {
            format!("OBS was already set up. Nothing changed.{on}")
        } else {
            format!("OBS is set up: {}.{on}", changes.join("; "))
        })
    }

    /// The scenes and the picture in the collection OBS is on now.
    async fn fill_collection(
        &self,
        url: &str,
        width: u32,
        height: u32,
        changes: &mut Vec<String>,
    ) -> Result<(), String> {
        let obs = &self.obs;
        if !self.collection_loaded(SETUP_COLLECTION).await {
            return Err(
                "OBS is slow to open the “Lampstand” collection. Try again in a moment.".into(),
            );
        }
        let have = obs.scene_list().await.map_err(|e| failed(&e))?.scenes;
        let missing: Vec<&str> = SETUP_SCENES
            .iter()
            .copied()
            .filter(|scene| !have.iter().any(|h| h == scene))
            .collect();
        for scene in &missing {
            obs.create_scene(scene).await.map_err(|e| failed(&e))?;
        }
        if !missing.is_empty() {
            changes.push(format!("added the scenes {}", missing.join(", ")));
        }

        // Only a browser source of our own name is ours to change.
        let inputs = obs.inputs().await.map_err(|e| failed(&e))?;
        if inputs
            .iter()
            .any(|i| i.name == SETUP_SOURCE && i.kind != "browser_source")
        {
            return Err(format!(
                "OBS already has something called “{SETUP_SOURCE}” that isn't a browser source. Lampstand left it alone; rename it in OBS and try again."
            ));
        }
        let mut exists = inputs.iter().any(|i| i.name == SETUP_SOURCE);
        for scene in SETUP_SCENES {
            let sources = obs.scene_sources(scene).await.map_err(|e| failed(&e))?;
            if !sources.iter().any(|s| s == SETUP_SOURCE) {
                obs.add_browser_source(scene, SETUP_SOURCE, url, width, height)
                    .await
                    .map_err(|e| failed(&e))?;
                if exists {
                    changes.push(format!("put the picture in the “{scene}” scene"));
                } else {
                    exists = true;
                    changes.push(format!("added the “{SETUP_SOURCE}” picture"));
                }
            }
        }
        let now = obs
            .browser_source_settings(SETUP_SOURCE)
            .await
            .map_err(|e| failed(&e))?;
        if now != (url.to_string(), width, height) {
            obs.update_browser_source(SETUP_SOURCE, url, width, height)
                .await
                .map_err(|e| failed(&e))?;
            changes.push("pointed the picture at this computer's address".into());
        }
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

    async fn open_download(&self) -> Result<(), String> {
        let probe = self.probe.clone();
        let os = self.os;
        tokio::task::spawn_blocking(move || open_download(&*probe, os))
            .await
            .map_err(|e| e.to_string())?
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
        // Only a computer whose OBS is elsewhere has a use for its addresses.
        let addresses = if local {
            Vec::new()
        } else {
            let probe = self.probe.clone();
            tokio::task::spawn_blocking(move || probe.addresses())
                .await
                .unwrap_or_default()
        };
        *lock(&self.addresses) = addresses;
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

    /// Switches OBS to the live item's scene and records the outcome.
    async fn switch(&self, scene: String) {
        lock(&self.follower).asking(&scene);
        let result = self.obs.set_program_scene(&scene).await;
        lock(&self.follower).switched(&scene, &result);
    }

    /// OBS's scene list and program scene. A failure keeps what was known.
    async fn refresh_scenes(&self) {
        if let Ok(list) = self.obs.scene_list().await {
            *lock(&self.scenes) = (list.scenes, Some(list.program));
        }
    }

    /// A picture of `scene`, or of the program scene, for the drawer's live
    /// preview. Asked for a few times a second while the drawer is open.
    async fn screenshot(&self, scene: Option<String>, width: u32) -> Result<String, String> {
        let known = scene.or_else(|| lock(&self.scenes).1.clone());
        let source = match known {
            Some(source) => source,
            None => {
                self.obs
                    .scene_list()
                    .await
                    .map_err(|e| e.to_string())?
                    .program
            }
        };
        self.obs
            .screenshot(&source, width)
            .await
            .map_err(|e| format!("No picture of {source} from OBS: {e}"))
    }

    /// OBS's audio devices (microphones, desktop audio) and whether each is
    /// muted; cameras and browser sources with sound are left out
    /// ([`Input::is_audio_device`]). A failure keeps what was known.
    async fn refresh_audio(&self) {
        let Ok(inputs) = self.obs.inputs().await else {
            return;
        };
        let mut audio = Vec::new();
        for input in inputs.into_iter().filter(Input::is_audio_device) {
            if let Ok(muted) = self.obs.input_muted(&input.name).await {
                audio.push(ObsAudioInput {
                    name: input.name,
                    muted,
                });
            }
        }
        *lock(&self.audio) = audio;
    }

    /// An input was muted or unmuted, here or in OBS.
    fn muted(&self, input: &str, muted: bool) {
        if let Some(a) = lock(&self.audio).iter_mut().find(|a| a.name == input) {
            a.muted = muted;
        }
    }

    /// An item went live.
    async fn cue(&self, cue: SceneCue) {
        let next = lock(&self.follower).item_live(cue.scene);
        if let Some(scene) = next {
            self.switch(scene).await;
        }
    }

    /// OBS connected: learn its scenes, then make any switch that had to
    /// wait for it.
    async fn on_connected(&self) {
        self.refresh_scenes().await;
        self.refresh_audio().await;
        let next = lock(&self.follower).next();
        if let Some(scene) = next {
            self.switch(scene).await;
        }
    }

    pub fn view(&self) -> ObsView {
        let (settings, saved) = {
            let store = lock(&self.settings);
            (store.settings().clone(), store.saved())
        };
        let connection = connection(&self.shown_status());
        let found = lock(&self.found).clone();
        let refused = matches!(
            self.shown_status(),
            ObsStatus::Disconnected { refused: true, .. }
        );
        let problem = diagnose(
            &connection,
            is_local(&settings.host),
            found.as_ref(),
            refused,
        );
        let connected = matches!(connection, ObsConnection::Connected { .. });
        let (stream, record) = if connected {
            lock(&self.outputs).clone()
        } else {
            (None, None)
        };
        let (scenes, program_scene) = lock(&self.scenes).clone();
        let follower = lock(&self.follower);
        let remote = !is_local(&settings.host);
        ObsView {
            remote,
            addresses: lock(&self.addresses).clone(),
            pinned_address: settings.pinned_address.clone().filter(|_| remote),
            host: settings.host,
            port: settings.port,
            has_password: settings.password.is_some(),
            connection,
            problem,
            relevant: saved || self.streaming.load(Ordering::Relaxed),
            stream,
            record,
            scenes,
            program_scene: program_scene.filter(|_| connected),
            held: follower.held(),
            scene_problem: follower.problem().cloned(),
            audio: if connected {
                lock(&self.audio).clone()
            } else {
                Vec::new()
            },
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
            (self.emit)(Event::Obs(Box::new(view)));
        }
    }

    fn busy(&self) -> bool {
        let outputs = lock(&self.outputs);
        outputs.0.as_ref().is_some_and(|s| s.active) || outputs.1.as_ref().is_some_and(|r| r.active)
    }
}

/// Follows the connection and OBS's events, and pushes the view when it
/// changes. Ends when the controller is dropped.
async fn watch_obs(
    control: std::sync::Weak<ObsControl>,
    mut status: watch::Receiver<ObsStatus>,
    mut cues: mpsc::UnboundedReceiver<SceneCue>,
) {
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
                let Some(c) = control.upgrade() else { return };
                let now = status.borrow_and_update().clone();
                match now {
                    ObsStatus::Connected { .. } => c.on_connected().await,
                    ObsStatus::Connecting => {}
                    _ => lock(&c.follower).disconnected(),
                }
                (true, true)
            }
            cue = cues.recv() => {
                let Some(cue) = cue else { return };
                let Some(c) = control.upgrade() else { return };
                c.cue(cue).await;
                (false, false)
            }
            event = events.recv() => match event {
                Ok(ObsEvent::StreamStateChanged { .. } | ObsEvent::RecordStateChanged { .. }) => (true, false),
                Ok(ObsEvent::ProgramSceneChanged { scene }) => {
                    let Some(c) = control.upgrade() else { return };
                    lock(&c.follower).obs_switched(&scene);
                    lock(&c.scenes).1 = Some(scene);
                    (false, false)
                }
                Ok(ObsEvent::SceneListChanged) => {
                    let Some(c) = control.upgrade() else { return };
                    c.refresh_scenes().await;
                    (false, false)
                }
                // Another collection has other scenes and inputs.
                Ok(ObsEvent::SceneCollectionChanged { .. }) => {
                    let Some(c) = control.upgrade() else { return };
                    c.refresh_scenes().await;
                    c.refresh_audio().await;
                    (false, false)
                }
                Ok(ObsEvent::InputMuteChanged { input, muted }) => {
                    let Some(c) = control.upgrade() else { return };
                    c.muted(&input, muted);
                    (false, false)
                }
                Ok(ObsEvent::Other { event_type, .. })
                    if matches!(event_type.as_str(), "InputCreated" | "InputRemoved" | "InputNameChanged") =>
                {
                    let Some(c) = control.upgrade() else { return };
                    c.refresh_audio().await;
                    (false, false)
                }
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

/// What the volunteer reads when OBS refuses a step of the setup: OBS's own
/// words are for the logs of OBS, not for them.
fn failed(_: &ObsError) -> String {
    "OBS didn't accept the setup. Check that OBS is still open, then try again.".to_string()
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
    fn get_obs_opens_the_download_page_in_the_browser() {
        let expected: [(Os, &str, &[&str]); 3] = [
            (Os::MacOs, "open", &[DOWNLOAD_URL]),
            (
                Os::Windows,
                "rundll32",
                &["url.dll,FileProtocolHandler", DOWNLOAD_URL],
            ),
            (Os::Linux, "xdg-open", &[DOWNLOAD_URL]),
        ];
        for (os, program, args) in expected {
            let fake = Fake::default();
            open_download(&fake, os).unwrap();
            assert_eq!(
                fake.spawned.lock().unwrap().as_slice(),
                [(
                    PathBuf::from(program),
                    args.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
                    None
                )],
                "{os:?}"
            );
        }
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
                ObsConnection::Disconnected {
                    reason: "OBS at 192.168.1.30:4455 did not answer".into(),
                },
                false,
                None,
                Some(ObsProblem::Unreachable),
            ),
            (
                ObsConnection::Disconnected {
                    reason: "nothing is listening on desk.local:4455 (failed to lookup address)"
                        .into(),
                },
                false,
                None,
                Some(ObsProblem::Unreachable),
            ),
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
                diagnose(&connection, local, found.as_ref(), false),
                want,
                "{connection:?} local={local} {found:?}"
            );
        }
    }

    #[test]
    fn a_remote_obs_that_refuses_is_told_from_one_that_never_answers() {
        let down = ObsConnection::Disconnected {
            reason: "whatever the words are".into(),
        };
        // Told by the kind of error the connection met, not by its words.
        assert_eq!(
            diagnose(&down, false, None, true),
            Some(ObsProblem::RemoteNotListening)
        );
        assert_eq!(
            diagnose(&down, false, None, false),
            Some(ObsProblem::Unreachable)
        );
        // OBS on this computer is diagnosed by looking at the computer.
        assert_eq!(diagnose(&down, true, None, true), None);
    }

    #[test]
    fn a_remote_obs_gets_the_network_address_never_localhost() {
        let url = "http://192.168.1.20:7340/obs/abc?k=key";
        // OBS on this computer: the URL as it stands, pin or no pin.
        for local in ["localhost", "127.0.0.1"] {
            assert_eq!(
                address_for_obs(
                    &format!("http://{local}:7340/obs/o1?k=1"),
                    Some("10.0.0.9"),
                    false
                ),
                Ok(format!("http://{local}:7340/obs/o1?k=1"))
            );
        }
        // OBS elsewhere: the address Lampstand picked, or the pinned one.
        assert_eq!(address_for_obs(url, None, true), Ok(url.to_string()));
        assert_eq!(
            address_for_obs(url, Some("10.0.0.9"), true),
            Ok("http://10.0.0.9:7340/obs/abc?k=key".to_string())
        );
        assert_eq!(
            address_for_obs(url, Some("fd00::9"), true),
            Ok("http://[fd00::9]:7340/obs/abc?k=key".to_string())
        );
        // No network address to give: refused, not written as localhost.
        for local in [
            "127.0.0.1:7340",
            "localhost:7340",
            "0.0.0.0:7340",
            "[::1]:7340",
        ] {
            let refused = address_for_obs(&format!("http://{local}/obs/abc?k=key"), None, true);
            assert!(
                refused
                    .as_ref()
                    .is_err_and(|m| m.contains("no network address")),
                "{local}: {refused:?}"
            );
        }
        // A pin wins over the address in the URL, even a localhost one.
        assert_eq!(
            address_for_obs("http://127.0.0.1:7340/obs/o1?k=1", Some("10.0.0.9"), true),
            Ok("http://10.0.0.9:7340/obs/o1?k=1".to_string())
        );
    }

    #[test]
    fn a_pin_must_be_an_address_another_computer_can_use() {
        let here = vec!["192.168.1.20".to_string(), "fd00::9".to_string()];
        assert_eq!(
            check_pin(" 192.168.1.20 ", &here),
            Ok("192.168.1.20".into())
        );
        assert_eq!(check_pin("fd00::9", &here), Ok("fd00::9".into()));
        assert_eq!(
            check_pin("Projector.Local", &here),
            Ok("projector.local".into())
        );
        for bad in [
            "",
            "localhost",
            "127.0.0.1",
            "0.0.0.0",
            "::1",
            "a b",
            "http://x",
            "x:7340",
            "-x",
            // Not an address this computer has.
            "192.168.1.99",
            // Look-alikes of an address.
            "999.1.1.1",
            "192.168.1",
            "1.2.3.4.5",
            // Only works on the cable itself.
            "169.254.3.4",
            "fe80::1",
        ] {
            assert!(check_pin(bad, &here).is_err(), "{bad:?} should be refused");
        }
        // Link-local is refused even when this computer has it.
        assert!(check_pin("169.254.3.4", &["169.254.3.4".into()]).is_err());
    }

    #[test]
    fn only_lampstands_own_picture_addresses_go_to_obs() {
        for bad in [
            "http://user:pw@192.168.1.20:7340/obs/a1?k=b2",
            "http://192.168.1.20@evil.example/obs/a1?k=b2",
            "https://192.168.1.20:7340/obs/a1?k=b2",
            "http://192.168.1.20:7340/other",
            "http://192.168.1.20:7340/obs/a1",
            "http://192.168.1.20:99999/obs/a1?k=b2",
            "javascript:alert(1)",
            "",
        ] {
            for remote in [true, false] {
                assert!(
                    address_for_obs(bad, Some("10.0.0.9"), remote).is_err(),
                    "{bad:?} remote={remote}"
                );
            }
        }
    }

    #[test]
    fn the_pin_is_kept_beside_the_password_and_only_when_set() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("obs.json");
        let mut store = SettingsStore::load(path.clone());
        store
            .put("studio.local".into(), 4455, Some("secret".into()))
            .unwrap();
        // A church with OBS on this computer: obs.json is as it was.
        assert!(!std::fs::read_to_string(&path)
            .unwrap()
            .contains("pinned_address"));
        store.pin(Some("192.168.1.20".into())).unwrap();
        // Saving the connection again keeps the pin and the password.
        store.put("studio.local".into(), 4455, None).unwrap();
        let again = SettingsStore::load(path);
        assert_eq!(
            again.settings().pinned_address.as_deref(),
            Some("192.168.1.20")
        );
        assert_eq!(again.settings().password.as_deref(), Some("secret"));
        // Never in the debug output.
        assert!(!format!("{:?}", again.settings()).contains("secret"));
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

    fn scene(name: &str) -> Option<String> {
        Some(name.to_string())
    }

    /// Asks for whatever the follower wants and reports `result`.
    fn switch(f: &mut Follower, result: Result<(), ObsError>) -> Option<String> {
        let next = f.next()?;
        f.asking(&next);
        f.switched(&next, &result);
        if result.is_ok() {
            f.obs_switched(&next);
        }
        Some(next)
    }

    #[test]
    fn an_item_going_live_switches_to_its_scene_once() {
        let mut f = Follower::default();
        assert_eq!(f.item_live(scene("Worship")), scene("Worship"));
        switch(&mut f, Ok(()));
        assert_eq!(f.next(), None, "on air: nothing more to do");
        assert!(!f.held(), "our own switch isn't a hand pick");
        assert_eq!(f.problem(), None);

        // An item with no scene leaves OBS alone.
        assert_eq!(f.item_live(None), None);
        // Previous back to the earlier item switches back.
        assert_eq!(f.item_live(scene("Worship")), scene("Worship"));
    }

    #[test]
    fn a_missing_scene_warns_and_the_service_carries_on() {
        let mut f = Follower::default();
        f.item_live(scene("Pastor and slides"));
        let missing = Err(ObsError::Request {
            code: codes::RESOURCE_NOT_FOUND,
            comment: None,
        });
        switch(&mut f, missing);
        assert_eq!(
            f.problem(),
            Some(&ObsSceneProblem {
                scene: "Pastor and slides".into(),
                reason: ObsSceneFailure::Missing,
            })
        );
        assert_eq!(f.next(), None, "not retried until the next item");
        f.item_live(scene("Worship"));
        assert_eq!(f.problem(), None, "the next item clears it");
    }

    #[test]
    fn a_switch_waits_for_a_disconnected_obs() {
        let mut f = Follower::default();
        f.item_live(scene("Sermon"));
        switch(&mut f, Err(ObsError::NotConnected));
        assert_eq!(
            f.problem().map(|p| &p.reason),
            Some(&ObsSceneFailure::NotConnected)
        );
        // On connect the controller asks again.
        f.disconnected();
        assert_eq!(switch(&mut f, Ok(())), scene("Sermon"));
        assert_eq!(f.problem(), None);
    }

    #[test]
    fn a_hand_pick_holds_until_the_next_item() {
        let mut f = Follower::default();
        f.item_live(scene("Sermon"));
        switch(&mut f, Ok(()));
        // Someone cuts to the wide shot in OBS itself.
        f.obs_switched("Wide");
        assert!(f.held());
        // OBS reconnecting doesn't undo it.
        f.disconnected();
        assert_eq!(f.next(), None);
        // The next item applies its own scene and ends the hold.
        assert_eq!(f.item_live(scene("Worship")), scene("Worship"));
        assert!(!f.held());
    }

    #[test]
    fn a_hand_pick_before_obs_answers_still_holds() {
        let mut f = Follower::default();
        f.item_live(scene("Sermon"));
        // Lampstand asks for Sermon, then the operator picks Wide before
        // OBS has echoed either.
        f.asking("Sermon");
        f.hand_pick();
        f.asking("Wide");
        f.switched("Sermon", &Ok(()));
        f.obs_switched("Sermon");
        f.obs_switched("Wide");
        assert!(f.held(), "echoes of both are ours, and the pick holds");
    }

    #[test]
    fn resume_ends_a_hold() {
        let mut f = Follower::default();
        f.item_live(scene("Sermon"));
        switch(&mut f, Ok(()));
        f.hand_pick();
        assert_eq!(f.resume(), scene("Sermon"));
        assert!(!f.held());

        // With no scene on the live item, resume just ends the hold.
        f.item_live(None);
        f.hand_pick();
        assert_eq!(f.resume(), None);
        assert!(!f.held());
    }

    #[test]
    fn timecodes_drop_milliseconds() {
        assert_eq!(whole_seconds("01:02:03.456"), "01:02:03");
        assert_eq!(whole_seconds(""), "");
    }
}
