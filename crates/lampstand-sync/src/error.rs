use std::io;

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("invalid document id {0:?}")]
    InvalidDocId(String),
    #[error("invalid church id {0:?}: use 1 to 64 lowercase letters, digits or '-'")]
    InvalidChurchId(String),
    #[error("no document {0}")]
    NoSuchDoc(String),
    #[error("storage: {0}")]
    Io(#[from] io::Error),
    #[error("automerge: {0}")]
    Automerge(#[from] automerge::AutomergeError),
    #[error("bad sync message: {0}")]
    Protocol(String),
    #[error("websocket: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("mdns: {0}")]
    Mdns(#[from] mdns_sd::Error),
}
