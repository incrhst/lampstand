//! Self-hostable sync relay: one binary, small enough for a Raspberry Pi.
//! Will also serve the browser planner (CD-5).
//!
//! PRD: CD-3, CD-5.

use std::path::PathBuf;
use std::process::ExitCode;

use lampstand_relay::{parse_churches, Config, Relay};
use lampstand_sync::auth::{generate_token, validate_church_id};

const USAGE: &str = "\
lampstand-relay: sync relay for Lampstand

USAGE:
    lampstand-relay [--listen ADDR] [--data DIR] [--churches-file FILE]
    lampstand-relay gen-token CHURCH

OPTIONS (each also read from the environment):
    --listen ADDR          LAMPSTAND_RELAY_LISTEN          default 0.0.0.0:7878
    --data DIR             LAMPSTAND_RELAY_DATA            default ./lampstand-relay-data
    --churches-file FILE   LAMPSTAND_RELAY_CHURCHES_FILE   lines of `church=token`
                           LAMPSTAND_RELAY_CHURCHES        `church=token,church2=token2`

Church ids are 1-64 lowercase letters, digits or '-'. Tokens are at least 16
characters; `gen-token` makes a random one. Devices connect to
ws://HOST:PORT (or wss:// through a TLS reverse proxy) with the same church
id and token. Set LAMPSTAND_RELAY_LOG=debug for more detail.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("gen-token") {
        return gen_token(args.get(1));
    }
    let config = match parse_config(&args) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    let level = match std::env::var("LAMPSTAND_RELAY_LOG").as_deref() {
        Ok("debug") => tracing::Level::DEBUG,
        Ok("warn") => tracing::Level::WARN,
        _ => tracing::Level::INFO,
    };
    tracing_subscriber::fmt().with_max_level(level).init();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    runtime.block_on(async {
        let churches = config.churches.len();
        let relay = match Relay::bind(config).await {
            Ok(relay) => relay,
            Err(e) => {
                tracing::error!(error = %e, "cannot start");
                return ExitCode::FAILURE;
            }
        };
        if let Ok(addr) = relay.local_addr() {
            tracing::info!(%addr, churches, "lampstand-relay {} listening", env!("CARGO_PKG_VERSION"));
        }
        tokio::select! {
            () = relay.run() => {}
            () = shutdown_signal() => tracing::info!("shutting down"),
        }
        ExitCode::SUCCESS
    })
}

fn parse_config(args: &[String]) -> Result<Config, String> {
    let mut listen =
        std::env::var("LAMPSTAND_RELAY_LISTEN").unwrap_or_else(|_| "0.0.0.0:7878".into());
    let mut data =
        std::env::var("LAMPSTAND_RELAY_DATA").unwrap_or_else(|_| "lampstand-relay-data".into());
    let mut churches_file = std::env::var("LAMPSTAND_RELAY_CHURCHES_FILE").ok();

    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().cloned().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--listen" => listen = value()?,
            "--data" => data = value()?,
            "--churches-file" => churches_file = Some(value()?),
            "-h" | "--help" => return Err("".into()),
            other => return Err(format!("unknown argument {other}")),
        }
    }

    let mut text = std::env::var("LAMPSTAND_RELAY_CHURCHES").unwrap_or_default();
    if let Some(path) = churches_file {
        let file =
            std::fs::read_to_string(&path).map_err(|e| format!("cannot read {path}: {e}"))?;
        text.push('\n');
        text.push_str(&file);
    }
    let churches = parse_churches(&text).map_err(|e| e.to_string())?;
    if churches.is_empty() {
        return Err("no churches configured".into());
    }
    Ok(Config {
        listen: listen
            .parse()
            .map_err(|e| format!("bad --listen {listen}: {e}"))?,
        data_dir: PathBuf::from(data),
        churches,
    })
}

fn gen_token(church: Option<&String>) -> ExitCode {
    let Some(church) = church else {
        eprintln!("usage: lampstand-relay gen-token CHURCH");
        return ExitCode::from(2);
    };
    if let Err(e) = validate_church_id(church) {
        eprintln!("{e}");
        return ExitCode::from(2);
    }
    println!("{church}={}", generate_token());
    ExitCode::SUCCESS
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = term.recv() => {}
                _ = tokio::signal::ctrl_c() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
