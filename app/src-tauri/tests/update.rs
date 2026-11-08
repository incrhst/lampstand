//! Asking for the update status, and recording what a check found, must
//! always get the `App` lock and give it back. 0.8.0 took it twice in one
//! statement, so about 20 seconds after launch, when the screen first asked,
//! the thread waited on itself and every request after it waited too.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use lampstand_api::UpdateState;
use lampstand_app::handler::{App, Placement, WindowHost};
use lampstand_app::update;
use lampstand_core::output::OutputId;
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

fn open(dir: &std::path::Path) -> Arc<Mutex<App>> {
    Arc::new(Mutex::new(
        App::open(
            dir,
            Fonts::bundled_only(),
            Arc::new(Sink),
            Box::new(Host),
            Arc::new(|_| {}),
        )
        .unwrap(),
    ))
}

/// Runs `work` on its own thread and fails, rather than hangs, if it has
/// not finished in ten seconds.
fn within_ten_seconds<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    let (done, finished) = mpsc::channel();
    thread::spawn(move || {
        let _ = done.send(work());
    });
    finished
        .recv_timeout(Duration::from_secs(10))
        .expect("could not get the App lock: the update status waited on itself")
}

#[test]
fn asking_for_the_update_status_does_not_wait_on_itself() {
    let dir = tempfile::tempdir().unwrap();
    let app = open(dir.path());
    let status = within_ten_seconds({
        let app = app.clone();
        move || update::status(&app, "0.0.0".into(), UpdateState::NotChecked)
    });
    assert_eq!(status.current, "0.0.0");
    assert_eq!(status.state, UpdateState::NotChecked);
    // Nothing is on air and no service is near, so an install is not refused.
    assert_eq!(status.blocked, None);
    // The lock is back: the next request gets it.
    within_ten_seconds(move || drop(app.lock().unwrap()));
}

#[test]
fn a_check_for_an_update_answers_with_the_status_and_gives_the_lock_back() {
    let dir = tempfile::tempdir().unwrap();
    let app = open(dir.path());
    let found = vec![
        Ok(None),
        Ok(Some(("0.9.1".to_string(), Some("Fixes.".to_string())))),
        Ok(Some(("0.9.1".to_string(), Some("  ".to_string())))),
        Err("offline".to_string()),
    ];
    for found in found {
        let app = app.clone();
        let status = within_ten_seconds(move || {
            let state = update::checked(found);
            update::status(&app, "0.9.0".into(), state)
        });
        match status.state {
            UpdateState::UpToDate | UpdateState::Failed { .. } => {}
            UpdateState::Available { version, notes } => {
                assert_eq!(version, "0.9.1");
                // Notes of only spaces are no notes.
                assert!(notes.is_none_or(|n| n == "Fixes."));
            }
            other => panic!("{other:?}"),
        }
    }
    within_ten_seconds(move || drop(app.lock().unwrap()));
}

#[test]
fn the_update_status_still_answers_after_a_panic_poisoned_the_lock() {
    let dir = tempfile::tempdir().unwrap();
    let app = open(dir.path());
    let poisoner = app.clone();
    let _ = thread::spawn(move || {
        let _held = poisoner.lock().unwrap();
        panic!("a request panicked while it held the lock");
    })
    .join();
    let status =
        within_ten_seconds(move || update::status(&app, "0.9.0".into(), UpdateState::UpToDate));
    assert_eq!(status.state, UpdateState::UpToDate);
}
