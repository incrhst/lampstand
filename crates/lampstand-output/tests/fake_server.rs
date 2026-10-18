//! Runs the real lampstand-output binary in `--windowed` mode against a fake
//! server playing a scripted show, and checks that it says hello as an
//! output, fetches images at its window width, survives every frame and the
//! server going away, and reconnects.
//!
//! Needs a display. Without one (e.g. a Linux CI box with no X/Wayland) the
//! binary cannot open a window; the test then skips, unless
//! LAMPSTAND_REQUIRE_GPU=1.

#[path = "support/fake_server.rs"]
mod fake_server;

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use fake_server::{script, FakeServer, LOGO_URL, SLIDE2_URL, SLIDE_URL};
use lampstand_show::protocol::{ClientMessage, Role};

struct Kill(Child);

impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn wait_for(what: &str, timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if done() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    eprintln!("timed out waiting for {what}");
    false
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plays_a_scripted_show_and_reconnects() {
    let server = FakeServer::start("127.0.0.1:0", None).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_lampstand-output"))
        .args(["--server", &server.ws_url(), "--windowed", "--display", "0", "--calibration"])
        .arg(dir.path().join("calibration.json"))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = Kill(child);

    let hello = wait_for("hello", Duration::from_secs(20), || server.hellos() > 0).await;
    if !hello {
        if let Some(status) = child.0.try_wait().unwrap() {
            let mut stderr = String::new();
            std::io::Read::read_to_string(child.0.stderr.as_mut().unwrap(), &mut stderr).unwrap();
            if std::env::var("LAMPSTAND_REQUIRE_GPU").as_deref() != Ok("1") {
                eprintln!("skipping: lampstand-output exited ({status}) without a display: {stderr}");
                return;
            }
            panic!("lampstand-output exited ({status}): {stderr}");
        }
        panic!("no hello from lampstand-output");
    }
    match &server.received.lock().unwrap()[0] {
        ClientMessage::Hello { role, .. } => assert_eq!(*role, Role::Output),
        other => panic!("first message was {other:?}"),
    }

    for (what, snapshot) in script() {
        eprintln!("state {}: {what}", snapshot.revision);
        server.set(snapshot);
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(child.0.try_wait().unwrap().is_none(), "lampstand-output died on: {what}");
    }

    // Deck images and the logo were fetched at the window's width, and the
    // second slide before it went live (it was next_cue).
    let requests = server.requests.lock().unwrap().clone();
    for url in [SLIDE_URL, SLIDE2_URL, LOGO_URL] {
        assert!(
            requests.iter().any(|r| r.starts_with(&format!("{url}?w=")) && !r.ends_with("w=0")),
            "{url} not fetched with ?w=: {requests:?}"
        );
    }
    let mut unique = requests.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), requests.len(), "each image fetched once per width: {requests:?}");

    // The server goes away and comes back: the process stays up and says
    // hello again within a few retries.
    server.kick();
    assert!(wait_for("reconnect", Duration::from_secs(5), || server.hellos() >= 2).await, "no reconnect");
    let (_, last) = script().pop().unwrap();
    server.set(last);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(child.0.try_wait().unwrap().is_none(), "lampstand-output died after reconnecting");
}
