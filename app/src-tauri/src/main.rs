//! Lampstand desktop app.
//!
//! Starts the local server (which owns the show), opens the operator window on
//! it, and keeps the projector process `lampstand-output` running: if it exits
//! or crashes it is started again within a second, and redraws the live slide
//! from the server's state (NFR Reliability: live slide back within 5 s).
//! See `docs/v0.1.md`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{RunEvent, WebviewUrl, WebviewWindowBuilder};

fn main() {
    tracing_subscriber::fmt().with_target(false).init();

    let port: u16 = std::env::var("LAMPSTAND_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(lampstand_server::DEFAULT_PORT);
    let data_dir = lampstand_server::default_data_dir();
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");

    // Bind before opening any window, so a second copy of Lampstand fails clearly
    // instead of showing an operator page that talks to the first one.
    let listener = match runtime.block_on(tokio::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port)))) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Lampstand could not use port {port}: {e}. Is Lampstand already running?");
            std::process::exit(1);
        }
    };
    let (state, warnings) = match lampstand_server::AppState::open(data_dir.clone()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Lampstand could not open its library in {}: {e}", data_dir.display());
            std::process::exit(1);
        }
    };
    for w in &warnings {
        tracing::warn!("{w}");
    }
    runtime.spawn(async move {
        if let Err(e) = lampstand_server::serve(state, listener).await {
            tracing::error!("server stopped: {e}");
        }
    });
    tracing::info!("server on http://127.0.0.1:{port} (data in {})", data_dir.display());

    let output: Arc<Mutex<Option<Child>>> = Arc::default();
    let stopping = Arc::new(std::sync::atomic::AtomicBool::new(false));
    supervise_output(port, output.clone(), stopping.clone());

    let app = tauri::Builder::default()
        .setup(move |app| {
            let url = format!("http://127.0.0.1:{port}/operator").parse().expect("operator URL");
            WebviewWindowBuilder::new(app, "operator", WebviewUrl::External(url))
                .title("Lampstand")
                .inner_size(1280.0, 800.0)
                .min_inner_size(960.0, 600.0)
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Tauri app");

    app.run(move |_handle, event| {
        if let RunEvent::ExitRequested { .. } | RunEvent::Exit = event {
            stopping.store(true, std::sync::atomic::Ordering::SeqCst);
            if let Some(mut child) = output.lock().unwrap().take() {
                let _ = child.kill();
            }
        }
    });
    drop(runtime);
}

/// Where `lampstand-output` is: `LAMPSTAND_OUTPUT`, else next to this executable
/// (the app bundle's `MacOS` folder, or `target/<profile>` in development).
fn output_binary() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("LAMPSTAND_OUTPUT") {
        return Some(PathBuf::from(p));
    }
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) { "lampstand-output.exe" } else { "lampstand-output" };
    let p = exe.parent()?.join(name);
    p.exists().then_some(p)
}

fn supervise_output(port: u16, slot: Arc<Mutex<Option<Child>>>, stopping: Arc<std::sync::atomic::AtomicBool>) {
    let Some(bin) = output_binary() else {
        tracing::warn!("lampstand-output not found next to the app; no projector output. Build it with `cargo build -p lampstand-output`.");
        return;
    };
    std::thread::spawn(move || loop {
        if stopping.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let mut guard = slot.lock().unwrap();
        let running = match guard.as_mut().map(|c| c.try_wait()) {
            Some(Ok(None)) => true,
            Some(Ok(Some(status))) => {
                tracing::warn!("lampstand-output exited ({status}); restarting");
                false
            }
            Some(Err(_)) | None => false,
        };
        if !running {
            match Command::new(&bin).arg("--server").arg(format!("ws://127.0.0.1:{port}/ws")).spawn() {
                Ok(child) => *guard = Some(child),
                Err(e) => tracing::error!("could not start {}: {e}", bin.display()),
            }
        }
        drop(guard);
        std::thread::sleep(Duration::from_millis(500));
    });
}
