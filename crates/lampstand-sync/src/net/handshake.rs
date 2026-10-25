//! The device-key challenge handshake, with optional pairing.
//!
//! ```text
//! client                                   server
//!   Hello { device, name, nonce, pair?, ephemeral } →
//!                                        ← Hello { device, name, nonce, pair?, ephemeral }
//!   Proof { sign(T ‖ "client"), confirm? } →
//!                                        ← Proof { sign(T ‖ "server"), confirm? }
//!   …every later frame sealed with the session keys ([`super::secure`])
//! ```
//!
//! `T` is a hash of both device keys, names and nonces, both SPAKE2
//! messages and both X25519 keys. Each side signs it with its device key, so
//! neither can claim a key it does not hold, a signature cannot be
//! replayed into another connection, and nobody in the middle can swap
//! the X25519 keys. The session keys come from the X25519 secret and `T`.
//! Without a pairing code, each side refuses a peer that is not in its
//! paired list. With one, `pair` carries SPAKE2 messages keyed by the
//! code, and `confirm` is a keyed hash of `T` under the derived key: a
//! match proves both sides typed the same code and saw the same keys, and
//! each then adds the other to its list.
//!
//! Either side may send `Refused { reason }` instead of its next frame.

use spake2::{Ed25519Group, Identity, Password, Spake2};
use x25519_dalek::{EphemeralSecret, PublicKey};
use zeroize::Zeroizing;

