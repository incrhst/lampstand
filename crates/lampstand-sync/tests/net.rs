//! T17 acceptance tests: the handshake refuses unpaired devices, pairing
//! with a code works and a wrong code does not, changes are pushed at once,
//! a cut-off peer catches up, blobs move by hash, and a large first sync
//! finishes in reasonable time. T75: through a relaying attacker, a changed,
//! replayed, reordered or injected frame closes the session, and a capture
//! shows no plaintext.
//!
//! Each node gets a plain tokio-tungstenite listener standing in for the
//! local server's `WS /sync` route (`lampstand-server` has the real one).

use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};

use lampstand_core::song::{Section, Song, SongId};
use lampstand_sync::driver::{Asking as DriveAsking, Notice as DriveNotice};
use lampstand_sync::net::{DriveHooks, DriveStatus, Link, NetError, NodeConfig, SyncNode};
use lampstand_sync::team::{DeviceKey, DeviceKeypair};
use lampstand_sync::{DocKind, Repo};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

struct Peer {
    node: Arc<SyncNode>,
    addr: SocketAddr,
    server: JoinHandle<()>,
    _dir: tempfile::TempDir,
}

impl Peer {
    fn repo(&self) -> &Repo {
        self.node.repo()
    }

    fn address(&self) -> String {
        self.addr.to_string()
    }

    /// Stops listening and drops every connection: the network is cut.
    fn cut(&self) {
        self.server.abort();
        self.node.drop_connections();
    }

    /// Listens again on the same port.
    async fn restore(&mut self) {
        let listener = TcpListener::bind(self.addr).await.unwrap();
        self.server = serve(self.node.clone(), listener);
    }
}

fn serve(node: Arc<SyncNode>, listener: TcpListener) -> JoinHandle<()> {
    tokio::spawn(async move {
        // Aborting the server drops the set, which aborts every accepted
        // connection, as a cut network would.
        let mut connections = tokio::task::JoinSet::new();
        while let Ok((stream, remote)) = listener.accept().await {
            let node = node.clone();
            connections.spawn(async move {
                if let Ok(ws) = tokio_tungstenite::accept_async(stream).await {
                    node.accept(Link::from_tungstenite(ws, Some(remote))).await;
                }
            });
        }
    })
}

async fn peer(name: &str) -> Peer {
    let dir = tempfile::tempdir().unwrap();
    peer_in(name, dir).await
}

