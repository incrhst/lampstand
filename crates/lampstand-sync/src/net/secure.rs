//! Encryption of every frame after the handshake (T75).
//!
//! Each hello carries a fresh X25519 key, and the transcript `T` that both
//! sides sign covers both of them, so the shared secret is known only to
//! the two devices that signed. Two keys are derived from the secret and
//! `T`, one per direction, and every later frame is sealed with
//! ChaCha20-Poly1305 under its direction's key. The nonce is a counter
//! that sender and receiver keep in step (one WebSocket delivers frames in
//! order), so it is never sent. A frame that is changed, replayed,
//! reordered, dropped or injected fails to open, and the session ends;
//! nothing after a failure is opened. The secret, the derived keys and
//! the cipher state are zeroed when dropped. Why this and not Noise:
//! `docs/decisions/0004-sync-session-encryption.md`.

use std::fmt;
use std::io;

use chacha20poly1305::aead::AeadInOut;
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use zeroize::Zeroizing;

/// Bytes a sealed frame adds to the plain one: the Poly1305 tag.
#[cfg(test)]
pub(crate) const OVERHEAD: usize = 16;

const CLIENT_TO_SERVER: &str = "lampstand-sync 2026-10 session key client to server";
const SERVER_TO_CLIENT: &str = "lampstand-sync 2026-10 session key server to client";

/// One connection's keys, from this side's point of view.
pub(crate) struct SessionKeys {
    send: Zeroizing<[u8; 32]>,
    recv: Zeroizing<[u8; 32]>,
}

impl fmt::Debug for SessionKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionKeys(..)")
    }
}

impl SessionKeys {
    /// Keys from the X25519 secret and the transcript both sides signed.
    /// `client` is true on the side that dialled.
    pub(crate) fn derive(shared: &[u8; 32], transcript: &[u8; 32], client: bool) -> Self {
        let material = Zeroizing::new([shared.as_slice(), transcript.as_slice()].concat());
        let c2s = Zeroizing::new(blake3::derive_key(CLIENT_TO_SERVER, &material));
        let s2c = Zeroizing::new(blake3::derive_key(SERVER_TO_CLIENT, &material));
        let (send, recv) = if client { (c2s, s2c) } else { (s2c, c2s) };
        Self { send, recv }
    }

    pub(crate) fn split(self) -> (Sealer, Opener) {
        (
            Sealer {
                aead: cipher(&self.send),
                counter: 0,
            },
            Opener {
                aead: cipher(&self.recv),
                counter: 0,
                failed: false,
            },
        )
    }
}

/// A cipher keyed straight from `key`, with no unzeroed copy in between.
fn cipher(key: &[u8; 32]) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new_from_slice(key).expect("a 32-byte key")
}

fn nonce(counter: u64) -> Nonce {
    let mut n = [0u8; 12];
    n[..8].copy_from_slice(&counter.to_le_bytes());
    Nonce::from(n)
}

/// Seals outgoing frames in order.
pub(crate) struct Sealer {
    aead: ChaCha20Poly1305,
    counter: u64,
}

impl Sealer {
    pub(crate) fn seal(&mut self, mut frame: Vec<u8>) -> Vec<u8> {
        self.aead
            .encrypt_in_place(&nonce(self.counter), &[], &mut frame)
            .expect("a sync frame is far below ChaCha20-Poly1305's length limit");
        // 2^64 frames is never reached; wrapping would reuse a nonce.
        self.counter = self.counter.checked_add(1).expect("frame counter");
        frame
    }
}

/// Opens incoming frames in order. After one fails, every later one does.
pub(crate) struct Opener {
    aead: ChaCha20Poly1305,
    counter: u64,
    failed: bool,
}

impl Opener {
    pub(crate) fn open(&mut self, mut frame: Vec<u8>) -> io::Result<Vec<u8>> {
        if !self.failed
            && self
                .aead
                .decrypt_in_place(&nonce(self.counter), &[], &mut frame)
                .is_ok()
        {
            self.counter += 1;
            return Ok(frame);
        }
        self.failed = true;
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "a sync frame failed authentication (changed, replayed, reordered or not sent by \
             the paired device)",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> ((Sealer, Opener), (Sealer, Opener)) {
        let shared = [7u8; 32];
        let t = [9u8; 32];
        (
            SessionKeys::derive(&shared, &t, true).split(),
            SessionKeys::derive(&shared, &t, false).split(),
        )
    }

    #[test]
    fn frames_open_in_order_both_ways() {
        let ((mut c_seal, mut c_open), (mut s_seal, mut s_open)) = pair();
        for i in 0..3u8 {
            let sealed = c_seal.seal(vec![i; 10]);
            assert_eq!(sealed.len(), 10 + OVERHEAD);
            assert_eq!(s_open.open(sealed).unwrap(), vec![i; 10]);
            assert_eq!(c_open.open(s_seal.seal(vec![i])).unwrap(), vec![i]);
        }
    }

    #[test]
    fn the_same_frame_seals_differently_each_time() {
        let ((mut seal, _), _) = pair();
        assert_ne!(seal.seal(b"hello".to_vec()), seal.seal(b"hello".to_vec()));
    }

    #[test]
    fn directions_have_their_own_keys() {
        // A frame reflected back at its sender does not open.
        let ((mut c_seal, mut c_open), _) = pair();
        assert!(c_open.open(c_seal.seal(b"echo".to_vec())).is_err());
    }

    #[test]
    fn a_changed_frame_fails_and_poisons_the_opener() {
        let ((mut seal, _), (_, mut open)) = pair();
        let mut bad = seal.seal(b"Amazing grace".to_vec());
        bad[3] ^= 1;
        let good = seal.seal(b"how sweet".to_vec());
        assert!(open.open(bad).is_err());
        assert!(open.open(good).is_err(), "nothing opens after a failure");
    }

    #[test]
    fn replayed_reordered_and_dropped_frames_fail() {
        let ((mut seal, _), (_, mut open)) = pair();
        let a = seal.seal(b"a".to_vec());
        let b = seal.seal(b"b".to_vec());
        let c = seal.seal(b"c".to_vec());

        let (_, (_, mut replay)) = pair();
        replay.open(a.clone()).unwrap();
        assert!(replay.open(a.clone()).is_err(), "replay");

        let (_, (_, mut reorder)) = pair();
        assert!(reorder.open(b.clone()).is_err(), "reorder");

        let (_, (_, mut dropped)) = pair();
        dropped.open(a.clone()).unwrap();
        assert!(dropped.open(c.clone()).is_err(), "drop");

        open.open(a).unwrap();
        open.open(b).unwrap();
        open.open(c).unwrap();
    }

    #[test]
    fn another_sessions_frame_fails() {
        let ((mut seal, _), _) = pair();
        let other = SessionKeys::derive(&[7u8; 32], &[8u8; 32], false);
        let (_, mut open) = other.split();
        assert!(open.open(seal.seal(b"x".to_vec())).is_err());
    }
}
