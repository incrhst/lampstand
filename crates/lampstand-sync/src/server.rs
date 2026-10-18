//! Accepting sync connections: used by LAN peers and the relay.

use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::{header, StatusCode};
use tokio_tungstenite::WebSocketStream;

use crate::SyncError;

/// Sync connections are WebSocket upgrades of `GET /sync/<church>`.
pub const SYNC_PATH_PREFIX: &str = "/sync/";

/// Completes the WebSocket handshake if `authorize(church, token)` accepts
/// the church named in the path and the bearer token in the
/// `Authorization` header. Otherwise answers 401 (or 404 for any other path)
/// and returns an error.
pub async fn accept<S, T, A>(stream: S, authorize: A) -> Result<(WebSocketStream<S>, T), SyncError>
where
    S: AsyncRead + AsyncWrite + Unpin,
    A: FnOnce(&str, &str) -> Option<T> + Unpin,
{
    let mut authorized = None;
    // tungstenite's handshake callback fixes the error type, so it cannot be boxed.
    #[allow(clippy::result_large_err)]
    let callback = |request: &Request, response: Response| {
        let Some(church) = request.uri().path().strip_prefix(SYNC_PATH_PREFIX) else {
            return Err(reject(StatusCode::NOT_FOUND));
        };
        let token = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        match authorize(church, token) {
            Some(value) => {
                authorized = Some(value);
                Ok(response)
            }
            None => Err(reject(StatusCode::UNAUTHORIZED)),
        }
    };
    let ws = tokio_tungstenite::accept_hdr_async(stream, callback).await?;
    let value = authorized.ok_or_else(|| SyncError::Protocol("not authorized".into()))?;
    Ok((ws, value))
}

fn reject(status: StatusCode) -> ErrorResponse {
    let mut response = ErrorResponse::new(Some(status.canonical_reason().unwrap_or("").into()));
    *response.status_mut() = status;
    response
}
