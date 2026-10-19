//! The device-key challenge handshake, with optional pairing.
//!
//! ```text
//! client                                   server
//!   Hello { device, name, nonce, pair? } →
//!                                        ← Hello { device, name, nonce, pair? }
//!   Proof { sign(T ‖ "client"), confirm? } →
//!                                        ← Proof { sign(T ‖ "server"), confirm? }
//! ```
//!
//! `T` is a hash of both device keys, both nonces and both SPAKE2
//! messages. Each side signs it with its device key, so neither can claim
//! a key it does not hold, and a signature cannot be replayed into another
//! connection. Without a pairing code, each side refuses a peer that is
//! not in its paired list. With one, `pair` carries SPAKE2 messages keyed
//! by the code, and `confirm` is a keyed hash of `T` under the derived key:
//! a match proves both sides typed the same code and saw the same keys,
//! and each then adds the other to its list.
//!
//! Either side may send `Refused { reason }` instead of its next frame.

use spake2::{Ed25519Group, Identity, Password, Spake2};

use super::trust::Trust;
use super::wire::{Frame, Hello, Link, PROTOCOL};
use super::NetError;
use crate::team::{DeviceKey, DeviceKeypair};

/// Who is at the other end, once the handshake succeeds.
#[derive(Debug, Clone)]
pub(crate) struct Peer {
    pub device: DeviceKey,
    pub name: String,
    /// The two devices paired during this handshake.
    pub paired_now: bool,
}

pub(crate) struct Me<'a> {
    pub key: &'a DeviceKeypair,
    pub name: &'a str,
}

const SPAKE_CLIENT: &[u8] = b"lampstand pairing client";
const SPAKE_SERVER: &[u8] = b"lampstand pairing server";

fn transcript(client: &Hello, server: &Hello) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(b"lampstand-sync-handshake-1");
    for hello in [client, server] {
        h.update(&hello.device);
        h.update(&hello.nonce);
        let pair = hello.pair.as_deref().unwrap_or_default();
        h.update(&(pair.len() as u64).to_le_bytes());
        h.update(pair);
    }
    *h.finalize().as_bytes()
}

fn signed(t: &[u8; 32], role: &[u8]) -> Vec<u8> {
    [t.as_slice(), role].concat()
}

fn confirm(key: &[u8], t: &[u8; 32], role: &[u8]) -> [u8; 32] {
    let key: [u8; 32] = blake3::hash(key).into();
    *blake3::keyed_hash(&key, &signed(t, role)).as_bytes()
}

fn hello(me: &Me, pair: Option<Vec<u8>>) -> Hello {
    Hello {
        protocol: PROTOCOL,
        device: me.key.public().0,
        name: me.name.to_string(),
        nonce: crate::repo::random_bytes::<32>(),
        pair,
    }
}

fn verify(device: DeviceKey, t: &[u8; 32], role: &[u8], signature: &[u8]) -> bool {
    <[u8; 64]>::try_from(signature).is_ok_and(|s| device.verify(&signed(t, role), &s))
}

async fn refuse(link: &mut Link, reason: &str) -> NetError {
    let _ = link
        .send(&Frame::Refused {
            reason: reason.to_string(),
        })
        .await;
    NetError::Refused(reason.to_string())
}

async fn expect(link: &mut Link) -> Result<Frame, NetError> {
    match link.recv().await? {
        Some(Frame::Refused { reason }) => Err(NetError::Refused(reason)),
        Some(frame) => Ok(frame),
        None => Err(NetError::Closed),
    }
}

fn version_check(hello: &Hello) -> Result<(), String> {
    match hello.protocol.cmp(&PROTOCOL) {
        std::cmp::Ordering::Equal => Ok(()),
        std::cmp::Ordering::Less => {
            Err("the other device runs an older Lampstand; update it".into())
        }
        std::cmp::Ordering::Greater => {
            Err("the other device runs a newer Lampstand; update this one".into())
        }
    }
}

