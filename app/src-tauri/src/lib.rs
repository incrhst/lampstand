//! The Lampstand desktop app: Tauri shell, app state and the wiring
//! between the UI, the live engine, the compositor and the render thread.
//!
//! `handler` holds the state and every request handler and has no Tauri
//! in it, so it is tested directly. `shell` is the Tauri glue: the `api`
//! command, the `api-event` stream and the output windows. `sync` runs the
//! local server and peer sync, and answers `Team` requests. `obs_control`
//! drives OBS over its WebSocket and answers `Obs` requests. `remote` runs
//! Lampstand Remote, and `gate` decides what it and a driving device may
//! do. `keys` holds the clicker's global shortcuts. `journal` keeps what is
//! on air so a crash comes back on the same slide.

pub mod audio;
pub mod awake;
pub mod config;
pub mod decks;
pub mod display_names;
pub mod gate;
pub mod handler;
pub mod help;
pub mod import;
pub mod journal;
pub mod keys;
pub mod obs_control;
pub mod remote;
pub mod seed;
mod shell;
pub mod stream_cues;
pub mod sync;
pub mod theme_preview;
pub mod training;
pub mod training_videos;
pub mod update;

pub use shell::run;
