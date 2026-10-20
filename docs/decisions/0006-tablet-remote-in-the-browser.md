# 0006. Tablet remote: a browser page over plain HTTP, paired by QR code and PIN

- Status: accepted (T95 to T98)
- Date: 2026-10-20

## Question

LV-5 asks for a phone and tablet remote over local Wi-Fi, and the PRD's
security section says remote control needs a pairing code. The local server
on port 7340 already serves the OBS browser source, so a remote can be one
more page on it, with nothing to install on the tablet.

That page has to be served over something. Plain `http://` on the LAN is
the easy option, but browsers don't treat it as a secure context: no Screen
Wake Lock, no service workers, no `crypto.subtle`, and the PIN travels in
the clear. Should the remote ship on plain HTTP, or wait for HTTPS?

## Decision

Ship the remote on plain HTTP, limit what it can do so that's acceptable,
and leave room to add HTTPS later through a domain Lampstand runs (below).

1. **Open by QR code.** Present shows a QR code for
   `http://<lan-ip>:<port>/remote`. Use the raw IP, not `<name>.local`,
   because Android Chrome often can't resolve mDNS names. Use the port the
   server actually got, which isn't always 7340. Show the code fresh each
   time, so a new DHCP address never leaves a stale printed one.
2. **Pair with a fresh PIN each time.** Show a new six-digit PIN next to
   the QR code every time someone pairs a tablet. There is no standing PIN
   that could leak over the months, and pairing needs someone at the
   laptop, which is where the operator is anyway. The PIN expires
   after five minutes, and five wrong tries void it, as with device pairing
   ([0004](0004-sync-session-encryption.md)). A correct PIN gets a random
   session token in an `HttpOnly`, `SameSite=Strict` cookie, so the tablet
   doesn't have to pair again.
3. **Paired tablets are device-local and revocable.** Store sessions beside
   `paired.json`, never in a synced document. List them under Setup →
   Devices, with Revoke.
4. **Start with an operator remote, scoped to driving the service.**
   `WS /remote/ws` accepts only `Request::Live` (the live engine's
   `Command`: next, previous, show a slide, black, logo, clear) and pushes
   `Event::Live` and `Event::Loaded` out. A musician's stage view and a
   congregation follow-along view (the rest of LV-5) come later as further
   pages, and only ever get less than this, never more. A session can't
   edit or delete anything in the library, read team or device settings,
   or change outputs. Pairing a remote never makes the tablet a sync peer.
5. **Keep the screen awake without the Wake Lock API.** Play a muted,
   looping inline video while the remote is open (the NoSleep technique),
   and tell users they can change the tablet's sleep setting instead.
6. **Explain blocked networks on the laptop, not the tablet.** If guest
   Wi-Fi isolates clients, the tablet scans the code and the page never
   loads. Show a short "not loading?" note next to the QR code: same
   network, client isolation, firewall prompt.
7. **Recommend a separate AV network.** Setup, the "not loading?" note and
   the docs encourage the AV team to put the laptop and its tablets on
   their own Wi-Fi or VLAN, apart from the church's general and guest
   Wi-Fi. That leaves only the AV team in a position to capture a PIN or
   session. It also avoids guest client isolation, and keeps congregation
   phones from competing with the remote for airtime.

## Why this is acceptable on plain HTTP

Someone on the same Wi-Fi can capture the PIN or a session cookie. The
most they can then do is advance, blank or clear slides until an operator
revokes that tablet. Today the same person could walk up to the laptop.
On a separate AV network (item 7), "the same Wi-Fi" means the AV team,
not everyone in the building. Lampstand can recommend that network but
can't require it, so the narrow scope has to hold either way.

This holds only while sessions stay this narrow. Anything with more power
(editing songs, planning, team admin) must not use this pairing over HTTP.
For a remote with more power, use another Lampstand machine (below).

## Remote control from another Lampstand machine

The other way to run a service remotely is a second machine running
Lampstand, paired as a sync peer: a second operator in the booth, or a
backup laptop. Its link is already encrypted and authenticated (T75), and
paired with SPAKE2 so the code is never sent, so none of the plain-HTTP
limits above apply. It needs an install, so it doesn't replace the tablet
remote. The two suit different people.