/// The connecting side. With `code`, pairs; otherwise the server must
/// already be paired.
pub(crate) async fn client(
    link: &mut Link,
    me: &Me<'_>,
    trust: &std::sync::Mutex<Trust>,
    code: Option<&str>,
) -> Result<Peer, NetError> {
    let spake = code.map(|code| {
        Spake2::<Ed25519Group>::start_a(
            &Password::new(code.as_bytes()),
            &Identity::new(SPAKE_CLIENT),
            &Identity::new(SPAKE_SERVER),
        )
    });
    let mine = hello(me, spake.as_ref().map(|(_, msg)| msg.clone()));
    link.send(&Frame::Hello(mine.clone())).await?;
    let Frame::Hello(theirs) = expect(link).await? else {
        return Err(refuse(link, "expected a hello").await);
    };
    if let Err(why) = version_check(&theirs) {
        return Err(refuse(link, &why).await);
    }
    let device = DeviceKey(theirs.device);
    if device == me.key.public() {
        return Err(refuse(link, "that address is this device").await);
    }
    let t = transcript(&mine, &theirs);
    let key = match (spake, &theirs.pair) {
        (Some((state, _)), Some(msg)) => match state.finish(msg) {
            Ok(key) => Some(key),
            Err(_) => return Err(refuse(link, "the pairing message was malformed").await),
        },
        (Some(_), None) => return Err(refuse(link, "the other device is not pairing").await),
        (None, _) => {
            if !lock(trust).is_paired(device) {
                return Err(refuse(link, "this device is not paired with that one").await);
            }
            None
        }
    };
    link.send(&Frame::Proof {
        signature: me.key.sign(&signed(&t, b"client")).to_vec(),
        confirm: key.as_ref().map(|k| confirm(k, &t, b"client")),
    })
    .await?;
    let Frame::Proof {
        signature,
        confirm: theirs_confirm,
    } = expect(link).await?
    else {
        return Err(refuse(link, "expected a proof").await);
    };
    if !verify(device, &t, b"server", &signature) {
        return Err(refuse(link, "the other device's key did not check out").await);
    }
    if let Some(k) = &key {
        if theirs_confirm != Some(confirm(k, &t, b"server")) {
            return Err(refuse(link, "the pairing code did not match").await);
        }
        let address = link.remote().map(|a| a.to_string());
        lock(trust).add(device, &theirs.name, address)?;
    }
    Ok(Peer {
        device,
        name: theirs.name,
        paired_now: key.is_some(),
    })
}

/// The accepting side. Refuses devices that are not paired, unless they
/// confirm the pairing code this device is showing. `pairing` admits one
/// pairing handshake at a time; others are refused until it finishes.
pub(crate) async fn server(
    link: &mut Link,
    me: &Me<'_>,
    trust: &std::sync::Mutex<Trust>,
    pairing: &tokio::sync::Mutex<()>,
) -> Result<Peer, NetError> {
    let Frame::Hello(theirs) = expect(link).await? else {
        return Err(refuse(link, "expected a hello").await);
    };
    if let Err(why) = version_check(&theirs) {
        return Err(refuse(link, &why).await);
    }
    let device = DeviceKey(theirs.device);
    if device == me.key.public() {
        return Err(refuse(link, "that address is this device").await);
    }
    let mut _pairing_slot = None;
    let mut guess = None;
    let spake = match &theirs.pair {
        Some(_) => {
            let Ok(slot) = pairing.try_lock() else {
                return Err(refuse(link, "another device is pairing; try again").await);
            };
            _pairing_slot = Some(slot);
            let begun = lock(trust).begin_guess();
            match begun {
                Some(g) => {
                    let spake = Spake2::<Ed25519Group>::start_b(
                        &Password::new(g.code.code.as_bytes()),
                        &Identity::new(SPAKE_CLIENT),
                        &Identity::new(SPAKE_SERVER),
                    );
                    guess = Some(g);
                    Some(spake)
                }
                None => {
                    return Err(
                        refuse(link, "the other device is not showing a pairing code").await,
                    )
                }
            }
        }
        None => {
            if !lock(trust).is_paired(device) {
                return Err(refuse(link, "this device is not paired with that one").await);
            }
            None
        }
    };
    let mine = hello(me, spake.as_ref().map(|(_, msg)| msg.clone()));
    link.send(&Frame::Hello(mine.clone())).await?;
    let t = transcript(&theirs, &mine);
    let Frame::Proof {
        signature,
        confirm: theirs_confirm,
    } = expect(link).await?
    else {
        return Err(refuse(link, "expected a proof").await);
    };
    if !verify(device, &t, b"client", &signature) {
        return Err(refuse(link, "the other device's key did not check out").await);
    }
    let key = match (spake, &theirs.pair, &guess) {
        (Some((state, _)), Some(msg), Some(guess)) => {
            let key = state.finish(msg).ok();
            let good = key
                .as_ref()
                .is_some_and(|k| theirs_confirm == Some(confirm(k, &t, b"client")));
            if !good {
                lock(trust).pairing_failed(guess);
                return Err(refuse(link, "the pairing code did not match").await);
            }
            // Re-checked under the lock: the code may have been voided,
            // replaced or used while this handshake ran.
            if !lock(trust).pairing_succeeded(guess) {
                return Err(refuse(link, "the pairing code is no longer valid").await);
            }
            key
        }
        _ => None,
    };
    link.send(&Frame::Proof {
        signature: me.key.sign(&signed(&t, b"server")).to_vec(),
        confirm: key.as_ref().map(|k| confirm(k, &t, b"server")),
    })
    .await?;
    if key.is_some() {
        lock(trust).add(device, &theirs.name, None)?;
    }
    Ok(Peer {
        device,
        name: theirs.name,
        paired_now: key.is_some(),
    })
}

fn lock(trust: &std::sync::Mutex<Trust>) -> std::sync::MutexGuard<'_, Trust> {
    trust.lock().unwrap_or_else(|e| e.into_inner())
}
