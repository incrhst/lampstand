//! What travels on a sync WebSocket: one postcard-encoded [`Frame`] per
//! binary message.
//!
//! A connection opens with the handshake (`Hello`, then `Proof` each way;
//! see [`super::handshake`]) in the clear, then carries `Sync` frames for
//! documents and `Blob*` frames for content-addressed files until either
//! side closes, each sealed with the session's keys ([`super::secure`]).

use std::io;
use std::net::SocketAddr;
use std::pin::Pin;

use futures_util::{Sink, SinkExt, Stream, StreamExt};
use serde::{Deserialize, Serialize};

use super::secure::SessionKeys;
use crate::roles::Role;

/// Bumped when frames change incompatibly. A peer speaking another version
/// is refused with a message saying which side should update. 2: hellos
/// carry an X25519 key and frames after the handshake are encrypted (T75).
/// 3: presenter lease, ask and heartbeat frames. 4: driving. 5: one record
/// per role (Projection, Operator, Stream, Audio) replaces the lease and
/// the driver seat.
pub(crate) const PROTOCOL: u32 = 5;

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
    /// The sender's record for one role ([`crate::roles::Record`]): sent for
    /// all four when a session opens and whenever one changes. The newer
    /// one wins, whoever sends it.
    Role {
        role: Role,
        holder: Option<[u8; 32]>,
        epoch: u64,
        /// The signed steps that led here, newest first (empty for a state
        /// moved over from 0.7.0). Names are never sent: each device uses
        /// the one it has from pairing.
        #[serde(deserialize_with = "crate::roles::bounded_chain")]
        steps: Vec<crate::roles::Link>,
    },
    /// The sender asks the receiver, the holder of `role`, to hand it over.
    RoleAsk {
        role: Role,
    },
    /// The sender withdraws its ask for `role`.
    RoleCancel {
        role: Role,
    },
    /// The holder turns an ask for `role` down, made at this record epoch.
    RoleAnswer {
        role: Role,
        epoch: u64,
        answer: RoleAnswer,
    },
    /// The sender, the holder of `role`, gives it up on purpose and asks the
    /// receiver to take it (an Operator stopping, for the Projection holder
    /// that runs the service by default).
    RoleYield {
        role: Role,
    },
    /// How long the sender waits before it takes a role from a device that
    /// has gone, in seconds (its takeover setting). Sent when a session opens
    /// and when the setting changes. The receiver, when it is the one that
    /// would claim from the sender, waits at least this long too, so a device
    /// with a shorter setting does not claim before the sender would count the
    /// other as gone. It can only make a device wait longer than its own
    /// setting, never shorter, and what is honoured is capped at the longest
    /// setting there is.
    Takeover {
        secs: u32,
    },
    /// Proof of life, every few seconds, so a silent connection is told
    /// from a quiet one.
    Beat,
    /// A request from the Operator to the Projection holder: the same JSON the
    /// local operator's screen sends.
    DriveCommand {
        payload: Vec<u8>,
    },
    /// The Projection holder's live state for the Operator's screen: one
    /// JSON event.
    DriveLive {
        payload: Vec<u8>,
    },
    /// From the Projection holder to the Stream holder: the OBS scene the
    /// item that just went live asks for (`None`: no scene, but a new item).
    /// The scene name is all it carries. `run` is random per start and `seq`
    /// grows within a run, so an older cue is not applied after a newer one.
    ObsCue {
        run: u64,
        seq: u64,
        scene: Option<String>,
    },
    /// From the Stream holder back to the Projection holder: the cue is
    /// taken, so it need not be sent again.
    ObsCueAck {
        run: u64,
        seq: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum RoleAnswer {
    Declined,
    Busy { by: String },
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
    /// A fresh X25519 public key, for this connection's session keys.
    /// Last, so a version 1 peer still reads the hello (postcard ignores
    /// trailing bytes) and can say which side should update.
    pub ephemeral: [u8; 32],
}

/// A hello this version cannot read, from a peer speaking `0`.
#[derive(Debug)]
pub(crate) struct OtherProtocol(pub u32);

impl std::fmt::Display for OtherProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "a hello in sync protocol {}", self.0)
    }
}

impl std::error::Error for OtherProtocol {}

impl Frame {
    pub(crate) fn encode(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("frames always encode")
    }

