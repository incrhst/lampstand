//! The Lampstand desktop app: Tauri shell, app state and the wiring
//! between the UI, the live engine, the compositor and the render thread.
//!
//! `handler` holds the state and every request handler and has no Tauri
//! in it, so it is tested directly. `shell` is the Tauri glue: the `api`
//! command, the `api-event` stream and the output windows. `sync` runs the
//! local server and peer sync, and answers `Team` requests. `journal`
//! keeps what is on air so a crash comes back on the same slide.

pub mod config;
pub mod decks;
pub mod handler;
pub mod import;
pub mod journal;
pub mod seed;
mod shell;
pub mod sync;

pub use shell::run;
