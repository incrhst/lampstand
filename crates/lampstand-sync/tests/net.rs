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
use lampstand_sync::net::{Link, NetError, NodeConfig, SyncNode};
use lampstand_sync::team::DeviceKeypair;
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
    assert!(p.problem.as_deref().unwrap().contains("not paired"), "{p:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cut_off_peer_is_offline_without_a_problem() {
    let (presenter, office) = paired().await;
    until(Duration::from_secs(5), "connected", || {
        office.node.status().connected() == 1
    })
    .await;
    presenter.cut();
    until(Duration::from_secs(5), "disconnected", || {
        office.node.status().connected() == 0
    })
    .await;
    // The device is reachable by address but refusing connections: that
    // is a reason to show, not a refusal.
    tokio::time::sleep(Duration::from_millis(1500)).await;
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
    until(Duration::from_secs(5), "disconnected", || {
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
