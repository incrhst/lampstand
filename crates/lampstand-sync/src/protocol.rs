//! Wire format: one binary WebSocket message per frame.
//!
//! ```text
//! Hello: VERSION, 0, peer id len (u8), peer id, doc count (u32 BE),
//!        then per doc: id len (u16 BE), id
//! Sync:  VERSION, 1, doc id len (u16 BE), doc id, Automerge sync message
//! ```
//!
//! Each side sends Hello first, listing the documents it holds, then one
//! Automerge sync conversation runs per document. The payload of a Sync
//! frame is a standard Automerge sync message, so any Automerge
//! implementation (for example the browser planner, CD-5) can join.

use crate::{DocId, SyncError};

pub const VERSION: u8 = 1;

const HELLO: u8 = 0;
const SYNC: u8 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    Hello { peer_id: String, docs: Vec<DocId> },
    Sync { doc: DocId, message: Vec<u8> },
}

impl Frame {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![VERSION];
        match self {
            Frame::Hello { peer_id, docs } => {
                out.push(HELLO);
                let peer = &peer_id.as_bytes()[..peer_id.len().min(u8::MAX as usize)];
                out.push(peer.len() as u8);
                out.extend_from_slice(peer);
                out.extend_from_slice(&(docs.len() as u32).to_be_bytes());
                for doc in docs {
                    put_doc_id(&mut out, doc);
                }
            }
            Frame::Sync { doc, message } => {
                out.push(SYNC);
                put_doc_id(&mut out, doc);
                out.extend_from_slice(message);
            }
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, SyncError> {
        let mut r = Reader(bytes);
        let version = r.u8()?;
        if version != VERSION {
            return Err(SyncError::Protocol(format!(
                "protocol version {version}, this build speaks {VERSION}"
            )));
        }
        match r.u8()? {
            HELLO => {
                let len = r.u8()? as usize;
                let peer_id = String::from_utf8(r.take(len)?.to_vec())
                    .map_err(|_| SyncError::Protocol("peer id is not UTF-8".into()))?;
                let count = u32::from_be_bytes(r.take(4)?.try_into().unwrap());
                // Each id takes at least 3 bytes, which bounds the allocation.
                let mut docs = Vec::with_capacity((count as usize).min(r.0.len() / 3));
                for _ in 0..count {
                    docs.push(r.doc_id()?);
                }
                Ok(Frame::Hello { peer_id, docs })
            }
            SYNC => {
                let doc = r.doc_id()?;
                Ok(Frame::Sync {
                    doc,
                    message: r.0.to_vec(),
                })
            }
            kind => Err(SyncError::Protocol(format!("unknown frame kind {kind}"))),
        }
    }
}

fn put_doc_id(out: &mut Vec<u8>, doc: &DocId) {
    // DocId::MAX_LEN keeps this within u16.
    out.extend_from_slice(&(doc.as_str().len() as u16).to_be_bytes());
    out.extend_from_slice(doc.as_str().as_bytes());
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], SyncError> {
        if self.0.len() < n {
            return Err(SyncError::Protocol("frame is truncated".into()));
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, SyncError> {
        Ok(self.take(1)?[0])
    }

    fn doc_id(&mut self) -> Result<DocId, SyncError> {
        let len = u16::from_be_bytes(self.take(2)?.try_into().unwrap()) as usize;
        let id = std::str::from_utf8(self.take(len)?)
            .map_err(|_| SyncError::Protocol("document id is not UTF-8".into()))?;
        DocId::new(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let frames = [
            Frame::Hello {
                peer_id: "abc123".into(),
                docs: vec![
                    DocId::new("library").unwrap(),
                    DocId::new("services/x").unwrap(),
                ],
            },
            Frame::Hello {
                peer_id: String::new(),
                docs: vec![],
            },
            Frame::Sync {
                doc: DocId::new("library").unwrap(),
                message: vec![0x42, 0, 1, 2],
            },
        ];
        for frame in frames {
            assert_eq!(Frame::decode(&frame.encode()).unwrap(), frame);
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(Frame::decode(&[]).is_err());
        assert!(Frame::decode(&[9, 0]).is_err());
        assert!(Frame::decode(&[VERSION, 7]).is_err());
        assert!(Frame::decode(&[VERSION, HELLO, 5, b'a']).is_err());
        assert!(Frame::decode(&[VERSION, HELLO, 0, 0xff, 0xff, 0xff, 0xff]).is_err());
        let mut bad_id = vec![VERSION, SYNC, 0, 2];
        bad_id.extend_from_slice(b"..");
        assert!(Frame::decode(&bad_id).is_err());
    }
}
