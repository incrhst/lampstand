# 0010. Roles: Projection, Operator, Stream and Audio, held by devices

- Status: accepted and built for 0.8.0, except point 8 (sound to the stream), which is planned and unbuilt (the map is [issue 114](https://github.com/incrhst/lampstand/issues/114)); amended after the build, see "Decided during the build"
- Date: 2026-10-24

## Question

Today one device holds the presenter lease ([0008](0008-presenter-lease.md))
and one other may drive it ([0009](0009-driving-another-device.md)), and OBS
control lives on the presenting machine. A church that wants a stream operator
on their own machine, running OBS and Zoom (the OBS Virtual Camera feeding
Zoom), has no way to say so. What are the duties, who holds them, how do they
move, and where does OBS live?

## Decision

A **role** is a duty, who is doing what right now. It is not a permission:
which person or Teams role *may* hold a duty is the Teams ticket (T18), and
this record names one place for that check (below). Teams "roles" in
[ARCHITECTURE](../ARCHITECTURE.md) are permissions; this record calls
the four duties *roles* only because the app does.

1. **Four roles at launch, all single-holder.**
   - **Projection**, "runs the screens": the projector and stage screens, the
     live engine and the local server OBS reads from. Today's lease. It is
     tied to the machine wired to the screens and does not move by itself: only
     a device with screens set up can hold it, and any other device's "Take"
     is greyed out ("No screens are set up on this device"). It can run
     unattended, since the service is operated from other devices.
   - **Operator**, "runs the service": next, previous, jump, take an item
     live. Today's driver seat, as a role of its own.
   - **Stream**, "runs what goes out online": the OBS program scene, mutes
     and preview.
   - **Audio**, "runs the sound": plays the sound Lampstand makes (the sound
     of videos, music-only items and backing tracks for singers) on its own
     output. No other device plays it. It can be the same device as
     Projection, as today, or a device at the mixer, and a church can start
     with one machine and spin Audio off later.
   Stage display, Planner and a video switcher are deferred, not rejected.
   Audio was deferred in the first design and brought in when backing tracks
   for singers came up: the sound desk is often far from the screens.
2. **A device holds a role, not a person.** The roster shows the device's
   friendly name. A device may hold any mix of roles. People come later
   through Teams, which gives a device an owner without changing the role
   record.
3. **Browsers are helpers, never holders.** The PIN-paired tablet or phone
   remote ([0006](0006-tablet-remote-in-the-browser.md)) acts for the device
   that holds a role, today sending Operator-type commands to Projection. It
   has no seat, no roster entry as a holder and no hand-over prompt. A browser
   Stream view is a later ticket and needs its own security decision.
4. **Same handoff rules for every role.**
   - *Ask and approve:* "Mary's laptop wants to run the stream. Allow / Not
     now", possible only while the holder is connected.
   - *Takeover:* only when the holder has been offline for the takeover time
     (two minutes by default, per device), or nobody holds the role. The
     claimant waits ten seconds longer than the takeover time (see below).
   - *One ask at a time:* a holder answers one ask; a second asker is told
     who is already asking and is not queued.
   - *Always allow this device:* for Operator and Stream, remembered per
     device. Never for Projection or Audio, because handing either over
     clears what is on air (the screens, or the music that is playing).
   - *On air at a hand-over:* Projection clears the screens and Audio stops
     what is playing, and the prompt says so ("Allow and clear the screens",
     "Allow and stop the music"). Operator leaves the current slide up.
     Stream leaves OBS on its current scene. No fallback scene.
   - A returning holder yields, as the presenter does.
   - The Projection holder may take the Operator seat back at any time without
     being allowed, and the Operator may hand the seat back (silently).
5. **Operator needs Projection. Audio needs Projection for new cues.** While
   nobody holds Projection the Operator controls are disabled and the seat is
   kept. Roles are otherwise independent: handing Projection to another device
   moves neither the Operator seat nor Audio, and the new Projection holder
   announces its state to the Operator. The Audio holder receives its cues (start this file, stop) from
   Projection, so it starts nothing new without it, but what is already
   playing keeps playing and its stop and pause stay on the Audio holder.
   Projection and Stream depend on nothing: the stream desk can work if the
   house machine drops.
6. **The Stream holder holds the OBS connection itself.** On a stream machine
   that is its own OBS on localhost, so the OBS password stays there.
   Projection sends the scene of each item that goes live to the Stream
   holder as a *cue*; the holder applies it. The OBS drawer, scene follower,
   preview and mutes run on the Stream holder. If the holder is offline at
   go-live the cue waits and is applied when it returns; Projection never
   switches OBS itself while Stream is held, so two devices never both
   switch. With nobody holding Stream, Projection keeps the OBS duty as
   today, so a one-machine church sees nothing new.
7. **Sound from the room is the mixer's concern, not Lampstand's, and
   belongs to no role** (it is not the Audio role, which plays only what
   Lampstand makes). Microphones, the band and the house mix reach the
   stream through the church's own mixer, and cameras plug into the stream
   machine or reach OBS over the network. Each set-up sorts this out with its
   own equipment; the source may be any device, several, or none. Lampstand
   never carries camera video or room sound.
8. **Sound that Lampstand plays goes with the picture to OBS.** When a church
   turns on "Send sound to the stream" (Setup › OBS, off by default), the
   stream output's browser page plays the sound of a video or music item, and
   OBS captures it (the browser source is set to "Control audio via OBS").
   The stream's sound and picture then come from one element and stay in step,
   the Stream holder has OBS's own mute and fader for it, and the mixer feed
   needs only the microphones. Nothing else changes: the house still hears the
   clip once, from the Audio holder's output (decision 0005 stands: no faders
   in Lampstand). A church that already sends the computer's channel
   to the stream through the mixer leaves the option off, or the sound doubles;
   the setup screen says so. Contracts for music-only items are in the
   [roles spec](../roles.md#sound-that-lampstand-plays).
   Needs a prototype first (ticket #125, "Prototype: can the OBS browser source
   play Lampstand's sound through OBS?"); until it passes, the mixer route is
   the documented way. **Planned, not built** (ticket #137; the prototype has
   not been run). The only OBS audio setting Lampstand may change is
   "Control audio via OBS" on its own browser source; it never touches
   another source or any other audio setting. Today Lampstand sets none of it.

9. **Audio is a role for backing tracks.** For 0.8.0: a music-only item
   (a song's backing track, walk-in music), attached to a song or on its own;
   "play when this goes live" or "wait for me to press play"; play, pause,
   stop with a fade, and a panic stop that always works, on the Audio holder
   and from the Operator and helpers (helpers planned); and a choice of sound output on the
   Audio holder (Setup › Sound), so the track goes to the mixer's interface
   and not the projector's. The Audio holder gets the files from sync and
   starts them on Projection's cue. Count-in, a click on a separate output for
   the band, and key or tempo changes are later tickets.

10. **Sound lives in the order of the service.** Music-only items and
    backing tracks are items in the service document (name, file, length,
    plays-on-its-own or waits, don't-send-to-the-stream), so they sync, count
    in the service timing and can be planned by any device. The Audio role
    decides only which device plays them; moving Audio never edits the order.
    The Present view shows what sound is playing and what is next, with the
    panic stop.

11. **A confidence screen is not a role.** It is a second output on the
    Projection machine, or a browser page on any device (a tablet), which is
    a helper like the remote. The browser stage page is the planned stage
    display and is built apart from this work. A native stage output on a
    second Lampstand machine would be a later Stage display role, not
    single-holder, since a church can have several stage screens.

Defaults and details that the map did not grill were approved with it and
adjusted during the build: see "Decided during the build" below and the
[roles spec](../roles.md).

## Decided during the build

- **The Projection holder retakes Operator unasked.** It is the machine that
  can always step in. The device that held Operator is told once.
- **"Hand the service back"** (0.7.0's "Stop driving") is silent: the seat goes
  back to the Projection holder, or is let go if that device is not connected.
- **Moving from 0.7.0** gives Operator and Audio to the Projection holder, at
  the lease's epoch. A first device that has no screens gets none of the
  three (Audio only goes with Projection). Stream is left unheld.
- **Roles are independent on a Projection hand-over.** Operator and Audio stay
  where they are.
- **The claimant waits the takeover time plus ten seconds** (default
  effectively 2 minutes 10 seconds; PR #157, `TAKEOVER_MARGIN`), by its own
  count of the holder's silence, so a holder that returns at once and counts
  the claimant's absence from a slightly earlier last frame agrees with it.
  PR #160 (merged, protocol 6) adds that a claimant also waits by the holder's advertised
  setting when that is longer. A split that remains after this (one-way link
  failure) is planned, not built (issue #153, closed): the planned fix is a signed refusal: a claimant whose claim the holder refuses hands the role back with an ordinary signed hand-over, and a cooldown stops it claiming again at once.
- **Signed role records.** A record carries up to eight signed steps; a device
  takes a newer record only if its steps join its own state and every signer
  is known; one that cannot be joined is *unverified* and waits. The volunteer
  sees nothing about this yet.
- **Always allow only for Operator and Stream**; one ask at a time.
- **Warnings.** Stream: none (Projection keeps the OBS duty when nobody holds
  Stream). Audio: "Nobody runs the sound." is built; the wording "Nobody runs
  the sound, so {item} will not play." is not.
- **Sound is opt-in.** Setup > Sound > "Use sound in services" is off by
  default and only decides whether Plan offers the buttons that add sound.
- **One-click OBS setup** runs on the machine that serves the pictures, with
  OBS on another computer, and writes that machine's network address into the
  browser source (never `localhost`); it is not run on the Stream holder.
- **Platforms.** Built and tested on macOS and Windows, and also run on Linux
  CI (the Linux builds pass); real Linux hardware has not been checked.

## Where the Teams check goes

At the two places that grant a role, `Roles::claim` and `Roles::allow` (the
generalised `Presenter::claim` and `Presenter::allow`). Nothing else changes
when T18 lands.

## Consequences

- Sync protocol 7 and a role record per role. Devices on an older version are refused with the usual message saying which side should update.
- The OBS drawer and scene follower move from "the presenting device" to "the
  device holding Stream, or Projection when nobody does".
- The browser-source URL must carry the Projection machine's LAN address, not
  localhost, and the address must be pinnable (built in #133).
- Going live depends on a second device (the Stream holder) being online for
  the OBS part, though never for the screens. The status bar says when it is
  not.
