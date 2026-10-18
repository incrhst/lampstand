//! Self-hostable sync relay (CD-3): one small binary, small enough for a
//! Raspberry Pi or a USD 5 VPS.
//!
//! The relay is a peer like any other, except that it is always online and
//! serves many devices. It keeps a full copy of each church's documents in
//! `<data>/<church>/`, so a planner at home on Tuesday and the projection
//! laptop on Sunday sync through it without ever being online together.
//! Each church on the relay has its own token; a connection must present it
//! to `GET /sync/<church>` and only ever sees that church's documents.
//!
//! The relay speaks plain `ws://`. On the internet, put it behind a TLS
//! reverse proxy (Caddy, nginx, Traefik) and give devices the `wss://` URL.

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use lampstand_sync::{server, session, ChurchCredentials, Hub, SyncError};
use tokio::net::TcpListener;
use tokio::task::JoinSet;

pub struct Config {
    pub listen: SocketAddr,
    pub data_dir: PathBuf,
    pub churches: Vec<ChurchCredentials>,
}

struct Church {
    creds: ChurchCredentials,
    hub: Arc<Hub>,
}

pub struct Relay {
    listener: TcpListener,
    churches: Arc<HashMap<String, Church>>,
}

impl Relay {
    /// Opens every church's store and binds the listener.
    pub async fn bind(config: Config) -> Result<Self, SyncError> {
        let mut churches = HashMap::new();
        for creds in config.churches {
            let hub = Hub::open(config.data_dir.join(creds.church()))?;
            let name = creds.church().to_owned();
            if churches
                .insert(name.clone(), Church { creds, hub })
                .is_some()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("church {name} is listed twice"),
                )
                .into());
            }
        }
        let listener = TcpListener::bind(config.listen).await?;
        Ok(Self {
            listener,
            churches: Arc::new(churches),
        })
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Serves until the task is dropped or aborted, which also ends every
    /// session.
    pub async fn run(self) {
        let mut sessions = JoinSet::new();
        loop {
            while sessions.try_join_next().is_some() {}
            let (stream, from) = match self.listener.accept().await {
                Ok(accepted) => accepted,
                Err(e) => {
                    tracing::warn!(error = %e, "accept failed");
                    continue;
                }
            };
            let churches = self.churches.clone();
            sessions.spawn(async move {
                let authorize = |church: &str, token: &str| {
                    let entry = churches.get(church)?;
                    entry
                        .creds
                        .matches(church, token)
                        .then(|| (church.to_owned(), entry.hub.clone()))
                };
                let (ws, (church, hub)) = match server::accept(stream, authorize).await {
                    Ok(accepted) => accepted,
                    Err(e) => {
                        tracing::info!(%from, error = %e, "refused connection");
                        return;
                    }
                };
                tracing::info!(%from, %church, "peer connected");
                match session::run(hub, ws).await {
                    Ok(()) => tracing::info!(%from, %church, "peer disconnected"),
                    Err(e) => tracing::info!(%from, %church, error = %e, "session ended"),
                }
            });
        }
    }
}

/// Parses church entries, one per line or comma-separated, as `name=token`
/// or `name token`. Blank lines and lines starting with `#` are skipped.
pub fn parse_churches(text: &str) -> Result<Vec<ChurchCredentials>, SyncError> {
    text.split(['\n', ','])
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let (name, token) = line
                .split_once('=')
                .or_else(|| line.split_once(char::is_whitespace))
                .ok_or_else(|| SyncError::InvalidChurchId(line.to_owned()))?;
            let token = token.trim();
            if token.len() < 16 {
                return Err(SyncError::Protocol(format!(
                    "token for church {name} is shorter than 16 characters"
                )));
            }
            ChurchCredentials::new(name.trim(), token)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_church_lists() {
        let text = "# churches\ngrace=0123456789abcdef\n\nhope  fedcba9876543210\n";
        let churches = parse_churches(text).unwrap();
        assert_eq!(churches.len(), 2);
        assert!(churches[1].matches("hope", "fedcba9876543210"));
        let inline = parse_churches("grace=0123456789abcdef,hope=fedcba9876543210").unwrap();
        assert_eq!(inline.len(), 2);
        assert!(parse_churches("grace=short").is_err());
        assert!(parse_churches("Grace=0123456789abcdef").is_err());
        assert!(parse_churches("grace").is_err());
    }
}
