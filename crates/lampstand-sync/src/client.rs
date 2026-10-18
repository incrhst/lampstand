//! Dialing a peer or relay.

use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{header, HeaderValue};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::server::SYNC_PATH_PREFIX;
use crate::{session, ChurchCredentials, Hub, SyncError};

pub type ClientStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// The sync endpoint for a church under a base URL such as
/// `ws://192.168.1.20:7878` or `wss://relay.example.org`.
pub fn sync_url(base: &str, church: &str) -> String {
    format!("{}{SYNC_PATH_PREFIX}{church}", base.trim_end_matches('/'))
}

/// Opens an authorised sync WebSocket to a peer or relay.
pub async fn connect(base: &str, creds: &ChurchCredentials) -> Result<ClientStream, SyncError> {
    let mut request = sync_url(base, creds.church()).into_client_request()?;
    let bearer = HeaderValue::from_str(&format!("Bearer {}", creds.token()))
        .map_err(|_| SyncError::Protocol("token is not a valid header value".into()))?;
    request.headers_mut().insert(header::AUTHORIZATION, bearer);
    let (ws, _) = tokio_tungstenite::connect_async(request).await?;
    Ok(ws)
}

/// Connects once and syncs until the connection closes.
pub async fn sync_with(
    hub: Arc<Hub>,
    base: &str,
    creds: &ChurchCredentials,
) -> Result<(), SyncError> {
    let ws = connect(base, creds).await?;
    session::run(hub, ws).await
}

/// Keeps this device synced with a relay: connects, and reconnects with
/// back-off (1 s doubling to 60 s) whenever the connection drops or fails.
/// Abort the handle to stop.
pub fn spawn_relay_sync(hub: Arc<Hub>, base: String, creds: ChurchCredentials) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut delay = Duration::from_secs(1);
        loop {
            match connect(&base, &creds).await {
                Ok(ws) => {
                    delay = Duration::from_secs(1);
                    tracing::info!(relay = %base, "connected to relay");
                    if let Err(e) = session::run(hub.clone(), ws).await {
                        tracing::warn!(relay = %base, error = %e, "relay session ended");
                    }
                }
                Err(e) => tracing::warn!(relay = %base, error = %e, "cannot reach relay"),
            }
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(Duration::from_secs(60));
        }
    })
}
