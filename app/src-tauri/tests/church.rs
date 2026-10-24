//! The church look, default theme and idle screen follow the church across
//! paired devices; the settings of one machine do not. Two apps stand in
//! for two computers; their libraries exchange sync messages as the real
//! network does, and a fake link says who is connected and records the
//! files asked for.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lampstand_api::{
    ChurchChoice, ChurchStatus, DeviceSettings, LibraryRequest, Request, Response, SettingsRequest,
};
use lampstand_app::handler::{App, Placement, SyncLink, WindowHost};
use lampstand_core::brand::Brand;
use lampstand_core::geom::Color;
use lampstand_core::id::BlobHash;
use lampstand_core::output::{IdleScreen, OutputId};
use lampstand_layout::Fonts;
use lampstand_render::output::DisplayInfo;
use lampstand_render::{OutputCommand, OutputSink};
use uuid::Uuid;

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

struct Link {
    name: &'static str,
    key: &'static str,
    peer: &'static str,
    fetched: Mutex<Vec<BlobHash>>,
}

impl SyncLink for Link {
    fn device_name(&self) -> String {
        self.name.into()
    }
    fn device_key(&self) -> String {
        self.key.into()
    }
    fn connected(&self) -> Vec<String> {
        vec![self.peer.into()]
    }
    fn fetch(&self, hashes: Vec<BlobHash>) {
        self.fetched.lock().unwrap().extend(hashes);
    }
}

struct Device {
    app: App,
    link: Arc<Link>,
}

fn device(dir: &Path, name: &'static str, key: &'static str, peer: &'static str) -> Device {
    let mut app = App::open(
        dir,
        Fonts::bundled_only(),
        Arc::new(Sink),
        Box::new(Host),
        Arc::new(|_| {}),
    )
    .unwrap();
    let link = Arc::new(Link {
        name,
        key,
        peer,
        fetched: Mutex::new(Vec::new()),
    });
    app.set_sync_link(link.clone());
    Device { app, link }
}

/// Mary's laptop has the lower key, so it offers first.
fn pair(a: &Path, b: &Path) -> (Device, Device) {
    (
        device(a, "Mary's laptop", "a", "b"),
        device(b, "Sanctuary PC", "b", "a"),
    )
}

fn exchange(a: &App, b: &App) {
    let (a, b) = (a.library().repo(), b.library().repo());
    for _ in 0..20 {
        let mut ids: Vec<Uuid> = a.documents().into_iter().map(|(_, id)| id).collect();
        ids.extend(b.documents().into_iter().map(|(_, id)| id));
        ids.sort();
        ids.dedup();
        let mut quiet = true;
        for id in ids {
            if let Some(m) = a.generate_sync_message(b.device(), id) {
                b.receive_sync_message(a.device(), m).unwrap();
                quiet = false;
            }
            if let Some(m) = b.generate_sync_message(a.device(), id) {
                a.receive_sync_message(b.device(), m).unwrap();
                quiet = false;
            }
        }
        if quiet {
            return;
        }
    }
    panic!("did not converge");
}

fn ok(r: Response) -> Response {
    if let Response::Error { message } = &r {
        panic!("{message}");
    }
    r
}

fn settings(d: &mut Device) -> DeviceSettings {
    match ok(d.app.handle(Request::Settings(SettingsRequest::Get))) {
        Response::Settings(s) => s,
        other => panic!("{other:?}"),
    }
}

fn status(d: &mut Device) -> ChurchStatus {
    match ok(d.app.handle(Request::Settings(SettingsRequest::Church))) {
        Response::Church(s) => s,
        other => panic!("{other:?}"),
    }
}

fn brand(name: &str, logo: Option<BlobHash>, primary: Color) -> Brand {
    Brand {
        name: name.into(),
        logo,
        primary,
        text: None,
        accent: Color::rgb(240, 190, 60),
        font_family: "Noto Serif".into(),
        bug: Default::default(),
    }
}

fn save_brand(d: &mut Device, b: Brand) {
    ok(d.app
        .handle(Request::Settings(SettingsRequest::SaveBrand(b))));
}

/// Ticks both devices at `t`, with the files they asked for and the
/// library exchange in between, as the running app does.
fn settle(a: &mut Device, b: &mut Device, start: Instant, secs: &[u64]) {
    for s in secs {
        let t = start + Duration::from_secs(*s);
        a.app.church_tick_at(t);
        exchange(&a.app, &b.app);
        b.app.church_tick_at(t);
        exchange(&a.app, &b.app);
    }
}

