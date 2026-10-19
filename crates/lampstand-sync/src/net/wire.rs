//! What travels on a sync WebSocket: one postcard-encoded [`Frame`] per
//! binary message.
//!
//! A connection opens with the handshake (`Hello`, then `Proof` each way;
//! see [`super::handshake`]), then carries `Sync` frames for documents and
//! `Blob*` frames for content-addressed files until either side closes.

use std::io;
use std::net::SocketAddr;
use std::pin::Pin;

use futures_util::{Sink, SinkExt, Stream, StreamExt};
use serde::{Deserialize, Serialize};

/// Bumped when frames change incompatibly. A peer speaking another version
/// is refused with a message saying which side should update.
pub(crate) const PROTOCOL: u32 = 1;

/// Blob bytes per `BlobChunk` frame.
pub(crate) const BLOB_CHUNK: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Frame {
    Hello(Hello),
    /// The sender's signature of the handshake transcript, and with a
    /// pairing code, its key confirmation.
    Proof {
        signature: Vec<u8>,
        confirm: Option<[u8; 32]>,
    },
    /// The connection is refused; the reason is shown to the person.
    Refused {
        reason: String,
    },
    /// One Automerge sync message for one document.
    Sync {
        kind: String,
        id: [u8; 16],
        data: Vec<u8>,
    },
    BlobWant {
        hash: [u8; 32],
    },
    BlobChunk {
        hash: [u8; 32],
        data: Vec<u8>,
        last: bool,
    },
    BlobMissing {
        hash: [u8; 32],
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Hello {
    pub protocol: u32,
    pub device: [u8; 32],
    /// The device's display name: "Office PC".
    pub name: String,
    pub nonce: [u8; 32],
    /// A SPAKE2 message, when the sender is pairing with a code.
    pub pair: Option<Vec<u8>>,
}

impl Frame {
    pub(crate) fn encode(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("frames always encode")
    }

    pub(crate) fn decode(bytes: &[u8]) -> io::Result<Frame> {
        postcard::from_bytes(bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("bad frame: {e}")))
    }
}

type BoxSink = Pin<Box<dyn Sink<Vec<u8>, Error = io::Error> + Send>>;
type BoxStream = Pin<Box<dyn Stream<Item = io::Result<Vec<u8>>> + Send>>;

/// One sync connection as binary messages, whichever WebSocket library
/// carries it: tokio-tungstenite for outgoing connections, axum's
/// WebSocket in `lampstand-server` for incoming ones.
pub struct Link {
    tx: BoxSink,
    rx: BoxStream,
    remote: Option<SocketAddr>,
}

impl Link {
    /// A link from any sink and stream of binary messages. The stream ends
    /// when the connection closes.
    pub fn new(
        tx: impl Sink<Vec<u8>, Error = io::Error> + Send + 'static,
        rx: impl Stream<Item = io::Result<Vec<u8>>> + Send + 'static,
        remote: Option<SocketAddr>,
    ) -> Self {
        Self {
            tx: Box::pin(tx),
            rx: Box::pin(rx),
            remote,
        }
    }

    /// A tokio-tungstenite WebSocket as a link. Text, ping and pong
    /// messages are skipped; a close message ends the stream.
    pub fn from_tungstenite<S>(
        ws: tokio_tungstenite::WebSocketStream<S>,
        remote: Option<SocketAddr>,
    ) -> Self
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        use tokio_tungstenite::tungstenite::Message;
        let (tx, rx) = ws.split();
        let tx = tx
            .sink_map_err(io::Error::other)
            .with(
                |bytes: Vec<u8>| async move { Ok::<_, io::Error>(Message::Binary(bytes.into())) },
            );
        let rx = rx
            .take_while(|m| std::future::ready(!matches!(m, Ok(Message::Close(_)))))
            .filter_map(|m| async move {
                match m {
                    Ok(Message::Binary(bytes)) => Some(Ok(bytes.to_vec())),
                    Ok(_) => None,
                    Err(e) => Some(Err(io::Error::other(e))),
                }
            });
        Self::new(tx, rx, remote)
    }

    /// The other end's address, when known.
    pub fn remote(&self) -> Option<SocketAddr> {
        self.remote
    }

    pub(crate) async fn send(&mut self, frame: &Frame) -> io::Result<()> {
        self.tx.send(frame.encode()).await
    }

    /// The next frame, or `None` when the connection closed.
    pub(crate) async fn recv(&mut self) -> io::Result<Option<Frame>> {
        match self.rx.next().await {
            Some(Ok(bytes)) => Frame::decode(&bytes).map(Some),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    pub(crate) fn split(self) -> (BoxSink, BoxStream) {
        (self.tx, self.rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let frames = [
            Frame::Hello(Hello {
                protocol: PROTOCOL,
                device: [7; 32],
                name: "Office PC".into(),
                nonce: [9; 32],
                pair: Some(vec![1, 2, 3]),
            }),
            Frame::Proof {
                signature: vec![5; 64],
                confirm: None,
            },
            Frame::Sync {
                kind: "song".into(),
                id: [1; 16],
                data: vec![0; 100],
            },
            Frame::BlobChunk {
                hash: [3; 32],
                data: vec![4; 10],
                last: true,
            },
        ];
        for frame in frames {
            assert_eq!(Frame::decode(&frame.encode()).unwrap(), frame);
        }
        assert!(Frame::decode(&[0xff, 0xff]).is_err());
    }
}
