//! One connection after the handshake: the document sweep, the sync loop,
//! pushed changes and blob transfer. The link it runs on is already
//! encrypted ([`super::secure`]); a frame that fails to open ends it.
//!
//! Repo work runs on Tokio's blocking pool (a received change is written
//! to disk), awaited in order, so the messages for one document go out in
//! the order the repo made them. Frames are written by their own task from
//! an unbounded queue, so a side busy sending its sweep still reads the
//! other side's: two peers sweeping thousands of documents at each other
//! cannot deadlock on full socket buffers. Blob chunks go through a small
//! bounded queue behind sync frames, so a video transfer never delays a
//! lyric fix by more than one chunk.
//!
//! A blob arriving is written to its own temp file in `incoming/`, named
//! for the hash and a random suffix, so two devices sending the same blob
//! at once (each over its own session) never write into one file. It is
//! dropped, and the file deleted, once it grows past the node's cap
//! ([`super::MAX_BLOB`]).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{FutureExt, SinkExt, StreamExt};
use lampstand_core::id::BlobHash;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use super::wire::{Frame, Link, BLOB_CHUNK};
use super::{Control, NetError, SyncNode};
use crate::roles::Record;
use crate::team::DeviceKey;
use crate::{DocKind, SyncMessage};

/// Incoming frames applied in one batch, at most.
const BATCH: usize = 512;

/// How often a session says it is alive.
const BEAT: Duration = crate::roles::HEARTBEAT;

/// A session that has heard nothing for this long is over: the other
/// device is gone even if the socket has not noticed.
const SILENCE: Duration = Duration::from_secs(30);

/// A blob being received: its temp file and how much has arrived.
struct Incoming {
    path: PathBuf,
    file: std::fs::File,
    received: u64,
    /// The node's cap when the transfer started.
    limit: u64,
}

/// Deletes partly received blobs left by a run that stopped mid-transfer.
/// Each transfer has its own file, so nothing else would replace them.
pub(crate) fn clear_incoming(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir.join("incoming"));
}