    /// Decodes a frame. A hello from another protocol version that does
    /// not decode is an [`OtherProtocol`] error, so the handshake can say
    /// which side should update.
    pub(crate) fn decode(bytes: &[u8]) -> io::Result<Frame> {
        postcard::from_bytes(bytes).map_err(|e| {
            // Every version starts a hello with variant 0, then the version.
            match postcard::from_bytes::<(u32, u32)>(bytes) {
                Ok((0, protocol)) if protocol != PROTOCOL => {
                    io::Error::new(io::ErrorKind::InvalidData, OtherProtocol(protocol))
                }
                _ => io::Error::new(io::ErrorKind::InvalidData, format!("bad frame: {e}")),
            }
        })
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

    /// This link with every later message sealed and opened with `keys`.
    /// The stream ends after the first message that fails to open, with
    /// that error.
    pub(crate) fn encrypt(self, keys: SessionKeys) -> Link {
        let (mut sealer, mut opener) = keys.split();
        let tx = self
            .tx
            .with(move |bytes: Vec<u8>| std::future::ready(Ok::<_, io::Error>(sealer.seal(bytes))));
        let rx = self
            .rx
            .map(move |m| m.and_then(|bytes| opener.open(bytes)))
            .scan(false, |failed, m| {
                let item = (!*failed).then_some(m);
                *failed = !matches!(item, Some(Ok(_)));
                std::future::ready(item)
            });
        Link::new(tx, rx, self.remote)
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
                ephemeral: [4; 32],
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
            Frame::Role {
                role: Role::Operator,
                holder: Some([8; 32]),
                epoch: 3,
                steps: vec![crate::roles::Link {
                    kind: crate::roles::Kind::HandOver,
                    holder: Some(crate::team::DeviceKey([8; 32])),
                    epoch: 3,
                    prev_holder: Some(crate::team::DeviceKey([7; 32])),
                    prev_epoch: 2,
                    sig: vec![5; 64],
                }],
            },
            Frame::Takeover { secs: 120 },
            Frame::RoleAsk { role: Role::Audio },
            Frame::RoleCancel { role: Role::Stream },
            Frame::RoleAnswer {
                role: Role::Operator,
                epoch: 3,
                answer: RoleAnswer::Busy { by: "Sam".into() },
            },
            Frame::RoleYield {
                role: Role::Operator,
            },
            Frame::DriveCommand {
                payload: b"{\"Next\":null}".to_vec(),
            },
            Frame::ObsCue {
                run: 9,
                seq: 2,
                scene: Some("Worship".into()),
            },
            Frame::ObsCue {
                run: 9,
                seq: 3,
                scene: None,
            },
            Frame::ObsCueAck { run: 9, seq: 3 },
        ];
        for frame in frames {
            assert_eq!(Frame::decode(&frame.encode()).unwrap(), frame);
        }
        assert!(Frame::decode(&[0xff, 0xff]).is_err());
    }

    #[test]
    fn a_role_frame_with_too_many_steps_cannot_be_read() {
        let step = crate::roles::Link {
            kind: crate::roles::Kind::HandOver,
            holder: Some(crate::team::DeviceKey([8; 32])),
            epoch: 3,
            prev_holder: Some(crate::team::DeviceKey([7; 32])),
            prev_epoch: 2,
            sig: vec![5; 64],
        };
        let frame = |n: usize| Frame::Role {
            role: Role::Stream,
            holder: Some([8; 32]),
            epoch: 3,
            steps: vec![step.clone(); n],
        };
        assert_eq!(
            Frame::decode(&frame(crate::roles::CHAIN).encode()).unwrap(),
            frame(crate::roles::CHAIN)
        );
        assert!(Frame::decode(&frame(crate::roles::CHAIN + 1).encode()).is_err());
        assert!(Frame::decode(&frame(1000).encode()).is_err());
    }

    /// Version 1's frames, as they were before T75.
    mod v1 {
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Serialize, Deserialize)]
        pub enum Frame {
            Hello(Hello),
        }

        #[derive(Debug, Serialize, Deserialize)]
        pub struct Hello {
            pub protocol: u32,
            pub device: [u8; 32],
            pub name: String,
            pub nonce: [u8; 32],
            pub pair: Option<Vec<u8>>,
        }
    }

    #[test]
    fn hellos_from_other_versions_say_which_version() {
        // A version 1 hello is too short for this version: the error says
        // which version sent it.
        let old = postcard::to_allocvec(&v1::Frame::Hello(v1::Hello {
            protocol: 1,
            device: [7; 32],
            name: "Old laptop".into(),
            nonce: [9; 32],
            pair: None,
        }))
        .unwrap();
        let e = Frame::decode(&old).unwrap_err();
        let other = e.get_ref().and_then(|e| e.downcast_ref::<OtherProtocol>());
        assert_eq!(other.map(|o| o.0), Some(1));

        // A version 1 device still reads this version's hello, so it can
        // refuse it with "the other device runs a newer Lampstand".
        let new = Frame::Hello(Hello {
            protocol: PROTOCOL,
            device: [7; 32],
            name: "New laptop".into(),
            nonce: [9; 32],
            pair: Some(vec![1]),
            ephemeral: [4; 32],
        })
        .encode();
        let v1::Frame::Hello(read) = postcard::from_bytes::<v1::Frame>(&new).unwrap();
        assert_eq!(read.protocol, PROTOCOL);
    }
}