#[test]
fn the_look_and_idle_screen_arrive_on_the_other_device_and_the_clicker_does_not() {
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (mut a, mut b) = pair(da.path(), db.path());
    let logo = a.app.library().repo().blobs().put(b"the logo").unwrap();
    let video = a.app.library().repo().blobs().put(b"the loop").unwrap();
    save_brand(
        &mut a,
        brand("Grace Chapel", Some(logo), Color::rgb(20, 30, 90)),
    );
    let mut s = settings(&mut a);
    s.clicker_mode = true;
    s.idle = Some(IdleScreen::Video {
        blob: video,
        name: "Loop.mp4".into(),
    });
    ok(a.app.handle(Request::Settings(SettingsRequest::Put(s))));
    assert!(!status(&mut a).shared, "not shared until a device is met");

    let start = Instant::now();
    settle(&mut a, &mut b, start, &[1, 4, 5]);
    assert!(status(&mut a).shared && status(&mut b).shared);

    let on_b = settings(&mut b);
    assert_eq!(on_b.brand.as_ref().unwrap().name, "Grace Chapel");
    assert_eq!(on_b.brand.as_ref().unwrap().logo, Some(logo));
    assert!(matches!(on_b.idle, Some(IdleScreen::Video { .. })));
    assert!(on_b.default_theme.is_some() && on_b.default_theme == on_b.brand_theme);
    assert!(!on_b.clicker_mode, "the clicker is this machine's own");

    // The files have not arrived: B says what it waits for, and from whom.
    let waiting = status(&mut b).waiting;
    let mut what: Vec<_> = waiting.iter().map(|w| w.what.as_str()).collect();
    what.sort();
    assert_eq!(what, ["idle video", "logo"]);
    assert!(waiting.iter().all(|w| w.from == "Mary's laptop"));
    let asked = b.link.fetched.lock().unwrap().clone();
    assert!(asked.contains(&logo) && asked.contains(&video));

    // They arrive through the blob store; the waiting state clears.
    b.app.library().repo().blobs().put(b"the logo").unwrap();
    b.app.library().repo().blobs().put(b"the loop").unwrap();
    b.app.church_tick_at(start + Duration::from_secs(20));
    assert!(status(&mut b).waiting.is_empty());

    // Nothing of this machine's screens went across.
    assert!(matches!(
        ok(b.app.handle(Request::Library(LibraryRequest::ListThemes))),
        Response::Themes(_)
    ));
}

#[test]
fn edits_after_pairing_merge_by_field() {
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (mut a, mut b) = pair(da.path(), db.path());
    save_brand(&mut a, brand("Grace Chapel", None, Color::rgb(20, 30, 90)));
    let start = Instant::now();
    settle(&mut a, &mut b, start, &[1, 4, 5]);

    // Mary renames the church, someone else sets the logo bug, at once.
    let mut s = settings(&mut a);
    s.brand.as_mut().unwrap().name = "Grace Church".into();
    ok(a.app.handle(Request::Settings(SettingsRequest::Put(s))));
    let mut bug = settings(&mut b).brand.unwrap().bug;
    bug.enabled = true;
    ok(b.app
        .handle(Request::Settings(SettingsRequest::SetLogoBug(bug))));
    exchange(&a.app, &b.app);
    for d in [&mut a, &mut b] {
        let got = settings(d).brand.unwrap();
        assert_eq!(got.name, "Grace Church");
        assert!(got.bug.enabled);
    }
}

#[test]
fn a_deleted_theme_falls_back_on_every_device() {
    let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (mut a, mut b) = pair(da.path(), db.path());
    save_brand(&mut a, brand("Grace Chapel", None, Color::rgb(20, 30, 90)));
    let start = Instant::now();
    settle(&mut a, &mut b, start, &[1, 4, 5]);
    let theme = settings(&mut b).default_theme.expect("shared");
    ok(a.app
        .handle(Request::Library(LibraryRequest::DeleteTheme(theme))));
    exchange(&a.app, &b.app);
    let on_b = settings(&mut b);
    assert_eq!((on_b.default_theme, on_b.brand_theme), (None, None));
    assert!(on_b.brand.is_some(), "the look itself stays");
}

#[test]
fn two_different_looks_ask_once_and_nothing_is_merged_silently() {
    for keep_mine in [true, false] {
        let (da, db) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (mut a, mut b) = pair(da.path(), db.path());
        save_brand(&mut a, brand("Grace Chapel", None, Color::rgb(20, 30, 90)));
        save_brand(&mut b, brand("Mercy Hall", None, Color::rgb(90, 20, 30)));
        let start = Instant::now();
        // Mary's laptop offers first; the Sanctuary PC then finds a look
        // that is not its own.
        settle(&mut a, &mut b, start, &[1, 4, 7, 8]);
        assert!(status(&mut a).shared);
        let asked = status(&mut b);
        assert!(!asked.shared);
        let conflict = asked.conflict.expect("asked");
        assert_eq!(conflict.other_device, "Mary's laptop");
        assert!(conflict.mine.starts_with("Mercy Hall"));
        assert!(conflict.theirs.starts_with("Grace Chapel"));
        // Until it chooses, it keeps showing its own.
        assert_eq!(settings(&mut b).brand.unwrap().name, "Mercy Hall");

        let choice = if keep_mine {
            ChurchChoice::ThisDevice
        } else {
            ChurchChoice::Shared
        };
        ok(b.app
            .handle(Request::Settings(SettingsRequest::ResolveChurch(choice))));
        exchange(&a.app, &b.app);
        let (on_a, on_b) = (settings(&mut a), settings(&mut b));
        assert_eq!(on_a.brand, on_b.brand);
        let want = if keep_mine {
            "Mercy Hall"
        } else {
            "Grace Chapel"
        };
        assert_eq!(on_b.brand.unwrap().name, want);
        assert!(status(&mut b).shared && status(&mut b).conflict.is_none());
    }
}