pub(crate) async fn run(
    node: Arc<SyncNode>,
    link: Link,
    peer: DeviceKey,
    id: u64,
    mut control: mpsc::UnboundedReceiver<Control>,
    mut stop: oneshot::Receiver<()>,
) {
    let (mut sink, mut stream) = link.split();
    let (out, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (blob_out, mut blob_rx) = mpsc::channel::<Vec<u8>>(4);
    let writer = tokio::spawn(async move {
        loop {
            let bytes = tokio::select! {
                biased;
                Some(b) = out_rx.recv() => b,
                Some(b) = blob_rx.recv() => b,
                else => break,
            };
            if sink.send(bytes).await.is_err() {
                // A sink that failed must not be polled again, not even
                // to close it.
                return;
            }
        }
        let _ = sink.close().await;
    });

    let mut s = State {
        node: node.clone(),
        peer,
        out,
        blob_out,
        wants: HashMap::new(),
        incoming: HashMap::new(),
    };
    // The role records go first: how long a connection has been open is
    // what tells a device that was away from one that never left, and the
    // sweep of documents below can take a while on a big library.
    for frame in node.record_frames() {
        s.send(frame);
    }
    s.send(node.takeover_frame());
    node.session_opened(peer);
    // The sweep: offer every document.
    let repo = node.repo.clone();
    let offered = blocking(move || {
        repo.documents()
            .into_iter()
            .filter_map(|(_, id)| repo.generate_sync_message(peer, id))
            .collect::<Vec<_>>()
    })
    .await;
    s.send_all(offered.unwrap_or_default());

    let mut beat = tokio::time::interval(BEAT);
    let mut heard = tokio::time::Instant::now();
    loop {
        tokio::select! {
            _ = &mut stop => break,
            _ = beat.tick() => {
                if heard.elapsed() > SILENCE {
                    eprintln!("lampstand: sync connection silent for {}s: closing", SILENCE.as_secs());
                    break;
                }
                s.send(Frame::Beat);
            }
            c = control.recv() => match c {
                Some(Control::Changed(ids)) => s.offer(ids).await,
                Some(Control::Want(hash, reply)) => s.want(hash, reply),
                Some(Control::Frame(frame)) => s.send(frame),
                None => break,
            },
            frame = stream.next() => {
                let first = match frame {
                    Some(Ok(bytes)) => bytes,
                    Some(Err(e)) => {
                        eprintln!("lampstand: sync connection closed: {e}");
                        break;
                    }
                    None => break,
                };
                node.heard(peer);
                // Take whatever else has already arrived, to apply together.
                // An error (a frame that failed to open) ends the session
                // after the frames before it.
                let mut frames = vec![first];
                let mut end = false;
                while frames.len() < BATCH {
                    match stream.next().now_or_never() {
                        Some(Some(Ok(bytes))) => frames.push(bytes),
                        Some(Some(Err(e))) => {
                            eprintln!("lampstand: sync connection closed: {e}");
                            end = true;
                            break;
                        }
                        Some(None) => {
                            end = true;
                            break;
                        }
                        None => break,
                    }
                }
                let failed = s.handle(frames).await.is_err();
                // Applying a big batch can take a while; that is not silence.
                heard = tokio::time::Instant::now();
                node.heard(peer);
                if failed || end {
                    break;
                }
            }
        }
    }
    writer.abort();
    for (_, f) in s.incoming.drain() {
        let _ = std::fs::remove_file(f.path);
    }
    node.ended(peer, id);
}

struct State {
    node: Arc<SyncNode>,
    peer: DeviceKey,
    out: mpsc::UnboundedSender<Vec<u8>>,
    blob_out: mpsc::Sender<Vec<u8>>,
    wants: HashMap<BlobHash, Vec<oneshot::Sender<Result<(), NetError>>>>,
    incoming: HashMap<BlobHash, Incoming>,
}

impl State {
    fn send(&self, frame: Frame) {
        let _ = self.out.send(frame.encode());
    }

    fn send_all(&self, messages: Vec<SyncMessage>) {
        if !messages.is_empty() {
            self.node.note_out();
        }
        for m in messages {
            self.send(Frame::Sync {
                kind: m.kind.as_str().to_string(),
                id: *m.id.as_bytes(),
                data: m.data,
            });
        }
    }

    /// Pushes documents that changed here.
    async fn offer(&self, ids: Arc<Vec<Uuid>>) {
        let repo = self.node.repo.clone();
        let peer = self.peer;
        let messages = blocking(move || {
            ids.iter()
                .filter_map(|id| repo.generate_sync_message(peer, *id))
                .collect::<Vec<_>>()
        })
        .await;
        self.send_all(messages.unwrap_or_default());
    }

    fn want(&mut self, hash: BlobHash, reply: oneshot::Sender<Result<(), NetError>>) {
        let waiters = self.wants.entry(hash).or_default();
        waiters.push(reply);
        if waiters.len() == 1 {
            self.send(Frame::BlobWant {
                hash: *hash.as_bytes(),
            });
        }
    }

    /// Handles frames in arrival order, applying runs of sync messages as
    /// one batch. An undecodable frame ends the session.
    async fn handle(&mut self, frames: Vec<Vec<u8>>) -> Result<(), NetError> {
        let mut batch = Vec::new();
        for bytes in frames {
            match Frame::decode(&bytes)? {
                Frame::Sync { kind, id, data } => {
                    let Some(kind) = DocKind::parse(&kind) else {
                        continue;
                    };
                    batch.push(SyncMessage {
                        kind,
                        id: Uuid::from_bytes(id),
                        data,
                    });
                }
                other => {
                    self.apply(std::mem::take(&mut batch)).await;
                    self.other(other).await;
                }
            }
        }
        self.apply(batch).await;
        Ok(())
    }

    /// Applies sync messages and sends the replies.
    async fn apply(&self, batch: Vec<SyncMessage>) {
        if batch.is_empty() {
            return;
        }
        self.node.note_in();
        let repo = self.node.repo.clone();
        let peer = self.peer;
        let replies = blocking(move || {
            let mut ids: Vec<Uuid> = batch.iter().map(|m| m.id).collect();
            for (id, result) in repo.receive_sync_messages(peer, batch) {
                if let Err(e) = result {
                    eprintln!("lampstand: sync message for {id} dropped: {e}");
                }
            }
            ids.sort();
            ids.dedup();
            ids.into_iter()
                .filter_map(|id| repo.generate_sync_message(peer, id))
                .collect::<Vec<_>>()
        })
        .await;
        self.send_all(replies.unwrap_or_default());
    }

    async fn other(&mut self, frame: Frame) {
        match frame {
            Frame::BlobWant { hash } => self.serve(BlobHash::from_bytes(hash)),
            Frame::BlobChunk { hash, data, last } => {
                self.chunk(BlobHash::from_bytes(hash), &data, last).await
            }
            Frame::BlobMissing { hash } => {
                let hash = BlobHash::from_bytes(hash);
                if let Some(f) = self.incoming.remove(&hash) {
                    let _ = std::fs::remove_file(f.path);
                }
                for w in self.wants.remove(&hash).unwrap_or_default() {
                    let _ = w.send(Err(NetError::Unreachable(format!(
                        "the other device does not have {hash}"
                    ))));
                }
            }
            Frame::Role {
                role,
                holder,
                epoch,
                steps,
            } => self.node.record_received(
                self.peer,
                role,
                Record {
                    holder: holder.map(DeviceKey),
                    name: String::new(),
                    epoch,
                    chain: steps,
                },
            ),
            Frame::RoleAsk { role } => self.node.role_ask_received(self.peer, role),
            Frame::RoleCancel { role } => self.node.role_cancel_received(self.peer, role),
            Frame::RoleAnswer {
                role,
                epoch,
                answer,
            } => self
                .node
                .role_answer_received(self.peer, role, epoch, answer),
            Frame::RoleYield { role } => self.node.role_yield_received(self.peer, role),
            Frame::DriveCommand { payload } => self.node.drive_command_received(self.peer, payload),
            Frame::DriveLive { payload } => self.node.drive_live_received(self.peer, payload),
            Frame::ObsCue { run, seq, scene } => {
                self.node.scene_cue_received(self.peer, run, seq, scene)
            }
            Frame::ObsCueAck { run, seq } => self.node.scene_cue_ack_received(self.peer, run, seq),
            Frame::SoundCue { cmd } => self.node.sound_cue_received(self.peer, cmd),
            Frame::SoundNow { now } => self.node.sound_now_received(self.peer, now),
            Frame::SoundProblem { problem } => self.node.sound_problem_received(self.peer, problem),
            Frame::Takeover { secs } => self.node.takeover_received(self.peer, secs),
            Frame::Beat => {}
            // Handshake frames after the handshake: ignore.
            Frame::Hello(_) | Frame::Proof { .. } | Frame::Refused { .. } => {}
            Frame::Sync { .. } => unreachable!("batched by handle"),
        }
    }

    /// Streams a blob to the peer from the blocking pool.
    fn serve(&self, hash: BlobHash) {
        let Some(path) = self.node.repo.blobs().path(&hash) else {
            self.send(Frame::BlobMissing {
                hash: *hash.as_bytes(),
            });
            return;
        };
        let out = self.blob_out.clone();
        let missing = self.out.clone();
        tokio::task::spawn_blocking(move || {
            let Ok(mut file) = std::fs::File::open(&path) else {
                let _ = missing.send(
                    Frame::BlobMissing {
                        hash: *hash.as_bytes(),
                    }
                    .encode(),
                );
                return;
            };
            let mut buf = vec![0u8; BLOB_CHUNK];
            let mut pending: Option<Vec<u8>> = None;
            let mut sent = false;
            loop {
                let n = read_full(&mut file, &mut buf).unwrap_or_default();
                // One chunk behind, so the last one can say it is last.
                if let Some(data) = pending.take() {
                    let frame = Frame::BlobChunk {
                        hash: *hash.as_bytes(),
                        data,
                        last: n == 0,
                    };
                    if out.blocking_send(frame.encode()).is_err() {
                        return;
                    }
                    sent = true;
                }
                if n == 0 {
                    if !sent {
                        let empty = Frame::BlobChunk {
                            hash: *hash.as_bytes(),
                            data: Vec::new(),
                            last: true,
                        };
                        let _ = out.blocking_send(empty.encode());
                    }
                    return;
                }
                pending = Some(buf[..n].to_vec());
            }
        });
    }

    async fn chunk(&mut self, hash: BlobHash, data: &[u8], last: bool) {
        if !self.wants.contains_key(&hash) {
            return;
        }
        if !self.incoming.contains_key(&hash) {
            match self.start_incoming(&hash) {
                Ok(f) => {
                    self.incoming.insert(hash, f);
                }
                Err(e) => return self.finish(hash, Err(e.into())),
            }
        }
        let f = self.incoming.get_mut(&hash).expect("just inserted");
        f.received = f.received.saturating_add(data.len() as u64);
        if f.received > f.limit {
            let limit = f.limit;
            eprintln!("lampstand: the file {hash} is larger than {limit} bytes: dropped");
            self.node.blob_too_big(hash, limit);
            // Later chunks of it find no want and are ignored.
            return self.finish(hash, Err(SyncNode::too_big_error(&hash, limit)));
        }
        if let Err(e) = f.file.write_all(data) {
            return self.finish(hash, Err(e.into()));
        }
        if !last {
            return;
        }
        let f = self.incoming.remove(&hash).expect("present");
        drop(f.file);
        let repo = self.node.repo.clone();
        let path = f.path;
        let result = blocking(move || {
            let result = std::fs::File::open(&path)
                .map_err(NetError::from)
                .and_then(|file| {
                    repo.blobs()
                        .put_verified(&hash, file)
                        .map_err(|e| NetError::Io(std::io::Error::other(e.to_string())))
                });
            let _ = std::fs::remove_file(&path);
            result
        })
        .await
        .unwrap_or(Err(NetError::Closed));
        self.finish(hash, result);
    }

    /// A new temp file for one transfer of `hash`, never shared with
    /// another session or an earlier transfer.
    fn start_incoming(&self, hash: &BlobHash) -> std::io::Result<Incoming> {
        let dir = self.node.dir.join("incoming");
        std::fs::create_dir_all(&dir)?;
        let suffix = u64::from_le_bytes(crate::repo::random_bytes::<8>());
        let path = dir.join(format!("{hash}-{suffix:016x}.part"));
        let file = std::fs::File::options()
            .write(true)
            .create_new(true)
            .open(&path)?;
        Ok(Incoming {
            path,
            file,
            received: 0,
            limit: self.node.max_blob(),
        })
    }

    fn finish(&mut self, hash: BlobHash, result: Result<(), NetError>) {
        if let Some(f) = self.incoming.remove(&hash) {
            let _ = std::fs::remove_file(f.path);
        }
        let waiters = self.wants.remove(&hash).unwrap_or_default();
        let ok = result.is_ok();
        let why = result.err().map(|e| e.to_string()).unwrap_or_default();
        for w in waiters {
            let _ = w.send(if ok {
                Ok(())
            } else {
                Err(NetError::Unreachable(why.clone()))
            });
        }
    }
}

/// Runs `f` on the blocking pool. `None` if it panicked.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::task::spawn_blocking(f).await.ok()
}

