//! Local-first sync: every device holds the full library and services and
//! merges changes over the church network, a relay or a shared folder.
//!
//! PRD: CD-1 to CD-7. This crate covers CD-1 (Automerge document store),
//! CD-2 (peer-to-peer sync on the LAN, found by mDNS) and the client side of
//! CD-3 (sync through a self-hosted relay).
//!
//! The pieces:
//!
//! - [`Store`]: Automerge documents on disk, one file per document, with
//!   change history and merge.
//! - [`Hub`]: a store shared between sync sessions, which tells every session
//!   when a document changes.
//! - [`session::run`]: the Automerge sync protocol over one WebSocket. LAN
//!   peers and the relay speak the same protocol.
//! - [`LanPeer`]: listens on the church network, advertises itself over mDNS
//!   and dials the peers it finds.
//! - [`client`]: dial a peer or relay by URL.
//! - [`server`]: accept and authorise an incoming WebSocket.

pub mod auth;
pub mod client;
mod doc_id;
mod error;
mod hub;
pub mod lan;
pub mod protocol;
pub mod server;
pub mod session;
mod store;

pub use auth::ChurchCredentials;
pub use automerge;
pub use doc_id::DocId;
pub use error::SyncError;
pub use hub::Hub;
pub use lan::LanPeer;
pub use store::{HistoryEntry, Store};
