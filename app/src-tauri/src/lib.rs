//! The Lampstand desktop app: Tauri shell, app state and the wiring
//! between the UI, the live engine, the compositor and the render thread.
//!
//! `handler` holds the state and every request handler and has no Tauri
//! in it, so it is tested directly. `shell` is the Tauri glue: the `api`
//! command, the `api-event` stream and the output windows.

pub mod config;
pub mod handler;
pub mod import;
pub mod seed;
mod shell;

pub use shell::run;
