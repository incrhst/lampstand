//! One sync conversation with one peer over one WebSocket.
//!
//! Both ends run the same loop, whether the other end is a laptop on the
//! church network or a relay: send Hello, then keep one Automerge
//! [`sync::State`] per document and send the next sync message whenever a
//! document changes here or a message arrives from the peer. The session ends
//! when the socket closes.

use std::collections::HashMap;
use std::sync::Arc;

use automerge::sync;
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use tokio::sync::broadcast::error::RecvError;
use tokio_tungstenite::tungstenite::{self, Message};

use crate::protocol::Frame;
use crate::{DocId, Hub, SyncError};

/// Runs a sync session until the peer disconnects.
pub async fn run<W>(hub: Arc<Hub>, ws: W) -> Result<(), SyncError>
where
    W: Stream<Item = Result<Message, tungstenite::Error>>
        + Sink<Message, Error = tungstenite::Error>
        + Unpin,
{
    let (mut tx, mut rx) = ws.split();
    // Subscribe before reading the doc list so no change slips between.
    let mut changes = hub.subscribe();
    let mut states: HashMap<DocId, sync::State> = HashMap::new();

    let docs = hub.store().doc_ids();
    let hello = Frame::Hello {
        peer_id: hub.peer_id().to_owned(),
        docs: docs.clone(),
    };
    tx.send(Message::binary(hello.encode())).await?;
    for doc in docs {
        flush(&hub, &mut states, &doc, &mut tx).await?;
    }

    loop {
        tokio::select! {
            incoming = rx.next() => {
                let bytes = match incoming {
                    None | Some(Ok(Message::Close(_))) => break,
                    Some(Ok(Message::Binary(bytes))) => bytes,
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => return Err(e.into()),
                };
                match Frame::decode(&bytes)? {
                    Frame::Hello { peer_id, docs } => {
                        tracing::debug!(peer = %peer_id, docs = docs.len(), "hello");
                        for doc in docs {
                            if !states.contains_key(&doc) {
                                flush(&hub, &mut states, &doc, &mut tx).await?;
                            }
                        }
                    }
                    Frame::Sync { doc, message } => {
                        let message = sync::Message::decode(&message)
                            .map_err(|e| SyncError::Protocol(e.to_string()))?;
                        let state = states.entry(doc.clone()).or_default();
                        hub.receive_sync_message(&doc, state, message)?;
                        flush(&hub, &mut states, &doc, &mut tx).await?;
                    }
                }
            }
            changed = changes.recv() => match changed {
                Ok(doc) => flush(&hub, &mut states, &doc, &mut tx).await?,
                Err(RecvError::Lagged(_)) => {
                    let docs = hub.store().doc_ids();
                    for doc in docs {
                        flush(&hub, &mut states, &doc, &mut tx).await?;
                    }
                }
                Err(RecvError::Closed) => break,
            },
        }
    }
    Ok(())
}

/// Sends the next sync message for `doc`, if the protocol has one to send.
async fn flush<T>(
    hub: &Hub,
    states: &mut HashMap<DocId, sync::State>,
    doc: &DocId,
    tx: &mut T,
) -> Result<(), SyncError>
where
    T: Sink<Message, Error = tungstenite::Error> + Unpin,
{
    let state = states.entry(doc.clone()).or_default();
    if let Some(message) = hub.generate_sync_message(doc, state) {
        let frame = Frame::Sync {
            doc: doc.clone(),
            message: message.encode(),
        };
        tx.send(Message::binary(frame.encode())).await?;
    }
    Ok(())
}
