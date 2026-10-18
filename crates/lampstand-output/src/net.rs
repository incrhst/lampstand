//! The WebSocket client: `hello` as an output, `state` in, `command` out.
//!
//! If the server goes away the windows keep the last frame and this retries
//! every [`RETRY`]; the first state after reconnecting redraws everything.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use lampstand_show::protocol::{ClientMessage, Command, Role, ServerMessage};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_tungstenite::tungstenite::http::Uri;
use tokio_tungstenite::tungstenite::Message;

use crate::Event;

pub const RETRY: Duration = Duration::from_millis(500);

/// `http://host:port` for the server behind a `ws://host:port/ws` URL.
pub fn http_base(ws_url: &str) -> Result<String, String> {
    let uri: Uri = ws_url.parse().map_err(|e| format!("bad server URL {ws_url}: {e}"))?;
    let scheme = match uri.scheme_str() {
        Some("ws") => "http",
        Some("wss") => "https",
        _ => return Err(format!("server URL must start with ws:// ({ws_url})")),
    };
    let authority = uri.authority().ok_or_else(|| format!("server URL has no host: {ws_url}"))?;
    Ok(format!("{scheme}://{authority}"))
}

/// Connect to `url` and keep connected until `commands` closes or `send`
/// returns false (the window side has gone).
pub async fn run(url: String, name: String, mut commands: UnboundedReceiver<Command>, send: impl Fn(Event) -> bool) {
    loop {
        if let Ok((ws, _)) = tokio_tungstenite::connect_async(url.as_str()).await {
            // Keys pressed while disconnected are stale; don't replay them.
            while commands.try_recv().is_ok() {}
            if !send(Event::Connected) {
                return;
            }
            let (mut tx, mut rx) = ws.split();
            let hello = ClientMessage::Hello { role: Role::Output, name: name.clone() };
            let mut ok = tx.send(Message::text(serde_json::to_string(&hello).expect("JSON"))).await.is_ok();
            while ok {
                tokio::select! {
                    message = rx.next() => match message {
                        Some(Ok(Message::Text(text))) => match serde_json::from_str::<ServerMessage>(text.as_str()) {
                            Ok(ServerMessage::State(snapshot)) => {
                                if !send(Event::State(snapshot)) {
                                    return;
                                }
                            }
                            Ok(ServerMessage::Error { message }) => eprintln!("lampstand-output: server: {message}"),
                            Err(e) => eprintln!("lampstand-output: unreadable message: {e}"),
                        },
                        Some(Ok(Message::Close(_))) | Some(Err(_)) | None => ok = false,
                        Some(Ok(_)) => {}
                    },
                    command = commands.recv() => match command {
                        Some(command) => {
                            let message = ClientMessage::Command { command };
                            ok = tx.send(Message::text(serde_json::to_string(&message).expect("JSON"))).await.is_ok();
                        }
                        None => return,
                    },
                }
            }
            if !send(Event::Disconnected) {
                return;
            }
        }
        if commands.is_closed() {
            return;
        }
        tokio::time::sleep(RETRY).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_base_from_ws_url() {
        assert_eq!(http_base("ws://127.0.0.1:7420/ws").unwrap(), "http://127.0.0.1:7420");
        assert_eq!(http_base("ws://localhost/ws").unwrap(), "http://localhost");
        assert!(http_base("http://x/ws").is_err());
        assert!(http_base("nonsense").is_err());
    }
}