async fn peer_in(name: &str, dir: tempfile::TempDir) -> Peer {
    let key = key_in(dir.path());
    let repo = Arc::new(
        Repo::open(
            dir.path().join("repo"),
            DeviceKeypair::from_secret_bytes(key.secret_bytes()),
        )
        .unwrap(),
    );
    let node = SyncNode::new(
        repo,
        key,
        NodeConfig {
            name: name.into(),
            dir: dir.path().join("sync"),
        },
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    node.start(addr.port(), false);
    let server = serve(node.clone(), listener);
    Peer {
        node,
        addr,
        server,
        _dir: dir,
    }
}

fn key_in(dir: &Path) -> DeviceKeypair {
    let path = dir.join("device.key");
    match std::fs::read(&path) {
        Ok(bytes) => DeviceKeypair::from_secret_bytes(bytes.try_into().unwrap()),
        Err(_) => {
            let key = DeviceKeypair::generate();
            std::fs::write(&path, key.secret_bytes()).unwrap();
            key
        }
    }
}

fn song(title: &str, line: &str) -> Song {
    let mut song = Song::new(title);
    song.sections = vec![Section {
        label: "V1".into(),
        lines: vec![line.into()],
    }];
    song
}

/// Waits until `check` holds, or fails after `limit`. Returns how long it
/// took.
async fn until(limit: Duration, what: &str, mut check: impl FnMut() -> bool) -> Duration {
    let start = Instant::now();
    while !check() {
        assert!(start.elapsed() < limit, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    start.elapsed()
}

fn title(repo: &Repo, id: SongId) -> Option<String> {
    repo.get::<Song>(id.uuid()).unwrap().map(|s| s.title)
}

async fn paired() -> (Peer, Peer) {
    let presenter = peer("Sanctuary").await;
    let office = peer("Office PC").await;
    let code = presenter.node.start_pairing();
    office
        .node
        .pair(&code.code, Some(&presenter.address()))
        .await
        .unwrap();
    (presenter, office)
}

#[tokio::test(flavor = "multi_thread")]
async fn unpaired_devices_are_refused_at_the_handshake() {
    let presenter = peer("Sanctuary").await;
    let stranger = peer("Stranger").await;
    let s = song("Secret", "nobody else sees this");
    presenter.repo().put(&s).unwrap();
    stranger.repo().put(&song("Junk", "never arrives")).unwrap();

    match stranger.node.connect(&presenter.address()).await {
        Err(NetError::Refused(why)) => assert!(why.contains("not paired"), "{why}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    // Pairing without a code being shown is refused too.
    match stranger
        .node
        .pair("123456", Some(&presenter.address()))
        .await
    {
        Err(NetError::Refused(why)) => assert!(why.contains("pairing code"), "{why}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(presenter.node.status().connected(), 0);
    assert!(presenter.node.status().paired.is_empty());
    assert_eq!(title(stranger.repo(), s.id), None);
    assert_eq!(presenter.repo().list(DocKind::Song).len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_code_is_refused_and_five_void_the_code() {
    let presenter = peer("Sanctuary").await;
    let office = peer("Office PC").await;
    let code = presenter.node.start_pairing();
    let wrong = if code.code == "000000" {
        "111111"
    } else {
        "000000"
    };
    for _ in 0..5 {
        match office.node.pair(wrong, Some(&presenter.address())).await {
            Err(NetError::Refused(why)) => assert!(why.contains("did not match"), "{why}"),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
    assert_eq!(presenter.node.status().pairing, None, "the code is void");
    // Even the right code no longer works.
    assert!(office
        .node
        .pair(&code.code, Some(&presenter.address()))
        .await
        .is_err());
    assert!(presenter.node.status().paired.is_empty());
    assert!(office.node.status().paired.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn parallel_wrong_codes_get_five_guesses_between_them() {
    let presenter = peer("Sanctuary").await;
    let code = presenter.node.start_pairing();
    let wrong = if code.code == "000000" {
        "111111"
    } else {
        "000000"
    };
    let mut attackers = Vec::new();
    for i in 0..4 {
        attackers.push(peer(&format!("Attacker {i}")).await);
    }
    let mut guessed = 0;
    let mut rounds = 0;
    while presenter.node.status().pairing.is_some() {
        rounds += 1;
        assert!(rounds < 50, "the code was never voided");
        let address = presenter.address();
        let tries = attackers.iter().map(|a| {
            let node = a.node.clone();
            let address = address.clone();
            async move { node.pair(wrong, Some(&address)).await }
        });
        for result in futures_util::future::join_all(tries).await {
            match result {
                Err(NetError::Refused(why)) if why.contains("did not match") => guessed += 1,
                Err(NetError::Refused(why))
                    if why.contains("another device is pairing")
                        || why.contains("not showing a pairing code") => {}
                other => panic!("expected a refusal, got {other:?}"),
            }
        }
    }
    assert_eq!(guessed, 5, "exactly five guesses were checked");
    // The right code no longer works.
    assert!(attackers[0]
        .node
        .pair(&code.code, Some(&presenter.address()))
        .await
        .is_err());
    assert!(presenter.node.status().paired.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn paired_devices_push_changes_both_ways_at_once() {
    let (presenter, office) = paired().await;
    assert_eq!(presenter.node.status().paired[0].name, "Office PC");
    assert_eq!(office.node.status().paired[0].name, "Sanctuary");
    assert_eq!(presenter.node.status().pairing, None, "a code is used once");
    until(Duration::from_secs(5), "both connected", || {
        presenter.node.status().connected() == 1 && office.node.status().connected() == 1
    })
    .await;

    // A lyric fix on the office PC reaches the presenting machine.
    let mut s = song("Amazing Grace", "Amazing grace how sweet the sond");
    office.repo().put(&s).unwrap();
    until(Duration::from_secs(5), "the new song", || {
        title(presenter.repo(), s.id).is_some()
    })
    .await;
    assert!(presenter.repo().list(DocKind::Song).contains(&s.id.uuid()));

    let mut times = Vec::new();
    for i in 0..20 {
        s.sections[0].lines[0] = format!("Amazing grace how sweet the sound ({i})");
        let start = Instant::now();
        office.repo().put(&s).unwrap();
        let want = s.sections[0].lines[0].clone();
        until(Duration::from_secs(5), "the fix", || {
            presenter
                .repo()
                .get::<Song>(s.id.uuid())
                .unwrap()
                .is_some_and(|got| got.sections[0].lines[0] == want)
        })
        .await;
        times.push(start.elapsed());
    }
    times.sort();
    println!(
        "edit → arrived on the presenting machine: median {:?}, max {:?} (20 edits, loopback)",
        times[times.len() / 2],
        times[times.len() - 1]
    );
    assert!(times[times.len() / 2] < Duration::from_millis(500));

    // And back the other way.
    let other = song("Be Thou My Vision", "Be thou my vision");
    presenter.repo().put(&other).unwrap();
    until(Duration::from_secs(5), "the song on the office PC", || {
        title(office.repo(), other.id).is_some()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn status_shows_when_changes_arrived_and_went_out() {
    let (presenter, office) = paired().await;
    until(Duration::from_secs(5), "connected", || {
        presenter.node.status().connected() == 1 && office.node.status().connected() == 1
    })
    .await;
    let s = song("Status", "a change to count");
    let before = Instant::now();
    office.repo().put(&s).unwrap();
    until(Duration::from_secs(5), "the song", || {
        title(presenter.repo(), s.id).is_some()
    })
    .await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let window = before.elapsed().as_millis() as i64 + 5_000;
    let out = office.node.status().last_out_ms.expect("sent a change");
    let arrived = presenter.node.status().last_in_ms.expect("got a change");
    assert!(now - out < window && now - arrived < window);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_device_that_unpaired_this_one_shows_as_refused_not_just_offline() {
    let (presenter, office) = paired().await;
    until(Duration::from_secs(5), "connected", || {
        office.node.status().connected() == 1
    })
    .await;
    assert!(office.node.status().paired[0].problem.is_none());

    presenter
        .node
        .unpair(office.node.device())
        .expect("unpaired");
    let status = loop {
        let st = office.node.status();
        if st.paired[0].refused {
            break st;
        }
        assert!(
            office.node.status().connected() <= 1,
            "still counted as a peer"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let p = &status.paired[0];
    assert!(!p.connected);
    assert!(
        p.problem.as_deref().unwrap().contains("not paired"),
        "{p:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cut_off_peer_is_offline_without_a_problem() {
    let (presenter, office) = paired().await;
    until(Duration::from_secs(5), "connected", || {
        office.node.status().connected() == 1
    })
    .await;
    presenter.cut();
    until(Duration::from_secs(20), "disconnected", || {
        office.node.status().connected() == 0
    })
    .await;
    // The device is reachable by address but refusing connections: that
    // is a reason to show, not a refusal. The first failed attempt can
    // take a while to be recorded (Windows is slow to refuse), so wait.
    until(Duration::from_secs(15), "a reason to show", || {
        office.node.status().paired[0].problem.is_some()
    })
    .await;
    let st = office.node.status();
    assert!(!st.paired[0].refused);
    assert!(st.paired[0].problem.is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cut_off_peer_catches_up_after_reconnecting() {
    let (mut presenter, office) = paired().await;
    until(Duration::from_secs(5), "connected", || {
        office.node.status().connected() == 1
    })
    .await;
    let s = song("Before", "before the cut");
    office.repo().put(&s).unwrap();
    until(Duration::from_secs(5), "first song", || {
        title(presenter.repo(), s.id).is_some()
    })
    .await;

    presenter.cut();
    until(Duration::from_secs(20), "disconnected", || {
        office.node.status().connected() == 0 && presenter.node.status().connected() == 0
    })
    .await;
    // Both sides change things while apart.
    let mut edited = s.clone();
    edited.title = "Renamed while apart".into();
    office.repo().put(&edited).unwrap();
    let added = song("Added while apart", "on the office PC");
    office.repo().put(&added).unwrap();
    let local = song("Added on the presenter", "while cut off");
    presenter.repo().put(&local).unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(title(presenter.repo(), added.id), None);

    presenter.restore().await;
    let took = until(Duration::from_secs(20), "catching up", || {
        title(presenter.repo(), added.id).is_some()
            && title(presenter.repo(), s.id).as_deref() == Some("Renamed while apart")
            && title(office.repo(), local.id).is_some()
    })
    .await;
    println!("caught up {took:?} after the network came back");
    assert_eq!(
        presenter.repo().list(DocKind::Song),
        office.repo().list(DocKind::Song)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn blobs_are_fetched_by_hash() {
    let (presenter, office) = paired().await;
    until(Duration::from_secs(5), "connected", || {
        presenter.node.status().connected() == 1
    })
    .await;
    // Bigger than one chunk, and not a multiple of it.
    let bytes: Vec<u8> = (0..700_000u32).map(|i| (i % 251) as u8).collect();
    let hash = office.repo().blobs().put(&bytes).unwrap();
    presenter.node.fetch_blob(hash).await.unwrap();
    let path = presenter.repo().blobs().path(&hash).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), bytes);

    let empty = office.repo().blobs().put(&[]).unwrap();
    presenter.node.fetch_blob(empty).await.unwrap();

    let missing = lampstand_core::id::BlobHash::of(b"nobody has this");
    assert!(presenter.node.fetch_blob(missing).await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_large_library_syncs_on_first_connect() {
    const SONGS: usize = 500;
    let presenter = peer("Sanctuary").await;
    let office = peer("Office PC").await;
    let start = Instant::now();
    for i in 0..SONGS {
        office
            .repo()
            .put(&song(&format!("Song {i}"), "a line"))
            .unwrap();
    }
    println!("{SONGS} puts on the office PC: {:?}", start.elapsed());
    let code = presenter.node.start_pairing();
    let start = Instant::now();
    office
        .node
        .pair(&code.code, Some(&presenter.address()))
        .await
        .unwrap();
    // Every song readable, not just listed: the team index can arrive
    // before the songs it lists.
    let took = until(Duration::from_secs(120), "the whole library", || {
        let listed = presenter.repo().list(DocKind::Song);
        listed.len() == SONGS
            && listed
                .iter()
                .all(|id| presenter.repo().get::<Song>(*id).unwrap().is_some())
    })
    .await;
    println!(
        "first sync of {SONGS} songs: {:?} ({:?} after pairing)",
        start.elapsed(),
        took
    );
}

/// What the relay in the middle does to the client's frames once the
/// handshake (a hello and a proof each way) is over.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Attack {
    Nothing,
    /// Flips one bit of the first encrypted frame.
    Change,
    /// Delivers the first encrypted frame twice.
    Replay,
    /// Swaps the first two encrypted frames.
    Reorder,
    /// Writes a well-formed plain `Sync` frame before the first one.
    Inject,
}

/// A WebSocket relay to a node's `/sync`, standing where an attacker on the
/// church network could: it sees every byte and can change what it relays.
/// Only the first connection is attacked; reconnections pass untouched.
struct Relay {
    addr: SocketAddr,
    /// Every message either way, on every connection.
    captured: Arc<Mutex<Vec<Vec<u8>>>>,
    /// The node closed the first connection.
    first_closed: Arc<AtomicBool>,
    _task: JoinHandle<()>,
}

async fn relay(upstream: SocketAddr, attack: Attack) -> Relay {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let captured = Arc::<Mutex<Vec<Vec<u8>>>>::default();
    let first_closed = Arc::new(AtomicBool::new(false));
    let (cap, closed) = (captured.clone(), first_closed.clone());
    let task = tokio::spawn(async move {
        let mut first = true;
        while let Ok((stream, _)) = listener.accept().await {
            let attack = if first { attack } else { Attack::Nothing };
            let closed = first.then(|| closed.clone());
            first = false;
            tokio::spawn(relay_one(stream, upstream, attack, cap.clone(), closed));
        }
    });
    Relay {
        addr,
        captured,
        first_closed,
        _task: task,
    }
}

async fn relay_one(
    stream: tokio::net::TcpStream,
    upstream: SocketAddr,
    attack: Attack,
    captured: Arc<Mutex<Vec<Vec<u8>>>>,
    closed: Option<Arc<AtomicBool>>,
) {
    let Ok(client) = tokio_tungstenite::accept_async(stream).await else {
        return;
    };
    let Ok((server, _)) = tokio_tungstenite::connect_async(format!("ws://{upstream}/sync")).await
    else {
        return;
    };
    let (mut to_client, mut from_client) = client.split();
    let (mut to_server, mut from_server) = server.split();
    let up_captured = captured.clone();
    let up = tokio::spawn(async move {
        let mut i = 0;
        let mut held = None;
        while let Some(Ok(m)) = from_client.next().await {
            let Message::Binary(m) = m else { continue };
            let m = m.to_vec();
            up_captured.lock().unwrap().push(m.clone());
            let out = match (attack, i) {
                (Attack::Change, 2) => {
                    let mut m = m;
                    let k = m.len() / 2;
                    m[k] ^= 1;
                    vec![m]
                }
                (Attack::Replay, 2) => vec![m.clone(), m],
                (Attack::Reorder, 2) => {
                    held = Some(m);
                    vec![]
                }
                (Attack::Reorder, 3) => vec![m, held.take().unwrap()],
                // Postcard for `Sync { kind: "song", id, data: "forged" }`.
                // Version 1 decoded it, logged the bad sync data and kept
                // the connection open; now the connection closes.
                (Attack::Inject, 2) => {
                    let forged = [&[3u8, 4][..], b"song", &[9; 16], &[6], b"forged"].concat();
                    vec![forged, m]
                }
                _ => vec![m],
            };
            i += 1;
            for m in out {
                if to_server.send(Message::Binary(m.into())).await.is_err() {
                    return;
                }
            }
        }
        let _ = to_server.close().await;
    });
    // Server to client, untouched, until the node closes.
    let mut node_closed = true;
    while let Some(Ok(m)) = from_server.next().await {
        let Message::Binary(m) = m else { continue };
        captured.lock().unwrap().push(m.to_vec());
        if to_client.send(Message::Binary(m)).await.is_err() {
            node_closed = false;
            break;
        }
    }
    if let (true, Some(closed)) = (node_closed, closed) {
        closed.store(true, Ordering::SeqCst);
    }
    let _ = to_client.close().await;
    up.abort();
}

/// Pairs a new office PC with a new presenter through a relay, the
/// office PC holding a few songs so it sends several frames at once.
async fn paired_through(attack: Attack) -> (Peer, Peer, Relay) {
    let presenter = peer("Sanctuary").await;
    let office = peer("Office PC").await;
    for i in 0..3 {
        office
            .repo()
            .put(&song(&format!("Hymn {i}"), "a line"))
            .unwrap();
    }
    let relay = relay(presenter.addr, attack).await;
    let code = presenter.node.start_pairing();
    office
        .node
        .pair(&code.code, Some(&relay.addr.to_string()))
        .await
        .unwrap();
    (presenter, office, relay)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[tokio::test(flavor = "multi_thread")]
async fn changed_replayed_reordered_or_injected_frames_close_the_session() {
    for attack in [
        Attack::Change,
        Attack::Replay,
        Attack::Reorder,
        Attack::Inject,
    ] {
        let (_presenter, _office, relay) = paired_through(attack).await;
        until(
            Duration::from_secs(5),
            &format!("the presenter to close the connection after {attack:?}"),
            || relay.first_closed.load(Ordering::SeqCst),
        )
        .await;
    }

    // Untouched, the same connection stays up and syncs.
    let (presenter, _office, relay) = paired_through(Attack::Nothing).await;
    until(Duration::from_secs(5), "the songs", || {
        presenter.repo().list(DocKind::Song).len() == 3
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!relay.first_closed.load(Ordering::SeqCst));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_capture_of_a_sync_shows_no_plaintext() {
    let presenter = peer("Sanctuary").await;
    let office = peer("Office PC").await;
    let lyric = "Great is thy faithfulness, O God my Father";
    let mut s = song("Great Is Thy Faithfulness", lyric);
    office.repo().put(&s).unwrap();
    // Blob chunks carry file bytes as they are, so a plain protocol would
    // show this lyric sheet whole.
    let sheet = format!("{lyric}\n").repeat(2000);
    let hash = office.repo().blobs().put(sheet.as_bytes()).unwrap();

    let relay = relay(presenter.addr, Attack::Nothing).await;
    let code = presenter.node.start_pairing();
    office
        .node
        .pair(&code.code, Some(&relay.addr.to_string()))
        .await
        .unwrap();
    until(Duration::from_secs(5), "the song", || {
        title(presenter.repo(), s.id).is_some()
    })
    .await;
    // A change pushed on the open connection, too.
    let pushed = "Morning by morning new mercies I see";
    s.sections[0].lines[0] = pushed.into();
    office.repo().put(&s).unwrap();
    until(Duration::from_secs(5), "the edit", || {
        presenter
            .repo()
            .get::<Song>(s.id.uuid())
            .unwrap()
            .is_some_and(|got| got.sections[0].lines[0] == pushed)
    })
    .await;
    presenter.node.fetch_blob(hash).await.unwrap();
    assert!(!relay.first_closed.load(Ordering::SeqCst));

    let captured = relay.captured.lock().unwrap().concat();
    assert!(captured.len() > sheet.len(), "the blob crossed the relay");
    // The handshake is in the clear: device names are not secret.
    assert!(contains(&captured, b"Office PC"));
    for needle in [
        "Great is thy faithfulness",
        "Great Is Thy Faithfulness",
        "Morning by morning",
        "song",
    ] {
        assert!(
            !contains(&captured, needle.as_bytes()),
            "{needle:?} is visible on the wire"
        );
    }
}

// ---- The presenter lease --------------------------------------------------

use lampstand_sync::net::PresenterStatus;
use lampstand_sync::presenter::{Denied, Notice, Role, Standing};

fn presenter_of(p: &Peer) -> PresenterStatus {
    p.node.status().presenter
}

/// Paired and connected, and both know that Sanctuary presents.
async fn paired_and_leased() -> (Peer, Peer) {
    let (sanctuary, office) = paired().await;
    until(Duration::from_secs(5), "both know who presents", || {
        presenter_of(&office).lease.name == "Sanctuary"
            && sanctuary.node.status().connected() == 1
            && office.node.status().connected() == 1
    })
    .await;
    (sanctuary, office)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lone_device_is_its_own_presenter() {
    let lone = peer("Sanctuary").await;
    assert_eq!(lone.node.role(), Role::Presenter);
    assert_eq!(presenter_of(&lone).standing, Standing::Presenting);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_device_that_showed_the_code_presents_and_the_other_plans() {
    let (sanctuary, office) = paired_and_leased().await;
    assert_eq!(sanctuary.node.role(), Role::Presenter);
    assert_eq!(office.node.role(), Role::Planning);
    let status = office.node.status();
    assert_eq!(status.presenter.standing, Standing::Ask);
    assert_eq!(status.presenter.lease.epoch, 1);
    assert!(
        status.paired[0].presenting,
        "Sanctuary is marked as presenting"
    );
    assert!(!sanctuary.node.status().paired[0].presenting);

    // A planning device cannot just take over while the presenter is here.
    assert_eq!(office.node.claim_presenter(), Err(Denied::MustAsk));
    assert_eq!(office.node.role(), Role::Planning);
    // Editing and syncing are unaffected.
    let s = song("Planned", "from the office");
    office.repo().put(&s).unwrap();
    until(Duration::from_secs(5), "the song", || {
        title(sanctuary.repo(), s.id).is_some()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn asking_shows_a_prompt_and_allowing_hands_the_role_over() {
    let (sanctuary, office) = paired_and_leased().await;
    office.node.ask_to_present().unwrap();
    assert!(!office.node.status().presenter.asking.unwrap().declined);
    until(Duration::from_secs(5), "the prompt", || {
        !presenter_of(&sanctuary).requests.is_empty()
    })
    .await;
    let request = presenter_of(&sanctuary).requests[0].clone();
    assert_eq!(request.name, "Office PC");
    assert_eq!(request.device, office.node.device());

    sanctuary.node.allow_presenter(request.device).unwrap();
    assert_eq!(
        sanctuary.node.role(),
        Role::Planning,
        "at once, on the old presenter"
    );
    until(Duration::from_secs(5), "the office PC to present", || {
        office.node.role() == Role::Presenter
    })
    .await;
    let status = office.node.status().presenter;
    assert_eq!(status.lease.name, "Office PC");
    assert_eq!(status.lease.epoch, 2);
    assert_eq!(status.asking, None);
    assert!(presenter_of(&sanctuary).requests.is_empty());
    // Handing over was the old presenter's choice: no notice.
    assert_eq!(presenter_of(&sanctuary).notice, None);
    until(Duration::from_secs(5), "the old presenter to know", || {
        presenter_of(&sanctuary).lease.name == "Office PC"
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn declining_keeps_the_presenter_and_tells_the_asker() {
    let (sanctuary, office) = paired_and_leased().await;
    office.node.ask_to_present().unwrap();
    until(Duration::from_secs(5), "the prompt", || {
        !presenter_of(&sanctuary).requests.is_empty()
    })
    .await;
    sanctuary.node.decline_presenter(office.node.device());
    until(Duration::from_secs(5), "the answer", || {
        presenter_of(&office).asking.is_some_and(|a| a.declined)
    })
    .await;
    assert_eq!(sanctuary.node.role(), Role::Presenter);
    assert_eq!(office.node.role(), Role::Planning);
    assert!(presenter_of(&sanctuary).requests.is_empty());

    // Asking again after "not now" works, and can be withdrawn.
    office.node.ask_to_present().unwrap();
    until(Duration::from_secs(5), "the prompt again", || {
        !presenter_of(&sanctuary).requests.is_empty()
    })
    .await;
    office.node.cancel_ask();
    until(Duration::from_secs(5), "the prompt to go", || {
        presenter_of(&sanctuary).requests.is_empty()
    })
    .await;
    assert_eq!(presenter_of(&office).asking, None);
}

#[tokio::test(flavor = "multi_thread")]
#[cfg_attr(
    target_os = "linux",
    ignore = "flaky on Linux CI: see docs/KNOWN-ISSUES.md"
)]
async fn takeover_waits_for_the_timeout_and_the_old_presenter_yields_on_return() {
    let (mut sanctuary, mut office) = paired_and_leased().await;
    office.node.set_takeover_after(Duration::from_secs(2));

    // A cut network cuts both ways: if the office kept listening, the
    // sanctuary's own dialler could reach it again and every reconnect
    // would restart the offline clock.
    sanctuary.cut();
    office.cut();
    until(Duration::from_secs(20), "disconnected", || {
        office.node.status().connected() == 0
    })
    .await;
    // Just offline: not yet.
    match office.node.claim_presenter() {
        Err(Denied::TooSoon { .. }) => {}
        other => panic!("expected TooSoon, got {other:?}"),
    }
    assert!(matches!(
        presenter_of(&office).standing,
        Standing::Wait { .. }
    ));
    assert_eq!(office.node.ask_to_present(), Err(Denied::NobodyToAsk));
    assert_eq!(office.node.role(), Role::Planning);

    let began = Instant::now();
    let mut seen: Vec<String> = Vec::new();
    while !matches!(presenter_of(&office).standing, Standing::TakeOver { .. }) {
        // Say what the office saw, second by second, if this ever times out.
        let st = office.node.status();
        let line = format!(
            "{}s: connected {} standing {} problem {:?}",
            began.elapsed().as_secs(),
            st.connected(),
            format!("{:?}", st.presenter.standing)
                .split([' ', '{'])
                .next()
                .unwrap_or_default(),
            st.paired[0].problem
        );
        if seen.last() != Some(&line) {
            seen.push(line);
        }
        assert!(
            began.elapsed() < Duration::from_secs(10),
            "timed out waiting for takeover to open:\n{}",
            seen.join("\n")
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    office.node.claim_presenter().unwrap();
    assert_eq!(office.node.role(), Role::Presenter);
    assert_eq!(presenter_of(&office).lease.epoch, 2);

    // The old presenter, still believing it presents, comes back.
    assert_eq!(sanctuary.node.role(), Role::Presenter);
    office.restore().await;
    sanctuary.restore().await;
    until(
        Duration::from_secs(15),
        "the old presenter to yield",
        || sanctuary.node.role() == Role::Planning,
    )
    .await;
    assert_eq!(
        presenter_of(&sanctuary).notice,
        Some(Notice::Replaced {
            by: "Office PC".into()
        })
    );
    sanctuary.node.dismiss_presenter_notice();
    assert_eq!(presenter_of(&sanctuary).notice, None);
    assert_eq!(office.node.role(), Role::Presenter, "it never resumes");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_lease_survives_a_restart() {
    let (sanctuary, office) = paired_and_leased().await;
    office.node.ask_to_present().unwrap();
    until(Duration::from_secs(5), "the prompt", || {
        !presenter_of(&sanctuary).requests.is_empty()
    })
    .await;
    sanctuary
        .node
        .allow_presenter(office.node.device())
        .unwrap();
    until(Duration::from_secs(5), "the office PC to present", || {
        office.node.role() == Role::Presenter
    })
    .await;
    let dir = office._dir.path().join("sync");
    until(Duration::from_secs(5), "the lease to be saved", || {
        std::fs::read_to_string(dir.join("presenter.json"))
            .is_ok_and(|j| j.contains("\"epoch\": 2"))
    })
    .await;
    // A node started afresh from the same files still knows.
    let again = SyncNode::new(
        office.node.repo().clone(),
        key_in(office._dir.path()),
        NodeConfig {
            name: "Office PC".into(),
            dir,
        },
    )
    .unwrap();
    assert_eq!(again.role(), Role::Presenter);
    assert_eq!(again.status().presenter.lease.epoch, 2);
}

// ---- Driving ------------------------------------------------------------

/// What the presenter's hooks saw: requests from drivers, and whether
/// someone drives.
type Commands = Arc<Mutex<Vec<(DeviceKey, Vec<u8>)>>>;

#[derive(Clone, Default)]
struct Seen {
    commands: Commands,
    live: Arc<Mutex<Vec<Vec<u8>>>>,
    driver: Arc<Mutex<Vec<bool>>>,
}

fn watch(p: &Peer) -> Seen {
    let seen = Seen::default();
    let (c, l, d) = (
        seen.commands.clone(),
        seen.live.clone(),
        seen.driver.clone(),
    );
    p.node.set_drive_hooks(DriveHooks {
        command: Box::new(move |from, payload| c.lock().unwrap().push((from, payload))),
        live: Box::new(move |payload| l.lock().unwrap().push(payload)),
        driver: Box::new(move |on| d.lock().unwrap().push(on)),
    });
    seen
}

fn drive_of(p: &Peer) -> DriveStatus {
    presenter_of(p).drive
}

/// Sanctuary presents; the office PC asks to drive and is allowed.
async fn driving_pair() -> (Peer, Peer, Seen, Seen) {
    let (sanctuary, office) = paired_and_leased().await;
    let on_sanctuary = watch(&sanctuary);
    let on_office = watch(&office);
    office.node.drive_ask().unwrap();
    until(Duration::from_secs(5), "the prompt", || {
        !drive_of(&sanctuary).requests.is_empty()
    })
    .await;
    sanctuary
        .node
        .drive_allow(office.node.device(), false)
        .unwrap();
    until(Duration::from_secs(5), "the office PC to drive", || {
        drive_of(&office).driving.is_some()
    })
    .await;
    (sanctuary, office, on_sanctuary, on_office)
}

#[tokio::test(flavor = "multi_thread")]
async fn asking_shows_a_prompt_and_allowing_lets_the_device_drive() {
    let (sanctuary, office) = paired_and_leased().await;
    assert!(drive_of(&office).can_drive);
    assert!(
        !drive_of(&sanctuary).can_drive,
        "the presenter does not ask"
    );
    assert!(office.node.drive_send(b"early".to_vec()).is_err());
    office.node.drive_ask().unwrap();
    assert_eq!(drive_of(&office).asking, Some(DriveAsking::Waiting));
    until(Duration::from_secs(5), "the prompt", || {
        !drive_of(&sanctuary).requests.is_empty()
    })
    .await;
    let request = drive_of(&sanctuary).requests[0].clone();
    assert_eq!(
        (request.name.as_str(), request.device),
        ("Office PC", office.node.device())
    );
    // Nothing is driven until the presenter says so.
    assert!(drive_of(&sanctuary).active.is_none());

    sanctuary.node.drive_allow(request.device, false).unwrap();
    until(Duration::from_secs(5), "the office PC to drive", || {
        drive_of(&office).driving.is_some()
    })
    .await;
    let office_view = drive_of(&office);
    let driving = office_view.driving.unwrap();
    assert_eq!((driving.name.as_str(), driving.online), ("Sanctuary", true));
    assert_eq!(office_view.asking, None);
    assert_eq!(drive_of(&sanctuary).active.unwrap().name, "Office PC");
    // Presenting did not move.
    assert_eq!(sanctuary.node.role(), Role::Presenter);
    assert_eq!(office.node.role(), Role::Planning);
}

#[tokio::test(flavor = "multi_thread")]
async fn declining_tells_the_asker_and_drives_nothing() {
    let (sanctuary, office) = paired_and_leased().await;
    office.node.drive_ask().unwrap();
    until(Duration::from_secs(5), "the prompt", || {
        !drive_of(&sanctuary).requests.is_empty()
    })
    .await;
    sanctuary.node.drive_decline(office.node.device());
    until(Duration::from_secs(5), "the answer", || {
        drive_of(&office).asking == Some(DriveAsking::Declined)
    })
    .await;
    assert!(drive_of(&sanctuary).active.is_none());
    assert!(drive_of(&office).driving.is_none());
    office.node.drive_dismiss();
    assert_eq!(drive_of(&office).asking, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_command_from_the_driver_reaches_the_presenter_and_live_state_comes_back() {
    let (sanctuary, office, on_sanctuary, on_office) = driving_pair().await;
    assert_eq!(*on_sanctuary.driver.lock().unwrap(), vec![true]);

    let sent = Instant::now();
    office
        .node
        .drive_send(b"{\"Live\":\"Next\"}".to_vec())
        .unwrap();
    let took = until(Duration::from_secs(5), "the command", || {
        !on_sanctuary.commands.lock().unwrap().is_empty()
    })
    .await;
    eprintln!("a command crossed in {:?}", sent.elapsed());
    assert!(took < Duration::from_millis(500), "took {took:?}");
    assert_eq!(
        on_sanctuary.commands.lock().unwrap()[0],
        (office.node.device(), b"{\"Live\":\"Next\"}".to_vec())
    );

    sanctuary.node.drive_live(b"live".to_vec());
    until(Duration::from_secs(5), "the live state", || {
        !on_office.live.lock().unwrap().is_empty()
    })
    .await;
    assert_eq!(on_office.live.lock().unwrap()[0], b"live".to_vec());
}

#[tokio::test(flavor = "multi_thread")]
async fn always_allow_is_remembered_across_stops_and_restarts() {
    let (sanctuary, office, ..) = driving_pair().await;
    sanctuary.node.drive_take_back();
    until(Duration::from_secs(5), "control to come back", || {
        drive_of(&office).driving.is_none()
    })
    .await;
    // Ask again, this time with "always allow".
    office.node.drive_ask().unwrap();
    until(Duration::from_secs(5), "the prompt", || {
        !drive_of(&sanctuary).requests.is_empty()
    })
    .await;
    sanctuary
        .node
        .drive_allow(office.node.device(), true)
        .unwrap();
    until(Duration::from_secs(5), "driving", || {
        drive_of(&office).driving.is_some()
    })
    .await;
    office.node.drive_cancel();
    until(Duration::from_secs(5), "the presenter to be free", || {
        drive_of(&sanctuary).active.is_none()
    })
    .await;
    // No prompt this time.
    office.node.drive_ask().unwrap();
    until(Duration::from_secs(5), "driving without a prompt", || {
        drive_of(&office).driving.is_some()
    })
    .await;
    assert!(drive_of(&sanctuary).requests.is_empty());
    assert_eq!(drive_of(&sanctuary).always.len(), 1);

    // A node started afresh from the same files still remembers.
    let again = SyncNode::new(
        sanctuary.node.repo().clone(),
        key_in(sanctuary._dir.path()),
        NodeConfig {
            name: "Sanctuary".into(),
            dir: sanctuary._dir.path().join("sync"),
        },
    )
    .unwrap();
    let remembered = again.status().presenter.drive.always;
    assert_eq!(remembered.len(), 1);
    assert_eq!(remembered[0].device, office.node.device());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_device_is_refused_while_one_drives() {
    let (sanctuary, office, ..) = driving_pair().await;
    let tablet = peer("Sam's tablet").await;
    let code = sanctuary.node.start_pairing();
    tablet
        .node
        .pair(&code.code, Some(&sanctuary.address()))
        .await
        .unwrap();
    until(
        Duration::from_secs(5),
        "the tablet to know who presents",
        || presenter_of(&tablet).lease.name == "Sanctuary" && tablet.node.status().connected() >= 1,
    )
    .await;
    tablet.node.drive_ask().unwrap();
    until(Duration::from_secs(5), "to be told who drives", || {
        drive_of(&tablet).asking
            == Some(DriveAsking::Busy {
                by: "Office PC".into(),
            })
    })
    .await;
    assert!(drive_of(&sanctuary).requests.is_empty(), "no queue");
    assert_eq!(
        drive_of(&sanctuary).active.unwrap().device,
        office.node.device()
    );
    // The tablet also knows who drives, to say so.
    until(Duration::from_secs(5), "the tablet to know", || {
        drive_of(&tablet).active.as_ref().map(|a| a.name.as_str()) == Some("Office PC")
    })
    .await;
    assert!(tablet.node.drive_send(b"x".to_vec()).is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_presenter_takes_control_back_at_once() {
    let (sanctuary, office, on_sanctuary, _) = driving_pair().await;
    sanctuary.node.drive_take_back();
    assert!(
        drive_of(&sanctuary).active.is_none(),
        "at once, on the presenter"
    );
    assert_eq!(*on_sanctuary.driver.lock().unwrap(), vec![true, false]);
    until(Duration::from_secs(5), "the driver to learn", || {
        drive_of(&office).driving.is_none()
    })
    .await;
    assert_eq!(
        drive_of(&office).notice,
        Some(DriveNotice::TakenBack {
            by: "Sanctuary".into()
        })
    );
    // What it sends now goes nowhere.
    assert!(office.node.drive_send(b"late".to_vec()).is_err());
    assert!(on_sanctuary.commands.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_presenter_going_offline_disables_driving_and_a_driver_going_offline_returns_control() {
    let (mut sanctuary, office, ..) = driving_pair().await;
    sanctuary.cut();
    until(
        Duration::from_secs(10),
        "the presenter to be offline",
        || drive_of(&office).driving.is_some_and(|d| !d.online),
    )
    .await;
    assert!(office.node.drive_send(b"x".to_vec()).is_err());
    // On the presenter, the driver is gone and control is back.
    until(Duration::from_secs(10), "the presenter to notice", || {
        drive_of(&sanctuary).active.is_none()
    })
    .await;
    assert!(!sanctuary.node.driven());
    sanctuary.restore().await;
    until(Duration::from_secs(15), "reconnecting", || {
        sanctuary.node.status().connected() == 1 && office.node.status().connected() == 1
    })
    .await;
    // The presenter forgot the driver while apart, and says so: this
    // device is no longer driving and has to ask again.
    until(Duration::from_secs(10), "the driver to be told", || {
        drive_of(&office).driving.is_none()
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn only_a_presenting_device_decides_and_it_alone() {
    let (sanctuary, office) = paired_and_leased().await;
    // The planning device cannot allow anyone, nor take control back.
    assert!(office
        .node
        .drive_allow(sanctuary.node.device(), false)
        .is_err());
    office.node.drive_take_back();
    // A device with nothing to ask cannot ask.
    assert!(sanctuary.node.drive_ask().is_err());
    // Handing the lease over ends any driving.
    let (sanctuary, office, ..) = driving_pair().await;
    office.node.ask_to_present().unwrap();
    until(Duration::from_secs(5), "the request", || {
        !presenter_of(&sanctuary).requests.is_empty()
    })
    .await;
    sanctuary
        .node
        .allow_presenter(office.node.device())
        .unwrap();
    until(Duration::from_secs(5), "the hand-over", || {
        office.node.role() == Role::Presenter
    })
    .await;
    assert!(office.node.driving().is_none());
    assert!(!sanctuary.node.driven());
}
