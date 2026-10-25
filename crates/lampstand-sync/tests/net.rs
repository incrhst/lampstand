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
use lampstand_sync::net::{
    DriveAsking, DriveHooks, DriveNotice, DriveStatus, Link, NetError, NodeConfig, SyncNode,
};
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
        // Offline first: no connection, accepted or dialled, can register
        // after this, so none survives the cut.
        self.node.set_offline(true);
        self.server.abort();
    }

    /// Stops listening and drops every connection, but the node can still
    /// dial out (an unplugged cable on one side only): for the test that has
    /// the sanctuary find the office PC again through an address.
    fn cut_listener(&self) {
        self.server.abort();
        self.node.drop_connections();
    }

    /// Listens again on the same port.
    async fn restore(&mut self) {
        let listener = TcpListener::bind(self.addr).await.unwrap();
        self.server = serve(self.node.clone(), listener);
        self.node.set_offline(false);
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
    // Every test device has a screen: Projection can be held.
    node.set_screens(true);
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
use lampstand_sync::roles::{AskState, Denied, Mode, Notice, Standing};

fn presenter_of(p: &Peer) -> PresenterStatus {
    p.node.status().presenter
}

/// Paired and connected, and both know that Sanctuary presents.
async fn paired_and_leased() -> (Peer, Peer) {
    let (sanctuary, office) = paired().await;
    until(Duration::from_secs(5), "both know who presents", || {
        office.node.status().roles.audio.record.name == "Sanctuary"
            && sanctuary.node.status().connected() == 1
            && office.node.status().connected() == 1
    })
    .await;
    (sanctuary, office)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lone_device_is_its_own_presenter() {
    let lone = peer("Sanctuary").await;
    assert_eq!(lone.node.role(), Mode::Presenter);
    assert_eq!(presenter_of(&lone).standing, Standing::Holding);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_device_that_showed_the_code_presents_and_the_other_plans() {
    let (sanctuary, office) = paired_and_leased().await;
    assert_eq!(sanctuary.node.role(), Mode::Presenter);
    assert_eq!(office.node.role(), Mode::Planning);
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
    assert_eq!(office.node.role(), Mode::Planning);
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
    assert_eq!(
        office.node.status().presenter.asking.unwrap().state,
        AskState::Waiting
    );
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
        Mode::Planning,
        "at once, on the old presenter"
    );
    until(Duration::from_secs(5), "the office PC to present", || {
        office.node.role() == Mode::Presenter
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
        presenter_of(&office)
            .asking
            .is_some_and(|a| a.state == AskState::Declined)
    })
    .await;
    assert_eq!(sanctuary.node.role(), Mode::Presenter);
    assert_eq!(office.node.role(), Mode::Planning);
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
    // A claim against a device is judged by that device's own takeover time.
    sanctuary.node.set_takeover_after(Duration::from_secs(2));
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
    assert_eq!(office.node.role(), Mode::Planning);

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
            began.elapsed() < Duration::from_secs(40),
            "timed out waiting for takeover to open:\n{}",
            seen.join("\n")
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    office.node.claim_presenter().unwrap();
    assert_eq!(office.node.role(), Mode::Presenter);
    assert_eq!(presenter_of(&office).lease.epoch, 2);

    // The old presenter, still believing it presents, comes back.
    assert_eq!(sanctuary.node.role(), Mode::Presenter);
    office.restore().await;
    sanctuary.restore().await;
    until(
        Duration::from_secs(15),
        "the old presenter to yield",
        || sanctuary.node.role() == Mode::Planning,
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
    assert_eq!(office.node.role(), Mode::Presenter, "it never resumes");
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
        office.node.role() == Mode::Presenter
    })
    .await;
    let dir = office._dir.path().join("sync");
    until(Duration::from_secs(5), "the lease to be saved", || {
        std::fs::read_to_string(dir.join("roles.json")).is_ok_and(|j| j.contains("\"epoch\": 2"))
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
    assert_eq!(again.role(), Mode::Presenter);
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
    assert_eq!(sanctuary.node.role(), Mode::Presenter);
    assert_eq!(office.node.role(), Mode::Planning);
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
async fn a_second_asker_is_told_who_is_already_asking_and_is_not_queued() {
    let (sanctuary, office) = paired_and_leased().await;
    let tablet = peer("Sam's tablet").await;
    // Pairing is between two devices, not transitive: the tablet pairs with
    // each device it will deal with.
    for other in [&sanctuary, &office] {
        let code = other.node.start_pairing();
        tablet
            .node
            .pair(&code.code, Some(&other.address()))
            .await
            .unwrap();
    }
    until(
        Duration::from_secs(5),
        "the tablet to know who holds the roles",
        || {
            tablet.node.status().roles.audio.record.name == "Sanctuary"
                && tablet.node.status().connected() == 2
        },
    )
    .await;
    office.node.drive_ask().unwrap();
    until(Duration::from_secs(5), "the prompt", || {
        !drive_of(&sanctuary).requests.is_empty()
    })
    .await;
    tablet.node.drive_ask().unwrap();
    until(Duration::from_secs(5), "to be told who is asking", || {
        drive_of(&tablet).asking
            == Some(DriveAsking::Busy {
                by: "Office PC".into(),
            })
    })
    .await;
    let requests = drive_of(&sanctuary).requests;
    assert_eq!(requests.len(), 1, "no queue");
    assert_eq!(requests[0].device, office.node.device());
    // The prompt is still the office's to answer.
    sanctuary
        .node
        .drive_allow(office.node.device(), false)
        .unwrap();
    until(Duration::from_secs(5), "the office PC to drive", || {
        drive_of(&office).driving.is_some()
    })
    .await;
    // The tablet knows who drives, to say so, and cannot send commands.
    until(Duration::from_secs(5), "the tablet to know", || {
        drive_of(&tablet).active.as_ref().map(|a| a.name.as_str()) == Some("Office PC")
    })
    .await;
    assert!(tablet.node.drive_send(b"x".to_vec()).is_err());
    // It can ask again now; the Operator seat's holder, the office PC, is
    // the one to answer.
    tablet.node.drive_dismiss();
    tablet.node.drive_ask().unwrap();
    until(Duration::from_secs(5), "the office PC to be asked", || {
        !drive_of(&office).requests.is_empty()
    })
    .await;
    assert_eq!(drive_of(&office).requests[0].device, tablet.node.device());
    assert!(drive_of(&sanctuary).requests.is_empty());
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
async fn the_presenter_going_offline_disables_driving_and_the_operator_keeps_its_seat() {
    let (mut sanctuary, office, on_sanctuary, _) = driving_pair().await;
    assert_eq!(*on_sanctuary.driver.lock().unwrap(), vec![true]);
    sanctuary.cut();
    until(
        Duration::from_secs(10),
        "the presenter to be offline",
        || drive_of(&office).driving.is_some_and(|d| !d.online),
    )
    .await;
    assert!(office.node.drive_send(b"x".to_vec()).is_err());
    assert!(!office.node.roles_status().operator_ready);
    // On the presenter, the operator is away, so nothing drives it...
    until(Duration::from_secs(10), "the presenter to notice", || {
        drive_of(&sanctuary).active.is_none()
    })
    .await;
    assert!(!sanctuary.node.driven());
    assert_eq!(*on_sanctuary.driver.lock().unwrap(), vec![true, false]);
    // ...but the seat is kept: nobody took it, so the operator is still the
    // operator when they meet again, and the presenter is told to send it
    // everything on air.
    assert_eq!(
        office.node.status().roles.operator.record.holder,
        Some(office.node.device())
    );
    sanctuary.restore().await;
    until(Duration::from_secs(15), "reconnecting", || {
        sanctuary.node.status().connected() == 1 && office.node.status().connected() == 1
    })
    .await;
    until(
        Duration::from_secs(10),
        "the operator to drive again",
        || drive_of(&office).driving.is_some_and(|d| d.online) && sanctuary.node.driven(),
    )
    .await;
    assert!(office.node.roles_status().operator_ready);
    assert_eq!(
        *on_sanctuary.driver.lock().unwrap(),
        vec![true, false, true]
    );
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
        office.node.role() == Mode::Presenter
    })
    .await;
    assert!(office.node.driving().is_none());
    assert!(!sanctuary.node.driven());
}

// ---- Roles: the same four ways for every job ---------------------------------

use lampstand_sync::net::{RoleStatus, RolesStatus};
use lampstand_sync::roles::{Blocked, OnAir, Record, Role};

fn roles_of(p: &Peer) -> RolesStatus {
    p.node.status().roles
}

fn role_status(p: &Peer, role: Role) -> RoleStatus {
    let r = roles_of(p);
    match role {
        Role::Projection => r.projection,
        Role::Operator => r.operator,
        Role::Stream => r.stream,
        Role::Audio => r.audio,
    }
}

fn holder_of(p: &Peer, role: Role) -> Option<DeviceKey> {
    role_status(p, role).record.holder
}

#[tokio::test(flavor = "multi_thread")]
async fn pairing_leaves_the_main_machine_with_three_roles_and_stream_unheld() {
    let (sanctuary, office) = paired_and_leased().await;
    let s = sanctuary.node.device();
    for role in [Role::Projection, Role::Operator, Role::Audio] {
        assert_eq!(holder_of(&sanctuary, role), Some(s), "{role:?}");
        assert_eq!(holder_of(&office, role), Some(s), "{role:?}");
        assert_eq!(role_status(&sanctuary, role).record.epoch, 1);
        assert!(role_status(&sanctuary, role).holds);
        assert!(!role_status(&office, role).holds);
        assert_eq!(role_status(&office, role).standing, Standing::Ask);
    }
    for p in [&sanctuary, &office] {
        assert_eq!(holder_of(p, Role::Stream), None, "Stream is unheld");
    }
    assert_eq!(role_status(&office, Role::Stream).standing, Standing::Open);
    assert!(office.node.roles_status().screens);
}

/// A church with one device and nothing paired sees nothing new: it holds
/// all four roles, is asked nothing, and the status its screens read is the
/// one 0.7.0 gave.
#[tokio::test(flavor = "multi_thread")]
async fn a_device_with_nobody_paired_holds_all_four_and_shows_nothing_new() {
    let lone = peer("Sanctuary").await;
    for role in Role::ALL {
        let st = role_status(&lone, role);
        assert!(st.holds, "{role:?}");
        assert_eq!(st.standing, Standing::Holding);
        assert_eq!(st.blocked, None);
        assert!(st.request.is_none() && st.asking.is_none() && st.notice.is_none());
        assert!(st.always.is_empty());
    }
    assert!(roles_of(&lone).operator_ready);
    // With no screens set up either: still the same.
    lone.node.set_screens(false);
    assert_eq!(lone.node.role(), Mode::Presenter);
    assert_eq!(lone.node.driving(), None);
    assert!(!lone.node.driven());
    let status = lone.node.status();
    assert_eq!(status.presenter.standing, Standing::Holding);
    assert!(status.presenter.requests.is_empty());
    assert!(status.presenter.asking.is_none() && status.presenter.notice.is_none());
    let drive = status.presenter.drive;
    assert!(drive.driving.is_none() && drive.active.is_none());
    assert!(drive.requests.is_empty() && drive.asking.is_none());
    assert!(!drive.can_drive, "no button to drive");
    assert!(drive.always.is_empty() && drive.notice.is_none());
    assert!(status.paired.is_empty() && status.nearby.is_empty());
    // Asking or taking anything is "already yours".
    for role in Role::ALL {
        assert_eq!(lone.node.ask_role(role), Err(Denied::Already), "{role:?}");
    }
    assert!(lone.node.drive_ask().is_err());
    // Nothing is written for roles until something is claimed or handed over.
    assert!(!lone._dir.path().join("sync/roles.json").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn every_role_is_asked_for_allowed_and_declined_the_same_way() {
    let (sanctuary, office) = paired_and_leased().await;
    sanctuary.node.claim_role(Role::Stream).unwrap();
    until(Duration::from_secs(5), "Stream to be known", || {
        holder_of(&office, Role::Stream) == Some(sanctuary.node.device())
    })
    .await;
    // Projection last: whoever holds it may take the Operator seat without
    // asking, so the office asks for Operator before it holds Projection.
    let expected = [
        (Role::Stream, OnAir::Nothing),
        (Role::Audio, OnAir::Music),
        (Role::Operator, OnAir::Nothing),
        (Role::Projection, OnAir::Screens),
    ];
    for (role, on_air) in expected {
        office.node.ask_role(role).unwrap();
        assert_eq!(
            role_status(&office, role).asking.unwrap().state,
            AskState::Waiting
        );
        until(Duration::from_secs(5), "the prompt", || {
            role_status(&sanctuary, role).request.is_some()
        })
        .await;
        let asker = role_status(&sanctuary, role).request.unwrap();
        assert_eq!(
            (asker.name.as_str(), asker.device),
            ("Office PC", office.node.device())
        );

        // Not now.
        sanctuary.node.decline_role(role, asker.device);
        until(Duration::from_secs(5), "the answer", || {
            role_status(&office, role)
                .asking
                .is_some_and(|a| a.state == AskState::Declined)
        })
        .await;
        assert_eq!(holder_of(&sanctuary, role), Some(sanctuary.node.device()));
        assert!(role_status(&sanctuary, role).request.is_none());

        // Ask again; this time allowed, and it says what it takes off air.
        office.node.ask_role(role).unwrap();
        until(Duration::from_secs(5), "the prompt again", || {
            role_status(&sanctuary, role).request.is_some()
        })
        .await;
        let got = sanctuary
            .node
            .allow_role(role, office.node.device(), true)
            .unwrap();
        assert_eq!(got, on_air, "{role:?}");
        assert!(
            !role_status(&sanctuary, role).holds,
            "at once, on the old holder"
        );
        until(Duration::from_secs(5), "the office PC to hold it", || {
            role_status(&office, role).holds
        })
        .await;
        let mine = role_status(&office, role);
        assert_eq!(mine.record.epoch, 2, "{role:?}");
        assert_eq!(mine.asking, None);
        // Handing over was the old holder's choice: no notice.
        assert_eq!(role_status(&sanctuary, role).notice, None);
        // "Always allow" is remembered for Operator and Stream, never for
        // Projection or Audio.
        assert_eq!(
            role_status(&sanctuary, role).always.len(),
            usize::from(matches!(role, Role::Operator | Role::Stream)),
            "{role:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ask_can_be_withdrawn_and_only_a_holder_can_allow() {
    let (sanctuary, office) = paired_and_leased().await;
    office.node.ask_role(Role::Audio).unwrap();
    until(Duration::from_secs(5), "the prompt", || {
        role_status(&sanctuary, Role::Audio).request.is_some()
    })
    .await;
    office.node.cancel_role_ask(Role::Audio);
    until(Duration::from_secs(5), "the prompt to go", || {
        role_status(&sanctuary, Role::Audio).request.is_none()
    })
    .await;
    assert_eq!(role_status(&office, Role::Audio).asking, None);
    // Allowing a device that is not asking does nothing.
    assert!(sanctuary
        .node
        .allow_role(Role::Audio, office.node.device(), false)
        .is_err());
    // The holder cannot ask itself, and a device cannot allow a role it
    // does not hold.
    assert_eq!(sanctuary.node.ask_role(Role::Audio), Err(Denied::Already));
    assert!(office
        .node
        .allow_role(Role::Audio, sanctuary.node.device(), false)
        .is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cut_network_cannot_split_a_role_between_two_devices() {
    let (mut sanctuary, mut office) = paired_and_leased().await;
    // Nobody holds Stream; both take it while apart.
    sanctuary.cut();
    office.cut();
    until(Duration::from_secs(20), "disconnected", || {
        office.node.status().connected() == 0 && sanctuary.node.status().connected() == 0
    })
    .await;
    sanctuary.node.claim_role(Role::Stream).unwrap();
    office.node.claim_role(Role::Stream).unwrap();
    assert!(role_status(&sanctuary, Role::Stream).holds);
    assert!(role_status(&office, Role::Stream).holds);
    let winner = sanctuary.node.device().max(office.node.device());
    sanctuary.restore().await;
    office.restore().await;
    until(Duration::from_secs(20), "both to agree", || {
        holder_of(&sanctuary, Role::Stream) == Some(winner)
            && holder_of(&office, Role::Stream) == Some(winner)
    })
    .await;
    // Same epoch, the higher device key; the other is told it lost.
    let (win, lose) = if winner == sanctuary.node.device() {
        (&sanctuary, &office)
    } else {
        (&office, &sanctuary)
    };
    assert_eq!(role_status(win, Role::Stream).record.epoch, 1);
    assert!(role_status(win, Role::Stream).holds);
    assert!(!role_status(lose, Role::Stream).holds);
    assert!(role_status(lose, Role::Stream).notice.is_some());
    assert!(role_status(win, Role::Stream).notice.is_none());
    // The other roles were not touched.
    assert_eq!(
        holder_of(&office, Role::Audio),
        Some(sanctuary.node.device())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn takeover_of_any_role_waits_for_the_timeout_and_the_old_holder_yields_on_return() {
    let (mut sanctuary, mut office) = paired_and_leased().await;
    // A claim against a device is judged by that device's own takeover time.
    sanctuary.node.set_takeover_after(Duration::from_secs(2));
    office.node.set_takeover_after(Duration::from_secs(2));
    sanctuary.cut();
    office.cut();
    until(Duration::from_secs(20), "disconnected", || {
        office.node.status().connected() == 0
    })
    .await;
    for role in [Role::Audio, Role::Operator, Role::Projection] {
        match office.node.claim_role(role) {
            Err(Denied::TooSoon { .. }) => {}
            other => panic!("{role:?}: expected TooSoon, got {other:?}"),
        }
        assert!(matches!(
            role_status(&office, role).standing,
            Standing::Wait { .. }
        ));
        assert_eq!(office.node.ask_role(role), Err(Denied::NobodyToAsk));
    }
    let began = Instant::now();
    while !matches!(
        role_status(&office, Role::Audio).standing,
        Standing::TakeOver { .. }
    ) {
        assert!(
            began.elapsed() < Duration::from_secs(40),
            "timed out waiting for takeover to open"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    // Only Audio is taken: the roles are independent.
    office.node.claim_role(Role::Audio).unwrap();
    assert!(role_status(&office, Role::Audio).holds);
    assert!(!role_status(&office, Role::Projection).holds);
    assert_eq!(role_status(&office, Role::Audio).record.epoch, 2);

    // The old holder, still believing it holds Audio, comes back.
    assert!(role_status(&sanctuary, Role::Audio).holds);
    office.restore().await;
    sanctuary.restore().await;
    until(Duration::from_secs(15), "the old holder to yield", || {
        !role_status(&sanctuary, Role::Audio).holds
    })
    .await;
    assert_eq!(
        role_status(&sanctuary, Role::Audio).notice,
        Some(Notice::Replaced {
            by: "Office PC".into()
        })
    );
    sanctuary.node.dismiss_role_notice(Role::Audio);
    assert_eq!(role_status(&sanctuary, Role::Audio).notice, None);
    assert!(role_status(&office, Role::Audio).holds, "it never resumes");
    // It kept the roles nobody took.
    assert!(role_status(&sanctuary, Role::Projection).holds);
    assert!(role_status(&sanctuary, Role::Operator).holds);
}

#[tokio::test(flavor = "multi_thread")]
async fn projection_is_refused_on_a_device_with_no_screens() {
    let (sanctuary, office) = paired_and_leased().await;
    office.node.set_screens(false);
    assert_eq!(
        role_status(&office, Role::Projection).blocked,
        Some(Blocked::NoScreens)
    );
    assert_eq!(
        office.node.ask_to_present(),
        Err(Denied::NoScreens),
        "it cannot even ask"
    );
    assert_eq!(office.node.claim_presenter(), Err(Denied::NoScreens));
    assert_eq!(
        Denied::NoScreens.to_string(),
        "No screens are set up on this device"
    );
    assert!(role_status(&sanctuary, Role::Projection).request.is_none());
    // The other roles do not need screens.
    office.node.ask_role(Role::Audio).unwrap();
    // Setting screens up opens the way.
    office.node.set_screens(true);
    assert_eq!(role_status(&office, Role::Projection).blocked, None);
    office.node.ask_to_present().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pairing_device_with_no_screens_takes_no_role_and_operator_waits_for_projection() {
    let laptop = peer("Laptop").await;
    laptop.node.set_screens(false);
    let projector = peer("Projector PC").await;
    let code = laptop.node.start_pairing();
    projector
        .node
        .pair(&code.code, Some(&laptop.address()))
        .await
        .unwrap();
    until(Duration::from_secs(5), "both connected", || {
        laptop.node.status().connected() == 1 && projector.node.status().connected() == 1
    })
    .await;
    until(Duration::from_secs(5), "both to have been told", || {
        laptop.node.settled() && projector.node.settled()
    })
    .await;
    for role in Role::ALL {
        assert_eq!(holder_of(&laptop, role), None, "{role:?}");
        assert_eq!(holder_of(&projector, role), None, "{role:?}");
    }
    // Neither can run the service while nobody runs the screens...
    assert_eq!(
        projector.node.claim_role(Role::Operator),
        Err(Denied::NeedsProjection)
    );
    assert_eq!(
        laptop.node.ask_role(Role::Operator),
        Err(Denied::NeedsProjection)
    );
    assert_eq!(
        role_status(&projector, Role::Operator).blocked,
        Some(Blocked::NeedsProjection)
    );
    assert!(!projector.node.roles_status().operator_ready);
    // ...and the laptop cannot take the screens.
    assert_eq!(
        laptop.node.claim_role(Role::Projection),
        Err(Denied::NoScreens)
    );
    // The projector PC takes them with one click, then the Operator seat
    // can be taken from anywhere.
    projector.node.claim_role(Role::Projection).unwrap();
    until(Duration::from_secs(5), "the laptop to know", || {
        holder_of(&laptop, Role::Projection) == Some(projector.node.device())
    })
    .await;
    assert!(laptop.node.roles_status().operator_ready);
    laptop.node.claim_role(Role::Operator).unwrap();
    until(Duration::from_secs(5), "the laptop to drive", || {
        laptop.node.driving().is_some_and(|d| d.online)
    })
    .await;
    // Stream and Audio need nothing.
    laptop.node.claim_role(Role::Stream).unwrap();
    laptop.node.claim_role(Role::Audio).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_operator_controls_wait_without_projection_and_the_seat_is_kept() {
    let (sanctuary, office, ..) = driving_pair().await;
    assert!(office.node.roles_status().operator_ready);
    // The office has another device paired, so it is not alone when the
    // sanctuary (the Projection holder) goes.
    let third = peer("Back room").await;
    let code = office.node.start_pairing();
    third
        .node
        .pair(&code.code, Some(&office.address()))
        .await
        .unwrap();
    until(Duration::from_secs(5), "three devices", || {
        office.node.status().connected() == 2
    })
    .await;
    office.node.unpair(sanctuary.node.device()).unwrap();
    assert!(
        !office.node.roles_status().operator_ready,
        "nobody it knows holds Projection"
    );
    assert_eq!(
        role_status(&office, Role::Projection).standing,
        Standing::Open
    );
    assert!(
        role_status(&office, Role::Operator).holds,
        "the seat is kept"
    );
    assert!(office.node.drive_send(b"x".to_vec()).is_err());
    // The office has screens, so it may take Projection itself; with
    // Projection held again the controls wake.
    office
        .node
        .claim_role(Role::Projection)
        .expect("the office claims Projection");
    assert!(office.node.roles_status().operator_ready);
    assert!(role_status(&office, Role::Operator).holds);
}

#[tokio::test(flavor = "multi_thread")]
async fn roles_and_the_always_list_survive_a_restart() {
    let (sanctuary, office) = paired_and_leased().await;
    sanctuary.node.claim_role(Role::Stream).unwrap();
    until(Duration::from_secs(5), "Stream to be known", || {
        holder_of(&office, Role::Stream).is_some()
    })
    .await;
    office.node.ask_role(Role::Stream).unwrap();
    until(Duration::from_secs(5), "the prompt", || {
        role_status(&sanctuary, Role::Stream).request.is_some()
    })
    .await;
    sanctuary
        .node
        .allow_role(Role::Stream, office.node.device(), true)
        .unwrap();
    sanctuary.node.set_takeover_after(Duration::from_secs(600));
    let dir = sanctuary._dir.path().join("sync");
    until(Duration::from_secs(5), "roles.json", || {
        std::fs::read_to_string(dir.join("roles.json"))
            .is_ok_and(|j| j.contains("\"takeover_secs\": 600") && j.contains("Office PC"))
    })
    .await;
    let again = SyncNode::new(
        sanctuary.node.repo().clone(),
        key_in(sanctuary._dir.path()),
        NodeConfig {
            name: "Sanctuary".into(),
            dir,
        },
    )
    .unwrap();
    let r = again.status().roles;
    assert_eq!(r.projection.record.holder, Some(sanctuary.node.device()));
    assert_eq!(r.audio.record.holder, Some(sanctuary.node.device()));
    assert_eq!(r.stream.record.holder, Some(office.node.device()));
    assert_eq!(r.stream.record.epoch, 2);
    assert_eq!(r.stream.always.len(), 1);
    assert_eq!(r.takeover_after, Duration::from_secs(600));
    assert!(r.projection.always.is_empty() && r.audio.always.is_empty());
}

// ---- Roles: moving over from 0.7.0 --------------------------------------------

/// A copy of the folders a real 0.7.0 pair wrote (tests/fixtures/v0.7.0): the
/// sanctuary handed the presenter lease to the office PC (epoch 2), allowed
/// it to drive "always" and set the takeover time to five minutes.
fn old_device(name: &str) -> tempfile::TempDir {
    let from = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/v0.7.0")
        .join(name);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("sync")).unwrap();
    for file in std::fs::read_dir(from).unwrap() {
        let file = file.unwrap();
        let to = if file.file_name() == "device.key" {
            dir.path().join("device.key")
        } else {
            dir.path().join("sync").join(file.file_name())
        };
        std::fs::copy(file.path(), to).unwrap();
    }
    dir
}

const OFFICE_KEY: &str = "2ac160d95ab8b26c2789f247317a67f3d4d5efe707b578d952bd9a1f8af72389";

#[tokio::test(flavor = "multi_thread")]
async fn a_0_7_0_pair_moves_to_roles_and_nothing_is_lost() {
    let sanctuary = peer_in("Sanctuary", old_device("sanctuary")).await;
    let office = peer_in("Office PC", old_device("office")).await;
    let office_key = DeviceKey::from_hex(OFFICE_KEY).unwrap();
    assert_eq!(office.node.device(), office_key, "the fixture is a pair");

    for (p, name) in [(&sanctuary, "sanctuary"), (&office, "office")] {
        let r = roles_of(p);
        // The lease became Projection, keeping its epoch and holder.
        assert_eq!(r.projection.record.holder, Some(office_key), "{name}");
        assert_eq!(r.projection.record.name, "Office PC");
        assert_eq!(r.projection.record.epoch, 2, "the epoch is kept");
        // The driver seat became Operator, with the sound where the screens
        // are, and Stream unheld.
        assert_eq!(r.operator.record, r.projection.record, "{name}");
        assert_eq!(r.audio.record, r.projection.record, "{name}");
        assert_eq!(r.stream.record, Record::default(), "{name}");
        // Nothing paired was lost.
        assert_eq!(p.node.status().paired.len(), 1, "{name}");
        assert_eq!(
            p.node.status().paired[0].presenting,
            name == "sanctuary",
            "{name}"
        );
    }
    // The takeover time and the always-allowed list carried over.
    let r = roles_of(&sanctuary);
    assert_eq!(r.takeover_after, Duration::from_secs(300));
    assert_eq!(r.operator.always.len(), 1);
    assert_eq!(r.operator.always[0].device, office_key);
    assert_eq!(r.operator.always[0].name, "Office PC");
    assert!(r.stream.always.is_empty());
    assert_eq!(roles_of(&office).takeover_after, Duration::from_secs(120));
    // The views the app reads say the same as 0.7.0 would have.
    assert_eq!(sanctuary.node.role(), Mode::Planning);
    assert_eq!(office.node.role(), Mode::Presenter);
    assert_eq!(sanctuary.node.status().presenter.drive.always.len(), 1);

    // roles.json was written; the old files are left exactly as they were.
    for (p, name) in [(&sanctuary, "sanctuary"), (&office, "office")] {
        let sync = p._dir.path().join("sync");
        assert!(sync.join("roles.json").is_file(), "{name}");
        let original = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/v0.7.0")
            .join(name);
        for old in ["presenter.json", "driver.json"] {
            if original.join(old).exists() {
                assert_eq!(
                    std::fs::read(sync.join(old)).unwrap(),
                    std::fs::read(original.join(old)).unwrap(),
                    "{name}/{old}"
                );
            }
        }
    }

    // They still connect and agree.
    office
        .node
        .connect(&sanctuary.address())
        .await
        .expect("a 0.7.0 pair, both moved over, connects");
    until(Duration::from_secs(10), "both connected", || {
        sanctuary.node.status().connected() == 1 && office.node.status().connected() == 1
    })
    .await;
    assert_eq!(roles_of(&sanctuary).projection.record.epoch, 2);
    assert_eq!(roles_of(&office).projection.record.epoch, 2);

    // The old always-allowed entry works on the new roles. The sanctuary
    // asks for the screens back and is allowed...
    sanctuary.node.set_screens(true);
    sanctuary.node.ask_to_present().unwrap();
    until(
        Duration::from_secs(5),
        "the prompt on the office PC",
        || !office.node.status().presenter.requests.is_empty(),
    )
    .await;
    office
        .node
        .allow_presenter(sanctuary.node.device())
        .unwrap();
    until(Duration::from_secs(5), "the sanctuary to present", || {
        sanctuary.node.role() == Mode::Presenter
    })
    .await;
    // ...and takes the Operator seat back, as the one that presents.
    sanctuary.node.drive_take_back();
    until(Duration::from_secs(5), "the office PC to know", || {
        holder_of(&office, Role::Operator) == Some(sanctuary.node.device())
    })
    .await;
    // The office PC, which the sanctuary had marked "always allow" in
    // 0.7.0, now drives it without being allowed again.
    office.node.drive_ask().unwrap();
    until(Duration::from_secs(5), "the office PC to drive", || {
        office.node.driving().is_some_and(|d| d.online)
    })
    .await;
    assert!(sanctuary.node.status().presenter.drive.requests.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_0_7_0_device_with_nobody_paired_holds_all_four_whatever_the_old_lease_says() {
    // The office PC's files, but with its pairing gone.
    let dir = old_device("office");
    std::fs::remove_file(dir.path().join("sync/paired.json")).unwrap();
    let lone = peer_in("Office PC", dir).await;
    let r = roles_of(&lone);
    assert_eq!(r.projection.record.epoch, 2, "the record is kept");
    for role in Role::ALL {
        assert!(role_status(&lone, role).holds, "{role:?}");
    }
    assert_eq!(lone.node.role(), Mode::Presenter);
    // Another 0.7.0 device: the sanctuary alone, which was not the holder.
    let dir = old_device("sanctuary");
    std::fs::remove_file(dir.path().join("sync/paired.json")).unwrap();
    let lone = peer_in("Sanctuary", dir).await;
    assert_eq!(lone.node.role(), Mode::Presenter);
    for role in Role::ALL {
        assert!(role_status(&lone, role).holds, "{role:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_device_on_the_protocol_before_roles_is_refused_by_a_running_node() {
    let (sanctuary, _office) = paired_and_leased().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/sync", sanctuary.addr))
        .await
        .unwrap();
    // A hello in protocol 4: the first two numbers of every version's hello
    // are the frame kind (0) and the protocol.
    ws.send(Message::Binary(vec![0u8, 4].into())).await.unwrap();
    let mut reason = None;
    while let Some(Ok(message)) = ws.next().await {
        if let Message::Binary(bytes) = message {
            reason = Some(String::from_utf8_lossy(&bytes).into_owned());
            break;
        }
    }
    let reason = reason.expect("the node answers");
    assert!(
        reason.contains("the other device runs an older Lampstand; update it"),
        "{reason}"
    );
    assert_eq!(sanctuary.node.status().paired.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_operator_seat_is_kept_across_a_projection_hand_over_and_the_new_holder_announces() {
    let (sanctuary, office, on_sanctuary, on_office) = driving_pair().await;
    // A back-room machine with a projector, paired with both.
    let back = peer("Back room").await;
    let on_back = watch(&back);
    for other in [&sanctuary, &office] {
        let code = other.node.start_pairing();
        back.node
            .pair(&code.code, Some(&other.address()))
            .await
            .unwrap();
    }
    until(Duration::from_secs(5), "the back room to know", || {
        holder_of(&back, Role::Operator) == Some(office.node.device())
            && back.node.status().connected() == 2
    })
    .await;
    assert_eq!(*on_sanctuary.driver.lock().unwrap(), vec![true]);

    // Projection moves to the back room.
    back.node.ask_to_present().unwrap();
    until(Duration::from_secs(5), "the prompt", || {
        !presenter_of(&sanctuary).requests.is_empty()
    })
    .await;
    sanctuary.node.allow_presenter(back.node.device()).unwrap();
    until(Duration::from_secs(5), "the back room to present", || {
        back.node.role() == Mode::Presenter
    })
    .await;

    // The Operator seat is where it was...
    assert_eq!(
        holder_of(&office, Role::Operator),
        Some(office.node.device())
    );
    // ...it now operates the back room, which announces its state to it...
    until(
        Duration::from_secs(5),
        "the office PC to drive the back room",
        || {
            office
                .node
                .driving()
                .is_some_and(|d| d.online && d.presenter == back.node.device())
        },
    )
    .await;
    until(Duration::from_secs(5), "the announcement", || {
        *on_back.driver.lock().unwrap() == vec![true]
    })
    .await;
    assert!(back.node.driven());
    // ...and the sanctuary is no longer driven.
    assert!(!sanctuary.node.driven());
    assert_eq!(*on_sanctuary.driver.lock().unwrap(), vec![true, false]);

    // Commands now go to the back room, and the sanctuary would ignore them.
    office
        .node
        .drive_send(b"{\"Live\":\"Next\"}".to_vec())
        .unwrap();
    until(Duration::from_secs(5), "the command", || {
        !on_back.commands.lock().unwrap().is_empty()
    })
    .await;
    assert_eq!(on_back.commands.lock().unwrap()[0].0, office.node.device());
    assert!(on_sanctuary.commands.lock().unwrap().is_empty());
    back.node.drive_live(b"live".to_vec());
    until(Duration::from_secs(5), "the live state", || {
        !on_office.live.lock().unwrap().is_empty()
    })
    .await;
    assert_eq!(on_office.live.lock().unwrap()[0], b"live".to_vec());
    // A command sent to the old holder by mistake does nothing there.
    sanctuary.node.drive_live(b"stale".to_vec());
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(on_office.live.lock().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_operator_can_send_commands() {
    let (sanctuary, office, on_sanctuary, _) = driving_pair().await;
    // The Projection holder's own side does not send commands to itself,
    // and a device that is not the Operator cannot send any.
    assert!(sanctuary.node.drive_send(b"x".to_vec()).is_err());
    // Even a frame forged by a paired device that is not the Operator is
    // ignored: the office is the Operator, so send as another device.
    let tablet = peer("Sam's tablet").await;
    let code = sanctuary.node.start_pairing();
    tablet
        .node
        .pair(&code.code, Some(&sanctuary.address()))
        .await
        .unwrap();
    until(Duration::from_secs(5), "the tablet to know", || {
        holder_of(&tablet, Role::Operator) == Some(office.node.device())
            && tablet.node.status().connected() >= 1
    })
    .await;
    assert!(tablet.node.drive_send(b"x".to_vec()).is_err());
    office.node.drive_send(b"ok".to_vec()).unwrap();
    until(Duration::from_secs(5), "the command", || {
        !on_sanctuary.commands.lock().unwrap().is_empty()
    })
    .await;
    assert_eq!(on_sanctuary.commands.lock().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_while_the_projection_holder_is_away_lets_the_seat_go_at_once() {
    let (mut sanctuary, office, ..) = driving_pair().await;
    sanctuary.cut();
    until(
        Duration::from_secs(10),
        "the presenter to be offline",
        || drive_of(&office).driving.is_some_and(|d| !d.online),
    )
    .await;
    office.node.drive_cancel();
    assert!(drive_of(&office).driving.is_none(), "at once, as in 0.7.0");
    assert_eq!(holder_of(&office, Role::Operator), None);
    assert_eq!(drive_of(&office).notice, None);
    // The seat is open for anyone when they meet again, and nothing is
    // left stuck: the office can ask to drive again.
    sanctuary.restore().await;
    until(Duration::from_secs(15), "reconnecting", || {
        sanctuary.node.status().connected() == 1 && office.node.status().connected() == 1
    })
    .await;
    until(Duration::from_secs(10), "the seat to be open", || {
        holder_of(&sanctuary, Role::Operator).is_none()
    })
    .await;
    assert!(!sanctuary.node.driven());
    office.node.drive_ask().unwrap();
    until(Duration::from_secs(10), "driving again", || {
        drive_of(&office).driving.is_some_and(|d| d.online)
    })
    .await;
}

/// `asker` asks `holder` for `role`, again if a connection changed under the
/// ask, until the holder shows the prompt.
async fn ask_until_prompted(asker: &Peer, holder: &Peer, role: Role) {
    let began = Instant::now();
    while role_status(holder, role).request.is_none() {
        assert!(began.elapsed() < Duration::from_secs(15), "no prompt");
        asker.node.ask_role(role).unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hand_over_between_two_devices_reaches_a_third_paired_with_only_one_of_them() {
    // t is paired only with s. o and x are paired with s, and with each other.
    let s = peer("Sanctuary").await;
    let o = peer("Office PC").await;
    let x = peer("Back room").await;
    let t = peer("Tablet").await;
    for (shows, joins) in [(&s, &o), (&s, &x), (&s, &t), (&o, &x)] {
        let code = shows.node.start_pairing();
        joins
            .node
            .pair(&code.code, Some(&shows.address()))
            .await
            .unwrap();
    }
    until(Duration::from_secs(10), "everyone connected", || {
        s.node.status().connected() == 3
            && o.node.status().connected() == 2
            && x.node.status().connected() == 2
            && t.node.status().connected() == 1
    })
    .await;
    // s takes Stream; o asks and is allowed; x asks o and is allowed.
    s.node.claim_role(Role::Stream).unwrap();
    until(Duration::from_secs(5), "Stream to be known", || {
        holder_of(&o, Role::Stream) == Some(s.node.device())
            && holder_of(&x, Role::Stream) == Some(s.node.device())
    })
    .await;
    ask_until_prompted(&o, &s, Role::Stream).await;
    s.node
        .allow_role(Role::Stream, o.node.device(), false)
        .unwrap();
    until(Duration::from_secs(5), "the tablet to hear of o", || {
        holder_of(&t, Role::Stream) == Some(o.node.device())
    })
    .await;
    ask_until_prompted(&x, &o, Role::Stream).await;
    o.node
        .allow_role(Role::Stream, x.node.device(), false)
        .unwrap();
    // The tablet knows neither o nor x; s passes the news on.
    until(Duration::from_secs(10), "the tablet to hear of x", || {
        holder_of(&t, Role::Stream) == Some(x.node.device())
    })
    .await;
    assert_eq!(role_status(&t, Role::Stream).record.epoch, 3);
    assert!(!role_status(&t, Role::Stream).unverified);
    assert_eq!(
        role_status(&t, Role::Stream).record.name,
        "",
        "no name for a device it has not paired with"
    );
    assert_eq!(holder_of(&s, Role::Stream), Some(x.node.device()));
}

/// A device cut off while two others hand a role back and forth, and then
/// catching up when it meets them. The helper network cannot cut a device
/// that dials out, so the tablet is paired only with the sanctuary and the
/// sanctuary is cut with it: the sanctuary finds the office PC again through
/// the address it was given (`connect`), and the tablet has nobody to reach.
#[tokio::test(flavor = "multi_thread")]
async fn a_device_cut_off_during_hand_overs_catches_up_when_it_meets_them_again() {
    let (mut s, o) = paired_and_leased().await;
    let mut t = peer("Tablet").await;
    let code = s.node.start_pairing();
    t.node.pair(&code.code, Some(&s.address())).await.unwrap();
    // The sanctuary can dial the office PC, not only be dialled by it.
    s.node.connect(&o.address()).await.unwrap();
    until(Duration::from_secs(10), "the tablet to know", || {
        t.node.status().connected() == 1 && holder_of(&t, Role::Audio) == Some(s.node.device())
    })
    .await;
    s.cut_listener();
    t.cut();
    until(Duration::from_secs(20), "the tablet to be cut off", || {
        t.node.status().connected() == 0
    })
    .await;
    until(
        Duration::from_secs(30),
        "the sanctuary and office to meet",
        || s.node.status().connected() == 1 && o.node.status().connected() == 1,
    )
    .await;
    // While the tablet is away Audio goes s -> o -> s -> o.
    let mut at_s = true;
    for _ in 0..3 {
        let (from, to) = if at_s { (&s, &o) } else { (&o, &s) };
        // The ask is made again if the connection changed under it.
        let began = Instant::now();
        while role_status(from, Role::Audio).request.is_none() {
            assert!(began.elapsed() < Duration::from_secs(15), "no prompt");
            to.node.ask_role(Role::Audio).unwrap();
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        from.node
            .allow_role(Role::Audio, to.node.device(), false)
            .unwrap();
        until(Duration::from_secs(5), "the hand-over to be known", || {
            holder_of(to, Role::Audio) == Some(to.node.device())
        })
        .await;
        at_s = !at_s;
    }
    assert_eq!(holder_of(&t, Role::Audio), Some(s.node.device()));
    t.restore().await;
    s.restore().await;
    until(Duration::from_secs(30), "the tablet to catch up", || {
        holder_of(&t, Role::Audio) == Some(o.node.device())
    })
    .await;
    assert_eq!(role_status(&t, Role::Audio).record.epoch, 4);
    assert!(!role_status(&t, Role::Audio).unverified);
    // The tablet knows the sanctuary (paired), not the office PC.
    assert_eq!(role_status(&t, Role::Audio).record.name, "");
}

/// The sanctuary goes down and comes back as a new process: it remembers
/// nothing of this run, only what it kept on disk about when it last heard
/// from the office PC. The office PC took the screens, the service and the
/// sound while it was gone, and the sanctuary must yield all three.
#[tokio::test(flavor = "multi_thread")]
async fn a_holder_that_restarted_yields_every_role_taken_while_it_was_down() {
    let (mut sanctuary, mut office) = paired_and_leased().await;
    sanctuary.node.set_takeover_after(Duration::from_secs(2));
    office.node.set_takeover_after(Duration::from_secs(2));
    sanctuary.cut();
    office.cut();
    until(Duration::from_secs(20), "disconnected", || {
        office.node.status().connected() == 0 && sanctuary.node.status().connected() == 0
    })
    .await;
    let began = Instant::now();
    while !matches!(
        role_status(&office, Role::Projection).standing,
        Standing::TakeOver { .. }
    ) {
        assert!(
            began.elapsed() < Duration::from_secs(40),
            "takeover never opened"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    for role in [Role::Projection, Role::Operator, Role::Audio] {
        office.node.claim_role(role).unwrap();
    }
    // The sanctuary restarts: this run knows nothing of the office PC.
    sanctuary.node.forget_this_runs_contacts();
    office.restore().await;
    sanctuary.restore().await;
    until(Duration::from_secs(20), "the sanctuary to yield", || {
        [Role::Projection, Role::Operator, Role::Audio]
            .iter()
            .all(|r| !role_status(&sanctuary, *r).holds)
    })
    .await;
    for role in [Role::Projection, Role::Operator, Role::Audio] {
        assert_eq!(
            holder_of(&sanctuary, role),
            Some(office.node.device()),
            "{role:?}"
        );
        assert!(role_status(&sanctuary, role).notice.is_some(), "{role:?}");
    }
    assert_eq!(sanctuary.node.role(), Mode::Planning);
    assert_eq!(office.node.role(), Mode::Presenter);
}

/// Once a connection is older than the window in which a claim made while
/// apart is believed, a paired device is still a known device: it can take
/// a role nobody holds, and every device accepts it.
#[tokio::test(flavor = "multi_thread")]
async fn a_claim_on_a_connection_older_than_ten_seconds_is_still_taken_in() {
    let (sanctuary, office) = paired_and_leased().await;
    // The default window, not a shortened one.
    tokio::time::sleep(Duration::from_secs(11)).await;
    assert_eq!(sanctuary.node.status().connected(), 1);
    office.node.claim_role(Role::Stream).unwrap();
    until(Duration::from_secs(10), "the sanctuary to know", || {
        holder_of(&sanctuary, Role::Stream) == Some(office.node.device())
    })
    .await;
    assert_eq!(role_status(&sanctuary, Role::Stream).record.epoch, 1);
    assert!(!role_status(&sanctuary, Role::Stream).unverified);
    // The sanctuary hands Audio to the office PC, which hands it back.
    ask_until_prompted(&office, &sanctuary, Role::Audio).await;
    sanctuary
        .node
        .allow_role(Role::Audio, office.node.device(), false)
        .unwrap();
    until(Duration::from_secs(5), "the hand-over", || {
        holder_of(&sanctuary, Role::Audio) == Some(office.node.device())
    })
    .await;
}

/// The sanctuary's laptop sleeps with its connection still registered: its
/// monotonic clock stands still, the wall clock does not, and the connection
/// ends only when it wakes and the network is cut. The office PC took the
/// roles while it slept. The time since the sanctuary last heard from the
/// office PC is the time of the last frame, not the time the connection
/// ended, so it yields on waking.
#[tokio::test(flavor = "multi_thread")]
async fn a_holder_whose_laptop_slept_with_a_live_connection_yields_on_waking() {
    let (mut sanctuary, mut office) = paired_and_leased().await;
    // The sanctuary waits a minute before it lets go; the office PC only two
    // seconds. Only the sleep makes the sanctuary's absence long enough.
    sanctuary.node.set_takeover_after(Duration::from_secs(60));
    office.node.set_takeover_after(Duration::from_secs(2));
    // Ten minutes asleep, with the connection up.
    sanctuary.node.skip_wall_clock(Duration::from_secs(600));
    sanctuary.cut();
    office.cut();
    until(Duration::from_secs(20), "disconnected", || {
        office.node.status().connected() == 0 && sanctuary.node.status().connected() == 0
    })
    .await;
    let began = Instant::now();
    while !matches!(
        role_status(&office, Role::Projection).standing,
        Standing::TakeOver { .. }
    ) {
        assert!(
            began.elapsed() < Duration::from_secs(40),
            "takeover never opened"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    for role in [Role::Projection, Role::Operator, Role::Audio] {
        office.node.claim_role(role).unwrap();
    }
    office.restore().await;
    sanctuary.restore().await;
    until(Duration::from_secs(20), "the sanctuary to yield", || {
        [Role::Projection, Role::Operator, Role::Audio]
            .iter()
            .all(|r| !role_status(&sanctuary, *r).holds)
    })
    .await;
    assert_eq!(sanctuary.node.role(), Mode::Planning);
    assert_eq!(office.node.role(), Mode::Presenter);
}

/// Unpairing a device that is connected is not undone by its connection
/// ending: nothing about when it was last heard is kept or saved.
#[tokio::test(flavor = "multi_thread")]
async fn unpairing_a_connected_device_leaves_nothing_about_it_behind() {
    let (sanctuary, office) = paired_and_leased().await;
    let office_hex = office.node.device().to_hex();
    // Let a save happen while paired.
    until(Duration::from_secs(10), "a contact time on disk", || {
        std::fs::read_to_string(sanctuary._dir.path().join("sync/contact.json"))
            .is_ok_and(|j| j.contains(&office_hex))
    })
    .await;
    sanctuary.node.unpair(office.node.device()).unwrap();
    until(Duration::from_secs(10), "its connection to end", || {
        sanctuary.node.status().connected() == 0
    })
    .await;
    tokio::time::sleep(Duration::from_secs(7)).await;
    let kept = std::fs::read_to_string(sanctuary._dir.path().join("sync/contact.json"))
        .unwrap_or_default();
    assert!(!kept.contains(&office_hex), "{kept}");
}

/// An ask that never arrived is made again by the connection that opens next,
/// with nobody pressing the button twice.
#[tokio::test(flavor = "multi_thread")]
async fn an_ask_that_was_lost_is_made_again_when_the_connection_comes_back() {
    let (mut sanctuary, mut office) = paired_and_leased().await;
    sanctuary.node.lose_asks(true);
    office.node.ask_role(Role::Audio).unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(role_status(&sanctuary, Role::Audio).request.is_none());
    assert_eq!(
        role_status(&office, Role::Audio).asking.map(|a| a.state),
        Some(AskState::Waiting)
    );
    // The frames of this run are lost; the connection is lost with them.
    sanctuary.node.lose_asks(false);
    sanctuary.cut();
    office.cut();
    until(Duration::from_secs(20), "disconnected", || {
        office.node.status().connected() == 0 && sanctuary.node.status().connected() == 0
    })
    .await;
    office.restore().await;
    sanctuary.restore().await;
    until(Duration::from_secs(30), "the ask to arrive again", || {
        role_status(&sanctuary, Role::Audio).request.is_some()
    })
    .await;
}

// ---- The Stream holder owns OBS: scene cues -------------------------------------

use lampstand_sync::net::CueRoute;
use std::sync::Mutex as StdMutex;

/// Records the scenes a device is told to switch OBS to.
fn applying_cues(p: &Peer) -> Arc<StdMutex<Vec<Option<String>>>> {
    let applied = Arc::new(StdMutex::new(Vec::new()));
    let a = applied.clone();
    p.node
        .set_scene_cue_hook(Arc::new(move |scene| a.lock().unwrap().push(scene)));
    applied
}

fn scenes(applied: &StdMutex<Vec<Option<String>>>) -> Vec<Option<String>> {
    applied.lock().unwrap().clone()
}

/// The sanctuary holds Projection; the office takes Stream.
async fn stream_at_the_office() -> (Peer, Peer, Arc<StdMutex<Vec<Option<String>>>>) {
    let (sanctuary, office) = paired_and_leased().await;
    let applied = applying_cues(&office);
    office.node.claim_role(Role::Stream).unwrap();
    until(
        Duration::from_secs(5),
        "both to know who runs the stream",
        || holder_of(&sanctuary, Role::Stream) == Some(office.node.device()),
    )
    .await;
    (sanctuary, office, applied)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cue_reaches_the_stream_holder_which_answers_and_projection_does_not_switch() {
    let (sanctuary, office, applied) = stream_at_the_office().await;
    assert!(office.node.obs_here() && !sanctuary.node.obs_here());
    assert_eq!(
        sanctuary.node.route_scene_cue(Some("Worship".into())),
        CueRoute::Sent,
        "Projection hands it on and does not switch its own OBS"
    );
    until(Duration::from_secs(5), "the cue", || {
        scenes(&applied) == [Some("Worship".to_string())]
    })
    .await;
    until(Duration::from_secs(5), "the answer", || {
        !sanctuary.node.scene_cue_waiting()
    })
    .await;
    // The next item has no scene: still a cue, so a hand pick ends.
    assert_eq!(sanctuary.node.route_scene_cue(None), CueRoute::Sent);
    until(Duration::from_secs(5), "the second cue", || {
        scenes(&applied).len() == 2
    })
    .await;
    assert_eq!(scenes(&applied)[1], None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cue_waits_while_the_stream_holder_is_away_and_only_the_latest_is_applied_on_return() {
    let (mut sanctuary, mut office, applied) = stream_at_the_office().await;
    // Both devices dial each other, so both listeners go for the network to
    // be cut between them (the Projection machine still holds its roles).
    office.cut();
    sanctuary.cut();
    until(
        Duration::from_secs(20),
        "the stream desk to be away",
        || sanctuary.node.status().connected() == 0 && office.node.status().connected() == 0,
    )
    .await;
    for scene in ["Worship", "Sermon", "Wide"] {
        assert_eq!(
            sanctuary.node.route_scene_cue(Some(scene.into())),
            CueRoute::Sent,
            "never a switch on the Projection machine"
        );
    }
    assert!(sanctuary.node.scene_cue_waiting());
    assert!(scenes(&applied).is_empty());
    office.restore().await;
    sanctuary.restore().await;
    until(Duration::from_secs(30), "the cue on return", || {
        !scenes(&applied).is_empty()
    })
    .await;
    until(Duration::from_secs(5), "the answer", || {
        !sanctuary.node.scene_cue_waiting()
    })
    .await;
    assert_eq!(scenes(&applied), [Some("Wide".to_string())]);
}

#[tokio::test(flavor = "multi_thread")]
async fn with_nobody_on_stream_projection_keeps_the_duty_and_nothing_is_sent() {
    let (sanctuary, office) = paired_and_leased().await;
    let applied = applying_cues(&office);
    assert_eq!(holder_of(&sanctuary, Role::Stream), None);
    assert!(sanctuary.node.obs_here());
    assert!(
        !office.node.obs_here(),
        "a device with no role has no OBS duty"
    );
    assert_eq!(
        sanctuary.node.route_scene_cue(Some("Worship".into())),
        CueRoute::Local
    );
    // A device with no role has no cue to give.
    assert_eq!(
        office.node.route_scene_cue(Some("Worship".into())),
        CueRoute::Dropped
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(scenes(&applied).is_empty());
    assert!(!sanctuary.node.scene_cue_waiting());
}

// ---- The Audio holder plays: sound cues ------------------------------------------

use lampstand_sync::net::{SoundCommand, SoundNow, SoundRoute};

/// Records the cues a device is told to act on.
fn hearing_sound(p: &Peer) -> Arc<StdMutex<Vec<SoundCommand>>> {
    let heard = Arc::new(StdMutex::new(Vec::new()));
    let h = heard.clone();
    p.node
        .set_sound_hook(Arc::new(move |cmd| h.lock().unwrap().push(cmd)));
    heard
}

/// The sanctuary holds Projection and Operator; the office takes Audio (the
/// sound desk).
async fn audio_at_the_office() -> (Peer, Peer, Arc<StdMutex<Vec<SoundCommand>>>) {
    let (sanctuary, office) = paired_and_leased().await;
    let heard = hearing_sound(&office);
    office.node.ask_role(Role::Audio).unwrap();
    until(Duration::from_secs(5), "the prompt", || {
        role_status(&sanctuary, Role::Audio).request.is_some()
    })
    .await;
    sanctuary
        .node
        .allow_role(Role::Audio, office.node.device(), false)
        .unwrap();
    until(
        Duration::from_secs(5),
        "both to know who runs the sound",
        || {
            holder_of(&sanctuary, Role::Audio) == Some(office.node.device())
                && office.node.audio_here()
        },
    )
    .await;
    (sanctuary, office, heard)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sound_cue_reaches_the_audio_holder_and_what_plays_comes_back() {
    let (sanctuary, office, heard) = audio_at_the_office().await;
    assert!(office.node.audio_here() && !sanctuary.node.audio_here());
    let play = SoundCommand::Play {
        file: [4; 32],
        name: "Walk-in music".into(),
        item: "item-1".into(),
    };
    assert_eq!(
        sanctuary.node.route_sound_cue(play.clone()),
        SoundRoute::Sent
    );
    assert_eq!(
        sanctuary
            .node
            .route_sound_cue(SoundCommand::Stop { fade_ms: 3000 }),
        SoundRoute::Sent
    );
    until(Duration::from_secs(5), "the cues", || {
        heard.lock().unwrap().len() == 2
    })
    .await;
    assert_eq!(
        *heard.lock().unwrap(),
        vec![play, SoundCommand::Stop { fade_ms: 3000 }]
    );
    // The sound desk says what is playing, and Projection hears it.
    let now = SoundNow {
        item: "item-1".into(),
        name: "Walk-in music".into(),
        paused: false,
        ending: false,
        position_ms: 0,
        length_ms: 90_000,
    };
    office.node.report_sound_now(Some(now.clone()));
    until(Duration::from_secs(5), "the state", || {
        sanctuary.node.sound_elsewhere().is_some()
    })
    .await;
    assert_eq!(sanctuary.node.sound_elsewhere().unwrap().1, now);
    office.node.report_sound_now(None);
    until(Duration::from_secs(5), "the state to clear", || {
        sanctuary.node.sound_elsewhere().is_none()
    })
    .await;
    // The desk may panic too, wherever it is asked from: it has no role of
    // its own besides Audio, and a panic from it to Projection is ignored
    // there because Projection does not hold Audio.
    assert_eq!(
        office.node.route_sound_cue(SoundCommand::Panic),
        SoundRoute::Local
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sound_cue_is_not_kept_for_an_audio_holder_that_is_away() {
    let (mut sanctuary, mut office, heard) = audio_at_the_office().await;
    office.cut();
    sanctuary.cut();
    until(Duration::from_secs(20), "the sound desk to be away", || {
        sanctuary.node.status().connected() == 0 && office.node.status().connected() == 0
    })
    .await;
    assert_eq!(
        sanctuary.node.route_sound_cue(SoundCommand::Pause),
        SoundRoute::Away
    );
    assert_eq!(
        sanctuary.node.route_sound_cue(SoundCommand::Panic),
        SoundRoute::Away
    );
    office.restore().await;
    sanctuary.restore().await;
    until(Duration::from_secs(30), "the devices to meet again", || {
        sanctuary.node.status().connected() == 1 && office.node.status().connected() == 1
    })
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        heard.lock().unwrap().is_empty(),
        "a cue from before the break is never played after it"
    );
    // Back together, cues go again.
    assert_eq!(
        sanctuary.node.route_sound_cue(SoundCommand::Pause),
        SoundRoute::Sent
    );
    until(Duration::from_secs(5), "the cue", || {
        heard.lock().unwrap().as_slice() == [SoundCommand::Pause]
    })
    .await;
}
