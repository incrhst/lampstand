# 0004. Sync sessions: encrypt on our handshake, not Noise

- Status: accepted (T75)
- Date: 2026-10-19

## Question

T17's sync connection is plain `ws://`. The handshake proves both device
keys, but after it no frame is protected: anyone on the church network
can read every song and service, and change, drop, replay or inject
frames. Should the session be secured by adding key agreement and an AEAD
to the handshake we have, or by replacing it with Noise (`snow`, XX or IK
with the device keys)?

## What the current handshake already has

- **Mutual authentication.** Each side signs a transcript hash `T` of both
  device keys, both nonces and both SPAKE2 messages with its ed25519
  device key. The signatures are checked against the paired list, or, when
  pairing, against a SPAKE2 key confirmation over the same `T`.
- **Pairing with a six-digit code.** SPAKE2 inside the hello, five
  guesses per code, one pairing handshake at a time, and the code
  re-checked atomically on success.
- **No secret to derive keys from.** `T` is public, and outside pairing
  there is no shared secret, so it cannot key a cipher on its own.

## Why not Noise

1. **Pairing would still need SPAKE2.** Noise's pre-shared-key patterns
   (`XXpsk3` and the like) assume a high-entropy PSK. Keyed with a
   six-digit code, a passive listener could brute-force the code offline
   from one captured handshake. We would keep SPAKE2 and feed its key into
   Noise, so pairing would become two key exchanges in sequence, with the
   guess counting and the atomic re-check threaded through both.
2. **Device keys are ed25519, Noise wants X25519 statics.** XX or IK with
   the device keys means converting ed25519 keys to X25519 (sound, but
   one more thing to get right), or a second static key per device that
   T18's team document would also have to carry.
3. **More change for the same result.** Noise XX is three messages; our
   handshake is two each way and already authenticated. Replacing it
   rewrites `handshake.rs`, its tests and its refusal messages for no
   property we cannot get by adding one field.
4. **`snow` is fine, but not needed.** It is maintained and
   Apache-2.0/MIT. If Lampstand ever needs Noise for interoperability, the
   frame layer below (`secure.rs`) would stay the same.

## Decision

Add an ephemeral X25519 key to each hello and fold both into `T`, then
derive per-direction keys from the X25519 secret and `T`. This is the
SIGMA pattern (sign the transcript, including both Diffie-Hellman
shares): only the two devices whose signatures check out can know the
secret, and swapping a share in the middle breaks a signature.

- **Key agreement.** `x25519-dalek` 2 (BSD-3-Clause; shares
  `curve25519-dalek` 4 with `ed25519-dalek`). A fresh key per connection,
  zeroed when the handshake returns: a stolen device key later does not
  decrypt recorded sessions. A low-order share (non-contributory secret)
  is refused.
- **Key derivation.** BLAKE3 in key-derivation mode (`blake3::derive_key`,
  already a dependency) over `secret ‖ T`, with one context string per
  direction, so the two directions never share a key and a frame
  reflected back at its sender does not open.
- **Frames.** ChaCha20-Poly1305 (`chacha20poly1305` 0.11, RustCrypto,
  Apache-2.0/MIT): fast in software on every machine a church has,
  including ones without AES instructions. Each postcard frame after the
  handshake is sealed in place, adding a 16-byte tag.
- **Nonces.** A 64-bit counter per direction, starting at zero. It is not
  sent: one WebSocket over TCP delivers frames in order, so the receiver
  knows which number comes next. A changed, replayed, reordered, dropped
  or injected frame fails to open; the session ends there, and nothing
  after a failure is opened. The diallers reconnect with a fresh
  handshake and fresh keys, and Automerge sync catches up.
- **Protocol 2.** Hellos changed, so `PROTOCOL` is 2. The new field is
  last, so a version 1 device still reads a version 2 hello and says
  "update this one"; a version 1 hello is recognised by its leading
  variant and version, so this side says "update it".

## What it does not do

- **The handshake is in the clear.** Device keys, device names and the
  SPAKE2 messages travel unencrypted, as they did (names are already in
  mDNS). Hiding them needs an encrypted hello, which Noise XX would give
  the responder only. Not worth the change on a LAN.
- **Traffic analysis.** Frame sizes and timing show that a sync happened
  and roughly how much moved, not what.
- **No rekeying.** 2^64 frames per direction is never reached; a
  connection that lasts a whole Sunday carries tens of thousands.
- **The transport is still `ws://`.** TLS on the relay (T64) is for
  browsers and proxies; device-to-device security does not depend on it.

## Cost

Measured with `crates/lampstand-sync/tests/net.rs` and
`crates/lampstand-server/tests/sync.rs` on the same machine, before and
after (debug build); see ARCHITECTURE.md, "Sync" → "Network", for the
numbers.
