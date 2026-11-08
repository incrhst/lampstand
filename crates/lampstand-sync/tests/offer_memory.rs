//! T77: what an offer of a document this device does not have costs in
//! memory while it waits, so that `MAX_UNKNOWN_BYTES` bounds memory and not
//! just message bytes. Its own test binary, with one test, because it counts
//! every allocation in the process.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

use automerge::sync::{self, SyncDoc};
use automerge::transaction::Transactable;
use automerge::{Automerge, AutomergeError, ROOT};
use lampstand_core::song::{Section, Song};
use lampstand_sync::team::DeviceKeypair;
use lampstand_sync::{DocKind, Repo, SyncMessage, UNKNOWN_OFFER_OVERHEAD};

struct Counting;

static LIVE: AtomicIsize = AtomicIsize::new(0);

// SAFETY: forwards to the system allocator, only counting sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size() as isize, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        LIVE.fetch_add(new as isize - layout.size() as isize, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// A song as a peer holds it after a few edits. Built once: autosurgeon in
/// a debug build is slow, and each offer only needs its own last change.
fn song_doc() -> Automerge {
    let mut song = Song::new("Amazing Grace");
    let mut doc = Automerge::new();
    for verse in 0..3 {
        song.sections.push(Section {
            label: format!("V{verse}"),
            lines: vec!["Amazing grace, how sweet the sound".into(); 4],
        });
        doc.transact::<_, _, autosurgeon::ReconcileError>(|tx| autosurgeon::reconcile(tx, &song))
            .unwrap();
    }
    doc
}

/// The opening message a peer sends about its copy of `template` as
/// document `n`: heads and a bloom filter, no changes.
fn offer(template: &Automerge, n: usize) -> SyncMessage {
    let mut doc = template.fork();
    doc.transact::<_, _, AutomergeError>(|tx| {
        tx.put(ROOT, "title", format!("Song {n}")).map(|_| ())
    })
    .unwrap();
    let data = doc
        .generate_sync_message(&mut sync::State::new())
        .unwrap()
        .encode();
    SyncMessage {
        kind: DocKind::Song,
        id: uuid::Uuid::now_v7(),
        data,
    }
}

#[test]
fn a_waiting_offer_costs_no_more_than_it_is_charged() {
    const N: usize = 5_000;
    let dir = tempfile::tempdir().unwrap();
    let repo = Repo::open(dir.path().to_path_buf(), DeviceKeypair::generate()).unwrap();
    let peer = DeviceKeypair::generate().public();
    let template = song_doc();
    let offers: Vec<SyncMessage> = (0..N).map(|n| offer(&template, n)).collect();
    let message_bytes: usize = offers.iter().map(|m| m.data.len()).sum();
    // Count only what the repo keeps: the messages themselves are freed.
    let before = LIVE.load(Ordering::Relaxed);
    let results = repo.receive_sync_messages(peer, offers);
    let after = LIVE.load(Ordering::Relaxed);
    assert!(results.iter().all(|(_, r)| r.is_ok()));
    drop(results);
    // `results` (allocated inside the call) was still live at `after`.
    let results_bytes =
        N * std::mem::size_of::<(uuid::Uuid, Result<(), lampstand_sync::SyncError>)>();
    // The messages were live at `before` and freed inside the call.
    let freed = message_bytes + N * std::mem::size_of::<SyncMessage>();
    let kept = (after - before + freed as isize) as usize - results_bytes;
    let per_offer = kept / N;
    let per_message = message_bytes / N;
    eprintln!(
        "one waiting offer: message {per_message} B, kept {per_offer} B \
         (charged {} B)",
        per_message + UNKNOWN_OFFER_OVERHEAD
    );
    assert!(
        per_offer <= per_message + UNKNOWN_OFFER_OVERHEAD,
        "an offer keeps {per_offer} B but is charged {per_message} + {UNKNOWN_OFFER_OVERHEAD}"
    );
}
