//! `WS /sync` on the local server, and T17's acceptance test across two
//! processes: two data folders pair, find each other (mDNS where the
//! network allows it), converge, push edits in well under a second, and a
//! peer that was cut off catches up.
//!
//! The second process is this test binary run again with
//! `LAMPSTAND_SYNC_CHILD` set; it plays the presenting machine and takes
//! line commands on stdin.

use std::io::{BufRead, BufReader, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lampstand_core::id::BlobHash;
use lampstand_core::output::OutputId;
use lampstand_core::scene::Scene;
use lampstand_core::song::{Song, SongId};
use lampstand_server::{Backend, Server};
use lampstand_sync::net::{NetError, NodeConfig, SyncNode};
use lampstand_sync::team::DeviceKeypair;
use lampstand_sync::{DocKind, Repo};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Sync(Option<Arc<SyncNode>>);

impl Backend for Sync {
    fn output_key_valid(&self, _: OutputId, _: &str) -> bool {
        false
    }
    fn watch_scene(&self, _: OutputId) -> Option<mpsc::Receiver<Arc<Scene>>> {
        None
    }
    fn blob_path(&self, _: &BlobHash) -> Option<PathBuf> {
        None
    }
    fn sync(&self) -> Option<Arc<SyncNode>> {
        self.0.clone()
    }
}

fn key_in(dir: &Path) -> DeviceKeypair {
    let path = dir.join("device.key");
    match std::fs::read(&path) {
        Ok(bytes) => DeviceKeypair::from_secret_bytes(bytes.try_into().unwrap()),
        Err(_) => {
            std::fs::create_dir_all(dir).unwrap();
            let key = DeviceKeypair::generate();
            std::fs::write(&path, key.secret_bytes()).unwrap();
            key
        }
    }
}

/// A node and its server on `port` (0 for any), as the app runs them, with
/// mDNS on this mDNS service type if one is given.
async fn node(dir: &Path, name: &str, port: u16, mdns: Option<&str>) -> (Arc<SyncNode>, Server) {
    let key = key_in(dir);
    let repo = Arc::new(
        Repo::open(
            dir.join("repo"),
            DeviceKeypair::from_secret_bytes(key.secret_bytes()),
        )
        .unwrap(),
    );
    let node = SyncNode::new(
        repo,
        key,
        NodeConfig {
            name: name.into(),
            dir: dir.join("sync"),
        },
    )
    .unwrap();
    // Every interface when advertising, as the app listens; mDNS gives
    // out the LAN address.
    let ip = if mdns.is_some() {
        [0, 0, 0, 0]
    } else {
        [127, 0, 0, 1]
    };
    let server = Server::start(
        SocketAddr::from((ip, port)),
        Arc::new(Sync(Some(node.clone()))),
    )
    .await
    .unwrap();
    if let Some(service) = mdns {
        node.set_mdns_service(service);
    }
    node.start(server.local_addr().port(), mdns.is_some());
    (node, server)
}

async fn http_status(addr: SocketAddr, path: &str) -> String {
    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    out.lines().next().unwrap_or_default().to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn sync_upgrades_and_the_rest_is_404_until_t43() {
    let dir = tempfile::tempdir().unwrap();
    let (presenter, server) = node(&dir.path().join("a"), "Sanctuary", 0, None).await;
    let addr = server.local_addr();
    assert!(http_status(addr, "/obs/x?k=y").await.contains("404"));
    assert!(http_status(addr, "/google/picker").await.contains("404"));
    // A plain GET of /sync is not an upgrade.
    assert!(!http_status(addr, "/sync").await.contains("101"));

    // The upgrade reaches the node, which refuses a stranger...
    let (stranger, _s) = node(&dir.path().join("b"), "Stranger", 0, None).await;
    match stranger.connect(&addr.to_string()).await {
        Err(NetError::Refused(why)) => assert!(why.contains("not paired"), "{why}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    // ...and pairs with the code.
    let code = presenter.start_pairing();
    stranger
        .pair(&code.code, Some(&addr.to_string()))
        .await
        .unwrap();
    assert_eq!(presenter.status().paired.len(), 1);

    // Without a sync node, /sync is not there.
    let none = Server::start(SocketAddr::from(([127, 0, 0, 1], 0)), Arc::new(Sync(None)))
        .await
        .unwrap();
    let url = format!("ws://{}/sync", none.local_addr());
    assert!(tokio_tungstenite::connect_async(url).await.is_err());
}

// ---- Two processes -----------------------------------------------------

const CHILD: &str = "LAMPSTAND_SYNC_CHILD";

fn song(id: SongId, title: &str) -> Song {
    let mut s = Song::new(title);
    s.id = id;
    s
}

fn titles(repo: &Repo) -> Vec<String> {
    let mut t: Vec<String> = repo
        .list(DocKind::Song)
        .into_iter()
        .filter_map(|id| repo.get::<Song>(id).unwrap().map(|s| s.title))
        .collect();
    t.sort();
    t
}

/// The presenting machine, in its own process.
async fn child(dir: PathBuf) {
    let port: u16 = std::env::var("LAMPSTAND_SYNC_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let mdns = std::env::var("LAMPSTAND_SYNC_MDNS").ok();
    let (node, server) = node(&dir, "Sanctuary", port, mdns.as_deref()).await;
    let repo = node.repo().clone();
    if repo.list(DocKind::Song).is_empty() {
        for title in ["Be Thou My Vision", "It Is Well"] {
            repo.put(&Song::new(title)).unwrap();
        }
    }
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    println!("READY {}", server.local_addr().port());
    while let Some(line) = rx.recv().await {
        let mut words = line.splitn(3, ' ');
        match (words.next(), words.next(), words.next()) {
            (Some("code"), _, _) => println!("CODE {}", node.start_pairing().code),
            (Some("titles"), _, _) => println!("TITLES {}", titles(&repo).join("|")),
            (Some("put"), Some(id), Some(title)) => {
                repo.put(&song(SongId::from_uuid(id.parse().unwrap()), title))
                    .unwrap();
                println!("OK");
            }
            (Some("watch"), Some(id), Some(title)) => {
                // Answers SEEN when the song has this title, from the
                // repo's change feed, as the app's reload does.
                let id: uuid::Uuid = id.parse().unwrap();
                let changes = repo.subscribe();
                println!("WATCHING");
                let title = title.to_string();
                let repo = repo.clone();
                tokio::task::spawn_blocking(move || loop {
                    if repo
                        .get::<Song>(id)
                        .unwrap()
                        .is_some_and(|s| s.title == title)
                    {
                        println!("SEEN {id}");
                        return;
                    }
                    if changes.recv_timeout(Duration::from_secs(30)).is_err() {
                        println!("TIMEOUT {id}");
                        return;
                    }
                })
                .await
                .unwrap();
            }
            (Some("quit"), _, _) | (None, _, _) => break,
            _ => println!("ERROR {line}"),
        }
    }
}

struct Presenter {
    process: Child,
    stdin: ChildStdin,
    lines: mpsc::Receiver<String>,
    port: u16,
}

impl Presenter {
    fn spawn(dir: &Path, port: u16, mdns: Option<&str>) -> Self {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args([
            "--exact",
            "two_processes_pair_find_converge_and_catch_up",
            "--nocapture",
        ])
        .env(CHILD, dir)
        .env("LAMPSTAND_SYNC_PORT", port.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
        if let Some(service) = mdns {
            cmd.env("LAMPSTAND_SYNC_MDNS", service);
        }
        let mut process = cmd.spawn().unwrap();
        let stdin = process.stdin.take().unwrap();
        let stdout = process.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut p = Self {
            process,
            stdin,
            lines,
            port,
        };
        p.port = p.expect("READY").parse().unwrap();
        p
    }

    fn send(&mut self, line: &str) {
        writeln!(self.stdin, "{line}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// The rest of the next line starting with `word`, skipping the test
    /// harness's own output.
    fn expect(&mut self, word: &str) -> String {
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(30))
                .unwrap_or_else(|_| {
                    panic!("the presenter process said nothing for 30 s (wanted {word})")
                });
            if let Some(rest) = line.strip_prefix(word) {
                return rest.trim().to_string();
            }
            assert!(
                !line.starts_with("TIMEOUT") && !line.starts_with("ERROR"),
                "presenter: {line}"
            );
        }
    }

    fn quit(mut self) {
        self.send("quit");
        let _ = self.process.wait();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test(flavor = "multi_thread")]
async fn two_processes_pair_find_converge_and_catch_up() {
    if let Some(dir) = std::env::var_os(CHILD) {
        return child(PathBuf::from(dir)).await;
    }
    let dir = tempfile::tempdir().unwrap();
    let presenter_dir = dir.path().join("presenter");
    // mDNS is tried; networks (and CI runners) that block multicast fall
    // back to the address, as a church would. Under a service type of this
    // run's own, so pairing without an address finds only this presenter:
    // not a Lampstand running elsewhere on the network, nor another copy
    // of this test, whose codes it would guess at and use up.
    let service = format!(
        "_lst-{}._tcp.local.",
        &SongId::generate().uuid().simple().to_string()[24..]
    );
    let mdns = std::env::var_os("LAMPSTAND_NO_MDNS")
        .is_none()
        .then_some(service.as_str());
    let mut presenter = Presenter::spawn(&presenter_dir, free_port(), mdns);
    let address = format!("127.0.0.1:{}", presenter.port);

    let (office, _server) = node(&dir.path().join("office"), "Office PC", 0, mdns).await;
    let repo = office.repo().clone();
    let grace = SongId::generate();
    repo.put(&song(grace, "Amazing Grace")).unwrap();
    repo.put(&Song::new("How Great Thou Art")).unwrap();

    // Pair: the presenter shows a code, the office PC types it.
    presenter.send("code");
    let code = presenter.expect("CODE");
    let start = Instant::now();
    let mut found = false;
    while mdns.is_some() && start.elapsed() < Duration::from_secs(5) {
        if office.status().nearby.iter().any(|n| n.pairing) {
            found = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    if found {
        println!("found the presenter by mDNS in {:?}", start.elapsed());
        office.pair(&code, None).await.unwrap();
    } else {
        println!("mDNS did not find the presenter; pairing by address");
        office.pair(&code, Some(&address)).await.unwrap();
    }

    // Converge: both libraries end up the same.
    let want = "Amazing Grace|Be Thou My Vision|How Great Thou Art|It Is Well";
    let start = Instant::now();
    loop {
        presenter.send("titles");
        if presenter.expect("TITLES") == want && titles(&repo).join("|") == want {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "did not converge"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    println!("converged {:?} after pairing", start.elapsed());

    // Quick fixes on the office PC reach the presenting machine.
    let mut times = Vec::new();
    for i in 0..20 {
        let title = format!("Amazing Grace (fix {i})");
        presenter.send(&format!("watch {} {title}", grace.uuid()));
        presenter.expect("WATCHING");
        let start = Instant::now();
        repo.put(&song(grace, &title)).unwrap();
        presenter.expect("SEEN");
        times.push(start.elapsed());
    }
    times.sort();
    let median = times[times.len() / 2];
    println!(
        "edit on the office PC → change on the presenting machine (two processes, one machine): \
         median {median:?}, max {:?}",
        times[times.len() - 1]
    );
    assert!(median < Duration::from_millis(500), "median {median:?}");

    // Cut off: the presenting machine goes away; the office PC carries on.
    let port = presenter.port;
    presenter.quit();
    let added = SongId::generate();
    repo.put(&song(added, "Added while apart")).unwrap();
    repo.put(&song(grace, "Amazing Grace (while apart)"))
        .unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;

    // It comes back on the same port and catches up.
    let start = Instant::now();
    let mut presenter = Presenter::spawn(&presenter_dir, port, mdns);
    presenter.send(&format!("watch {} Added while apart", added.uuid()));
    presenter.expect("WATCHING");
    presenter.expect("SEEN");
    presenter.send(&format!(
        "watch {} Amazing Grace (while apart)",
        grace.uuid()
    ));
    presenter.expect("WATCHING");
    presenter.expect("SEEN");
    println!("caught up {:?} after restarting", start.elapsed());
    // And its own changes come back the other way.
    let late = SongId::generate();
    presenter.send(&format!("put {} Added on the presenter", late.uuid()));
    presenter.expect("OK");
    let start = Instant::now();
    while repo.get::<Song>(late.uuid()).unwrap().is_none() {
        assert!(start.elapsed() < Duration::from_secs(10));
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    presenter.quit();
}