fn read_full(file: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match file.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::NodeConfig;
    use crate::team::DeviceKeypair;
    use crate::Repo;

    fn node(dir: &Path) -> Arc<SyncNode> {
        let key = DeviceKeypair::generate();
        let repo = Arc::new(
            Repo::open(
                dir.join("repo"),
                DeviceKeypair::from_secret_bytes(key.secret_bytes()),
            )
            .unwrap(),
        );
        SyncNode::new(
            repo,
            key,
            NodeConfig {
                name: "Sanctuary".into(),
                dir: dir.join("sync"),
            },
        )
        .unwrap()
    }

    /// A session with a peer, as `run` makes it, minus the socket.
    fn session(node: &Arc<SyncNode>, peer: u8) -> State {
        let (out, _) = mpsc::unbounded_channel();
        let (blob_out, _) = mpsc::channel(4);
        State {
            node: node.clone(),
            peer: DeviceKey([peer; 32]),
            out,
            blob_out,
            wants: HashMap::new(),
            incoming: HashMap::new(),
        }
    }

    fn want(s: &mut State, hash: BlobHash) -> oneshot::Receiver<Result<(), NetError>> {
        let (tx, rx) = oneshot::channel();
        s.want(hash, tx);
        rx
    }

    fn leftovers(node: &SyncNode) -> Vec<PathBuf> {
        match std::fs::read_dir(node.dir.join("incoming")) {
            Ok(dir) => dir.map(|e| e.unwrap().path()).collect(),
            Err(_) => Vec::new(),
        }
    }

    #[tokio::test]
    async fn a_blob_past_the_cap_is_dropped_and_its_file_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let node = node(dir.path());
        node.set_max_blob(10);
        let mut s = session(&node, 1);
        let bytes = [7u8; 16];
        let hash = BlobHash::of(&bytes);
        let mut rx = want(&mut s, hash);

        s.chunk(hash, &bytes[..8], false).await;
        assert!(rx.try_recv().is_err(), "8 bytes is under the cap");
        assert_eq!(leftovers(&node).len(), 1);
        s.chunk(hash, &bytes[8..], false).await;
        let err = rx.try_recv().unwrap().unwrap_err().to_string();
        assert!(err.contains("10-byte limit"), "{err}");
        assert!(leftovers(&node).is_empty(), "the part file is deleted");
        // The rest of the stream is ignored, and nothing is stored.
        s.chunk(hash, &[], true).await;
        assert!(leftovers(&node).is_empty());
        assert!(node.repo.blobs().path(&hash).is_none());

        // A blob exactly at the cap is taken.
        let small = [3u8; 10];
        let small_hash = BlobHash::of(&small);
        let mut rx = want(&mut s, small_hash);
        s.chunk(small_hash, &small, true).await;
        rx.try_recv().unwrap().unwrap();
        assert!(node.repo.blobs().path(&small_hash).is_some());
    }

    #[tokio::test]
    async fn two_peers_sending_the_same_blob_at_once_do_not_mix_their_writes() {
        let dir = tempfile::tempdir().unwrap();
        let node = node(dir.path());
        let mut a = session(&node, 1);
        let mut b = session(&node, 2);
        let bytes: Vec<u8> = (0..300u32).map(|i| (i % 251) as u8).collect();
        let hash = BlobHash::of(&bytes);
        let parts: Vec<&[u8]> = bytes.chunks(100).collect();
        let mut rx_a = want(&mut a, hash);
        let mut rx_b = want(&mut b, hash);

        // A is two chunks in when B starts; A finishes while B is midway.
        a.chunk(hash, parts[0], false).await;
        a.chunk(hash, parts[1], false).await;
        b.chunk(hash, parts[0], false).await;
        assert_eq!(leftovers(&node).len(), 2, "a file per transfer");
        a.chunk(hash, parts[2], true).await;
        rx_a.try_recv().unwrap().unwrap();
        b.chunk(hash, parts[1], false).await;
        b.chunk(hash, parts[2], true).await;
        rx_b.try_recv().unwrap().unwrap();

        let path = node.repo.blobs().path(&hash).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert!(leftovers(&node).is_empty());
    }

    #[test]
    fn a_new_node_clears_partly_received_blobs() {
        let dir = tempfile::tempdir().unwrap();
        let incoming = dir.path().join("sync").join("incoming");
        std::fs::create_dir_all(&incoming).unwrap();
        std::fs::write(incoming.join("abc-0123.part"), b"half").unwrap();
        let _node = node(dir.path());
        assert!(!incoming.join("abc-0123.part").exists());
    }
}
