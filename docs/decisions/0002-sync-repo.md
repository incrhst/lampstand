# 0002. Sync repo: wrap Automerge ourselves, not samod

- Status: accepted (T16)
- Date: 2026-10-18

## Question

`lampstand-sync` needs a repo: Automerge documents on disk, a sync protocol
between peers, and a blob store beside them. samod (Rust, by one of the
Automerge maintainers) is a ready-made repo that is wire-compatible with
automerge-repo in JavaScript. Should `Repo` be built on samod, or on
`automerge` directly with our own storage and sync-state handling?

## What samod 0.15 offers

- **Wire compatibility.** Real. It speaks the automerge-repo CBOR protocol
  and ships an interop test against the JavaScript server. Its document IDs
  are UUIDs printed as bs58check, the same as automerge-repo.
- **Storage layout.** A key-value store with automerge-repo's keys
  (`<doc>/snapshot/<hash>`, `<doc>/incremental/<hash>`), and a filesystem
  backend that matches `automerge-repo-storage-nodefs`.
- **Maturity.** Pre-1.0 (0.15), edition 2024, with breaking API changes
  between minor versions. Actively maintained. MIT.
- **Shape.** Async throughout (`create`, `find` and connections are
  futures). Runtimes: tokio, gio or a local pool.

## Where it does not fit

1. **Document IDs.** Our document ID is the entity's UUID
   (ARCHITECTURE.md: "The document ID is the entity's UUID"). samod's
   `Repo::create` always picks a random ID; there is no way to create a
   document under an ID we choose. We would need a second ID per entity
   and a mapping kept in sync, which is the kind of index that goes wrong.
2. **Role checks on incoming changes.** Peers must drop changes to
   document kinds the author's role may not write (`Role::can_write`).
   samod applies sync messages inside its document actors and offers only
   an announce policy (which documents to offer a peer). There is no hook
   to inspect a change's actor before it is applied. Filtering at the
   transport would mean rewriting sync messages, which breaks the sync
   state's bookkeeping.
3. **Document kinds.** Our framing carries a `DocKind` with each document
   so that a peer can create an unknown document of the right kind and
   check permissions. automerge-repo messages have no place for it.
4. **Sync API, async runtime.** `Repo` is a small synchronous API used
   from the library facade, the API layer and tests. samod would pull an
   async runtime into every caller, or a runtime hidden inside `Repo`.
5. **Churn.** A pre-1.0 dependency at the centre of our data model, with
   breaking minor releases, is a standing upgrade cost.

What samod buys, compatibility with automerge-repo in the browser, is a
Phase 2 need (the browser planner), and only on the link between the
relay and the browser.

## Decision

Build `Repo` on `automerge` 0.12 and `autosurgeon` 0.14 directly:

- One `automerge::Automerge` per document, held in memory once loaded.
- Storage: `dir/docs/<uuid>/` holds the document's saved chunks. Each
  `put` appends one incremental chunk (`save_after` the last saved
  heads), written atomically. After 32 chunks the document is compacted
  into one snapshot chunk. Loading applies every chunk with
  `load_incremental`, so order and duplicates do not matter.
- Sync: one `automerge::sync::State` per (peer, document), driven by
  `Repo::generate_sync_message` and `Repo::receive_sync_message`. Each
  message carries the document's kind and ID. T17 puts these on the
  WebSocket; tests pass them between in-memory repos.
- The team document's skeleton (`index`, `deleted`) is created by a
  fixed actor at time zero, so every device makes the identical first
  change and the skeletons merge instead of conflicting.
- Actor IDs are the device's 32-byte ed25519 public key plus 8 random
  bytes per `Repo::open`. The key prefix lets a peer find the author's
  role (T18); the random tail means a device that loses its store and
  starts again can never reuse a sequence number.

## Keeping the door open

- Document IDs are UUIDs, so a document's automerge-repo ID is its UUID in
  bs58check; nothing needs renaming.
- The sync messages are plain Automerge sync messages, the same bytes
  automerge-repo sends inside its CBOR envelope.
- If the Phase 2 browser planner wants automerge-repo, the relay can speak
  automerge-repo's protocol to browsers (samod or a small adapter) while
  speaking ours to devices, and do the role checks in between.

Revisit if samod adds create-with-ID and a per-change admission hook, and
reaches 1.0.
