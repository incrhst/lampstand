//! Run the server on its own, for development or a headless machine:
//! `cargo run -p lampstand-server -- [--port 7420] [--data DIR]`.
//! The desktop app embeds the same server.

use std::net::SocketAddr;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();
    let mut port = lampstand_server::DEFAULT_PORT;
    let mut data = lampstand_server::default_data_dir();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--port" => port = args.next().and_then(|p| p.parse().ok()).expect("--port N"),
            "--data" => data = args.next().expect("--data DIR").into(),
            other => panic!("unknown argument {other}"),
        }
    }
    let (state, warnings) = lampstand_server::AppState::open(data.clone()).map_err(std::io::Error::other)?;
    for w in warnings {
        tracing::warn!("{w}");
    }
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Lampstand on http://127.0.0.1:{port}/operator (data in {})", data.display());
    lampstand_server::serve(state, listener).await
}