use super::secure::SessionKeys;
use super::trust::Trust;
use super::wire::{Frame, Hello, Link, OtherProtocol, PROTOCOL};
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
    h.update(b"lampstand-sync-handshake-2");
    for hello in [client, server] {
        h.update(&hello.device);
        // The name a pairing stores in `paired.json`: nobody in the middle
        // may change it.
        h.update(&(hello.name.len() as u64).to_le_bytes());
        h.update(hello.name.as_bytes());
        h.update(&hello.nonce);
        let pair = hello.pair.as_deref().unwrap_or_default();
        h.update(&(pair.len() as u64).to_le_bytes());
        h.update(pair);
        h.update(&hello.ephemeral);
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

/// A fresh X25519 key for one connection, used once and zeroed.
fn ephemeral() -> (EphemeralSecret, PublicKey) {
    let secret = EphemeralSecret::random();
    let public = PublicKey::from(&secret);
    (secret, public)
}

fn hello(me: &Me, pair: Option<Vec<u8>>, ephemeral: PublicKey) -> Hello {
    Hello {
        protocol: PROTOCOL,
        device: me.key.public().0,
        name: me.name.to_string(),
        nonce: crate::repo::random_bytes::<32>(),
        pair,
        ephemeral: ephemeral.to_bytes(),
    }
}

/// The X25519 secret with the peer's key, or `None` for a key that would
/// make it predictable (a low-order point).
fn agree(mine: EphemeralSecret, theirs: &Hello) -> Option<Zeroizing<[u8; 32]>> {
    let shared = mine.diffie_hellman(&PublicKey::from(theirs.ephemeral));
    shared
        .was_contributory()
        .then(|| Zeroizing::new(shared.to_bytes()))
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
    match link.recv().await {
        Ok(Some(Frame::Refused { reason })) => Err(NetError::Refused(reason)),
        Ok(Some(frame)) => Ok(frame),
        Ok(None) => Err(NetError::Closed),
        Err(e) => {
            // A hello from a version whose frames this one cannot read.
            let other = e.get_ref().and_then(|e| e.downcast_ref::<OtherProtocol>());
            match other.map(|o| version_check(o.0)) {
                Some(Err(why)) => Err(refuse(link, &why).await),
                _ => Err(e.into()),
            }
        }
    }
}

fn version_check(protocol: u32) -> Result<(), String> {
    match protocol.cmp(&PROTOCOL) {
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
/// already be paired. Returns the keys for every later frame.
pub(crate) async fn client(
    link: &mut Link,
    me: &Me<'_>,
    trust: &std::sync::Mutex<Trust>,
    code: Option<&str>,
) -> Result<(Peer, SessionKeys), NetError> {
    let spake = code.map(|code| {
        Spake2::<Ed25519Group>::start_a(
            &Password::new(code.as_bytes()),
            &Identity::new(SPAKE_CLIENT),
            &Identity::new(SPAKE_SERVER),
        )
    });
    let (secret, public) = ephemeral();
    let mine = hello(me, spake.as_ref().map(|(_, msg)| msg.clone()), public);
    link.send(&Frame::Hello(mine.clone())).await?;
    let Frame::Hello(theirs) = expect(link).await? else {
        return Err(refuse(link, "expected a hello").await);
    };
    if let Err(why) = version_check(theirs.protocol) {
        return Err(refuse(link, &why).await);
    }
    let device = DeviceKey(theirs.device);
    if device == me.key.public() {
        return Err(refuse(link, "that address is this device").await);
    }
    let Some(shared) = agree(secret, &theirs) else {
        return Err(refuse(link, "the other device's session key is not valid").await);
    };
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
    let peer = Peer {
        device,
        name: theirs.name,
        paired_now: key.is_some(),
    };
    Ok((peer, SessionKeys::derive(&shared, &t, true)))
}

/// The accepting side. Refuses devices that are not paired, unless they
/// confirm the pairing code this device is showing. `pairing` admits one
/// pairing handshake at a time; others are refused until it finishes.
/// Returns the keys for every later frame.
pub(crate) async fn server(
    link: &mut Link,
    me: &Me<'_>,
    trust: &std::sync::Mutex<Trust>,
    pairing: &tokio::sync::Mutex<()>,
) -> Result<(Peer, SessionKeys), NetError> {
    let Frame::Hello(theirs) = expect(link).await? else {
        return Err(refuse(link, "expected a hello").await);
    };
    if let Err(why) = version_check(theirs.protocol) {
        return Err(refuse(link, &why).await);
    }
    let device = DeviceKey(theirs.device);
    if device == me.key.public() {
        return Err(refuse(link, "that address is this device").await);
    }
    let (secret, public) = ephemeral();
    // Checked before a pairing slot or guess is taken, so a bad key cannot
    // use up the code's five guesses.
    let Some(shared) = agree(secret, &theirs) else {
        return Err(refuse(link, "the other device's session key is not valid").await);
    };
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
    let mine = hello(me, spake.as_ref().map(|(_, msg)| msg.clone()), public);
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
    let peer = Peer {
        device,
        name: theirs.name,
        paired_now: key.is_some(),
    };
    Ok((peer, SessionKeys::derive(&shared, &t, false)))
}

fn lock(trust: &std::sync::Mutex<Trust>) -> std::sync::MutexGuard<'_, Trust> {
    trust.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};

    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

    use super::*;

    type Tap = Arc<Mutex<Vec<Vec<u8>>>>;

    /// A link that sends into `tx`, copying every message into `tap`, and
    /// reads from `rx`.
    fn link(tx: UnboundedSender<Vec<u8>>, rx: UnboundedReceiver<Vec<u8>>, tap: Tap) -> Link {
        let sink = futures_util::sink::unfold((tx, tap), |(tx, tap), bytes: Vec<u8>| async move {
            tap.lock().unwrap().push(bytes.clone());
            tx.send(bytes)
                .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
            Ok::<_, io::Error>((tx, tap))
        });
        let stream = futures_util::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|bytes| (Ok(bytes), rx))
        });
        Link::new(sink, stream, None)
    }

    struct Wire {
        client: Link,
        server: Link,
        /// Everything either side sent, in order.
        tap: Tap,
        /// Messages from here reach the server as if the client sent them.
        to_server: UnboundedSender<Vec<u8>>,
    }

    fn wire() -> Wire {
        let tap = Tap::default();
        let (c_tx, s_rx) = unbounded_channel();
        let (s_tx, c_rx) = unbounded_channel();
        Wire {
            client: link(c_tx.clone(), c_rx, tap.clone()),
            server: link(s_tx, s_rx, tap.clone()),
            tap,
            to_server: c_tx,
        }
    }

    struct Device {
        key: DeviceKeypair,
        name: &'static str,
        trust: std::sync::Mutex<Trust>,
        _dir: tempfile::TempDir,
    }

    fn device(name: &'static str) -> Device {
        let dir = tempfile::tempdir().unwrap();
        Device {
            key: DeviceKeypair::generate(),
            name,
            trust: std::sync::Mutex::new(Trust::load(dir.path().join("paired.json")).unwrap()),
            _dir: dir,
        }
    }

    impl Device {
        fn me(&self) -> Me<'_> {
            Me {
                key: &self.key,
                name: self.name,
            }
        }
    }

    /// Pairs two devices over `wire` and returns both links encrypted.
    async fn paired(wire: Wire) -> (Link, Link, Tap, UnboundedSender<Vec<u8>>) {
        let office = device("Office PC");
        let sanctuary = device("Sanctuary");
        let code = lock(&sanctuary.trust).start_pairing().code;
        let pairing = tokio::sync::Mutex::new(());
        let Wire {
            client: mut c_link,
            server: mut s_link,
            tap,
            to_server,
        } = wire;
        let (c_me, s_me) = (office.me(), sanctuary.me());
        let (c, s) = tokio::join!(
            client(&mut c_link, &c_me, &office.trust, Some(&code)),
            server(&mut s_link, &s_me, &sanctuary.trust, &pairing),
        );
        let ((c_peer, c_keys), (s_peer, s_keys)) = (c.unwrap(), s.unwrap());
        assert!(c_peer.paired_now && s_peer.paired_now);
        assert_eq!(c_peer.device, sanctuary.key.public());
        assert_eq!(s_peer.device, office.key.public());
        (
            c_link.encrypt(c_keys),
            s_link.encrypt(s_keys),
            tap,
            to_server,
        )
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[tokio::test]
    async fn frames_after_the_handshake_are_sealed() {
        let (mut client, mut server, tap, _) = paired(wire()).await;
        let handshake = tap.lock().unwrap().len();
        assert_eq!(handshake, 4, "two hellos and two proofs");
        let lyric = b"Amazing grace, how sweet the sound".to_vec();
        let frame = Frame::Sync {
            kind: "song".into(),
            id: [1; 16],
            data: lyric.clone(),
        };
        client.send(&frame).await.unwrap();
        server.send(&frame).await.unwrap();
        assert_eq!(server.recv().await.unwrap(), Some(frame.clone()));
        assert_eq!(client.recv().await.unwrap(), Some(frame));

        let sent = tap.lock().unwrap();
        // The tap sees the handshake: device names travel in the clear.
        assert!(contains(&sent[0], b"Office PC"));
        for bytes in &sent[handshake..] {
            assert!(!contains(bytes, &lyric[..12]), "lyric visible on the wire");
            assert!(
                !contains(bytes, b"song"),
                "document kind visible on the wire"
            );
        }
    }

    #[tokio::test]
    async fn a_frame_injected_after_the_handshake_ends_the_link() {
        let (mut client, mut server, _, to_server) = paired(wire()).await;
        // A third party on the path writes a well-formed plain frame.
        let forged = Frame::Sync {
            kind: "song".into(),
            id: [6; 16],
            data: b"forged".to_vec(),
        };
        to_server.send(forged.encode()).unwrap();
        client.send(&forged).await.unwrap();
        let e = server.recv().await.unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
        // The genuine frame behind it is not read either.
        assert!(server.recv().await.unwrap().is_none());
    }

    /// Pairs Office PC with Sanctuary while a party in the middle changes
    /// Sanctuary's hello with `change`. Returns the office's result and
    /// whether it ended up trusting Sanctuary.
    async fn pair_with_server_hello_changed(
        change: fn(&mut Hello),
    ) -> (Result<(Peer, SessionKeys), NetError>, bool) {
        let tap = Tap::default();
        let (c_tx, s_rx) = unbounded_channel();
        let (s_tx, mut mitm_rx) = unbounded_channel::<Vec<u8>>();
        let (mitm_tx, c_rx) = unbounded_channel();
        let mut client_link = link(c_tx, c_rx, tap.clone());
        let mut server_link = link(s_tx, s_rx, tap);
        tokio::spawn(async move {
            while let Some(m) = mitm_rx.recv().await {
                let m = match Frame::decode(&m) {
                    Ok(Frame::Hello(mut h)) => {
                        change(&mut h);
                        Frame::Hello(h).encode()
                    }
                    _ => m,
                };
                let _ = mitm_tx.send(m);
            }
        });
        let office = device("Office PC");
        let sanctuary = device("Sanctuary");
        let code = lock(&sanctuary.trust).start_pairing().code;
        let pairing = tokio::sync::Mutex::new(());
        let (c_me, s_me) = (office.me(), sanctuary.me());
        let (c, _) = tokio::join!(
            client(&mut client_link, &c_me, &office.trust, Some(&code)),
            server(&mut server_link, &s_me, &sanctuary.trust, &pairing),
        );
        let trusted = lock(&office.trust).is_paired(sanctuary.key.public());
        (c, trusted)
    }

    fn refused_by_signature(result: &Result<(Peer, SessionKeys), NetError>) -> bool {
        matches!(result, Err(NetError::Refused(why)) if why.contains("did not check out"))
    }

    #[tokio::test]
    async fn a_swapped_session_key_is_refused_by_the_signature() {
        // A party in the middle replaces the server's X25519 key with its
        // own. Its signature covers the original, so the client refuses.
        let (c, trusted) =
            pair_with_server_hello_changed(|h| h.ephemeral = ephemeral().1.to_bytes()).await;
        assert!(refused_by_signature(&c), "{c:?}");
        assert!(!trusted);
    }

    #[tokio::test]
    async fn a_renamed_device_is_refused_by_the_signature() {
        // The name is what pairing stores in `paired.json`.
        let (c, trusted) =
            pair_with_server_hello_changed(|h| h.name = "Pastor's laptop".into()).await;
        assert!(refused_by_signature(&c), "{c:?}");
        assert!(!trusted);
    }

    #[tokio::test]
    async fn a_low_order_key_is_refused_before_a_pairing_guess_is_taken() {
        let sanctuary = device("Sanctuary");
        let code = lock(&sanctuary.trust).start_pairing().code;
        let pairing = tokio::sync::Mutex::new(());
        let attacker = DeviceKeypair::generate();
        // More tries than the code has guesses, each with the right code's
        // SPAKE2 message and an all-zero (low-order) X25519 key.
        for _ in 0..10 {
            let Wire {
                client: mut c_link,
                server: mut s_link,
                ..
            } = wire();
            let (_, msg) = Spake2::<Ed25519Group>::start_a(
                &Password::new(code.as_bytes()),
                &Identity::new(SPAKE_CLIENT),
                &Identity::new(SPAKE_SERVER),
            );
            let hello = Hello {
                protocol: PROTOCOL,
                device: attacker.public().0,
                name: "Attacker".into(),
                nonce: crate::repo::random_bytes::<32>(),
                pair: Some(msg),
                ephemeral: [0; 32],
            };
            c_link.send(&Frame::Hello(hello)).await.unwrap();
            let s_me = sanctuary.me();
            let refused = server(&mut s_link, &s_me, &sanctuary.trust, &pairing).await;
            assert!(
                matches!(refused, Err(NetError::Refused(ref why)) if why.contains("session key is not valid")),
                "{refused:?}"
            );
            assert!(matches!(
                c_link.recv().await.unwrap(),
                Some(Frame::Refused { .. })
            ));
        }
        // No guess was spent: the code is still shown and still pairs.
        assert!(lock(&sanctuary.trust).pairing().is_some());
        let office = device("Office PC");
        let Wire {
            client: mut c_link,
            server: mut s_link,
            ..
        } = wire();
        let (c_me, s_me) = (office.me(), sanctuary.me());
        let (c, s) = tokio::join!(
            client(&mut c_link, &c_me, &office.trust, Some(&code)),
            server(&mut s_link, &s_me, &sanctuary.trust, &pairing),
        );
        assert!(c.unwrap().0.paired_now);
        assert!(s.unwrap().0.paired_now);
    }

    #[tokio::test]
    async fn a_low_order_key_from_the_server_is_refused() {
        let (c, trusted) = pair_with_server_hello_changed(|h| h.ephemeral = [0; 32]).await;
        assert!(
            matches!(c, Err(NetError::Refused(ref why)) if why.contains("session key is not valid")),
            "{c:?}"
        );
        assert!(!trusted);
    }

    /// A device on the protocol before roles (version 4, 0.7.0), or on the
    /// one before the sound problem frame (6, test builds of 0.8.0), sends a
    /// hello this version can read; it is refused with the usual message
    /// and is not paired, whether or not it brings a code.
    #[tokio::test]
    async fn a_device_on_the_protocol_before_roles_is_refused_with_the_usual_message() {
        assert_eq!(PROTOCOL, 7, "the sound problem frame is protocol 7");
        for (old_protocol, with_code) in [(4, false), (4, true), (6, false), (6, true)] {
            let sanctuary = device("Sanctuary");
            let old = DeviceKeypair::generate();
            let code = lock(&sanctuary.trust).start_pairing().code;
            let pairing = tokio::sync::Mutex::new(());
            let Wire {
                client: mut c_link,
                server: mut s_link,
                ..
            } = wire();
            let pair = with_code.then(|| {
                Spake2::<Ed25519Group>::start_a(
                    &Password::new(code.as_bytes()),
                    &Identity::new(SPAKE_CLIENT),
                    &Identity::new(SPAKE_SERVER),
                )
                .1
            });
            let (_, public) = ephemeral();
            let hello = Hello {
                protocol: old_protocol,
                device: old.public().0,
                name: "Old laptop".into(),
                nonce: crate::repo::random_bytes::<32>(),
                pair,
                ephemeral: public.to_bytes(),
            };
            c_link.send(&Frame::Hello(hello)).await.unwrap();
            let s_me = sanctuary.me();
            let refused = server(&mut s_link, &s_me, &sanctuary.trust, &pairing).await;
            assert!(
                matches!(refused, Err(NetError::Refused(ref why))
                    if why == "the other device runs an older Lampstand; update it"),
                "{refused:?}"
            );
            match c_link.recv().await.unwrap() {
                Some(Frame::Refused { reason }) => {
                    assert_eq!(
                        reason,
                        "the other device runs an older Lampstand; update it"
                    )
                }
                other => panic!("expected a refusal, got {other:?}"),
            }
            // Nothing was trusted, and the code was not spent.
            assert!(!lock(&sanctuary.trust).is_paired(old.public()));
            assert!(lock(&sanctuary.trust).pairing().is_some());
        }
    }

    /// And the other way round: a hello from a newer version says to update
    /// this one.
    #[test]
    fn versions_say_which_side_should_update() {
        assert!(version_check(PROTOCOL).is_ok());
        assert_eq!(
            version_check(PROTOCOL - 1).unwrap_err(),
            "the other device runs an older Lampstand; update it"
        );
        assert_eq!(
            version_check(PROTOCOL + 1).unwrap_err(),
            "the other device runs a newer Lampstand; update this one"
        );
    }
}