Today that link carries documents, not control. Edits reach the presenting
machine in about 50 ms, but live state stays inside each machine's live
engine, so a second machine can change the service but not run it. To
close that gap:

1. **One presenting machine.** Exactly one machine owns the live engine and
   the outputs. Another machine controls it, and never runs a second live
   engine against the same screens.
2. **Two new sync frames.** One carries a live `Command` from the
   controlling machine to the presenting one. The other carries the live
   state (the same content as `Event::Live`) back to every connected peer
   on each change. Old peers can't read the new frames, so this is sync
   protocol 3.
3. **Present follows the presenting machine.** On the controlling machine,
   Present shows the presenting machine's live state instead of its own
   engine, and sends its commands there. It draws slides from the synced
   service, songs and decks, so no images cross the link.
4. **One gate for every remote.** On the presenting machine, a command
   from a browser session (T95) and one from a sync peer go through the
   same check before reaching the live engine. Until T18, being paired is
   enough for a peer. After T18, only an operator or admin may send live
   commands. Then the tablet and the second machine can't drift apart in
   what they may do. A peer can do more than a tablet (it can edit
   documents it may write), but no more over this gate.
5. **Losing the link never stops the service.** If the controlling machine
   disconnects, the presenting machine carries on where it is, and its own
   operator can take over at once.

A backup machine already has every song and deck synced, so handing the
outputs over to it if the presenting laptop fails is a natural next step.
It's not part of this decision.

## Why not HTTPS now

- **A self-signed certificate.** Every tablet shows a full-page warning,
  and clicking through can happen more than once. It's not certain that
  every browser treats a clicked-through page as a secure context, so this
  may not even bring back wake lock and service workers.
- **A Lampstand CA installed on each tablet.** On iOS that means installing
  a profile, then turning on full trust in a buried setting. Volunteers
  won't do it, and a leaked CA key would compromise every tablet that
  trusts it.
- **Chrome's insecure-origin flag.** Only on Android, set on each tablet,
  and hidden in `chrome://flags`.
- **Encryption inside the page.** `crypto.subtle` isn't available over HTTP.
  The page's own script arrives over HTTP, so an active attacker can swap it.

## The HTTPS upgrade, and what it would cost

Plex does this with `plex.direct`. Lampstand would own a domain (say
`lampstand.direct`) and run a DNS server that answers
`192-168-1-23.<device-id>.lampstand.direct` with `192.168.1.23`. Each
laptop gets its own wildcard certificate for `*.<device-id>.lampstand.direct`
from Let's Encrypt, through a DNS-01 challenge that a Lampstand API
answers. The private key stays on the laptop. It must be per device,
because a key shared across installs would be revoked. The QR code then
holds the `https://` name. The tablet sees a valid certificate, and the
page is a secure context.

The costs, and why it isn't the first version:

- **A service to run indefinitely:** a DNS server and a certificate API.
  That means an ongoing bill and a dependency for a free, offline-first
  project, and an outage stops new certificates and renewals.
- **Internet to get and renew a certificate.** Lifetimes are shrinking
  below 50 days. A certificate already issued works offline.
- **Internet to resolve the name.** On a LAN with no internet the name
  fails, so HTTP stays as the fallback.
- **DNS rebinding protection.** Some routers and ISP resolvers refuse
  public names that resolve to private addresses, and the name fails.
  HTTP stays as the fallback again.

If it's built, the laptop offers the `https://` link only when it holds a
valid certificate and the name resolves to it. Otherwise it offers
`http://`. Nothing on the tablet changes.

## What would change this

- **Add `lampstand.direct`** if users report the tablet sleeping despite
  the video workaround, or a hosted Lampstand service appears that could
  carry the DNS and certificate API.
- **Require HTTPS, or keep it off HTTP entirely,** before any remote
  feature goes beyond driving the service, such as the browser planner
  (CD-5) on the LAN.
- **Revisit the video workaround** if Safari or Chrome stop letting a muted
  inline video keep the screen on.
