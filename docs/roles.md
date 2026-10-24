# Roles: who runs what

Status: proposed for 0.8.0. Decisions: [0010](decisions/0010-roles-and-handoff.md), the map is [issue 114](https://github.com/incrhst/lampstand/issues/114). Builds on the presenter lease ([0008](decisions/0008-presenter-lease.md)) and driving ([0009](decisions/0009-driving-another-device.md)).

A **role** is a job someone is doing right now. It is not a permission; who may do a job is the Teams work (T18).

## The three roles

| Role | In plain words | What it owns | Needs |
| --- | --- | --- | --- |
| **Projection** | Runs the screens | Projector and stage screens, the live engine, media sound, the local server OBS reads | nothing |
| **Operator** | Runs the service | Next, previous, jump, take an item live | Projection held |
| **Stream** | Runs what goes out online | OBS scene, mutes, preview; OBS and Zoom | nothing |

All three have one holder at a time. A device may hold any mix. A device with no role still edits and syncs freely, as a planning device does today.

## Set-ups: one machine, two, three

You can start with one machine and add the others later. Nothing about the service changes; each role moves to another device by asking.

### 1. One machine, all roles

One computer runs the screens, the service and OBS. It holds Projection, Operator and Stream (it holds them automatically; there is nothing to set up). OBS and Zoom, if used, run on the same computer. The house sound is whatever that computer plays, so for Zoom use a mixer feed into the computer. Nothing is paired.

### 2. Two machines

- **Machine A, in the house:** Projection and Operator. Screens, projector, sound desk.
- **Machine B, the stream machine:** Stream. Runs OBS, Zoom and the OBS Virtual Camera. Connects to its own OBS.

Pair B with A. B asks for Stream; A allows (and can say "always allow this device"). Use a wired network with reserved addresses. The browser source in B's OBS reads A's address.

Variant: B is a second operator laptop (Operator) and OBS stays on A.

### 3. Three machines

- **A, house:** Projection (screens, projector).
- **B, back of the room:** Operator (runs the service, sees what is on air).
- **C, stream machine:** Stream (OBS, Zoom, Virtual Camera).

Operator is disabled if A drops, and wakes when Projection is held again. C keeps working without A. Phones and tablets act as helpers on any of these set-ups (below).

### Sound for the stream

Lampstand sends no sound to OBS or Zoom. **We recommend a house feed from the mixing board straight into the stream machine** through a USB audio interface: use it as the Zoom microphone (with "Original sound" on) and as an OBS audio source. The mix should carry service playback and the microphones. The stream machine's mutes belong to the Stream holder.

### Phones and tablets

A phone or tablet in the browser (the tablet remote, paired by QR code and PIN) is a **helper**: it acts for the device that holds a role, today by sending Operator-type commands (next, previous, show, black, logo, clear) to the Projection machine. It never holds a role, and does not appear in a seat. A browser view for Stream is a later feature.

## How a role moves

The same rules for every role.

1. **Ask.** A device presses its "Take" button for a role. The current holder sees "Mary's laptop wants to run the stream. Allow / Not now" (Operator and Stream also offer "Always allow this device"). It works only while the holder is connected.
2. **Take over.** If the holder has been offline for the takeover time (two minutes by default, Setup › Devices), or nobody holds the role, another device may take it after confirming.
3. **What is on air.** Passing **Projection** clears the screens, and the prompt says so ("Allow and clear the screens"). Passing **Operator** leaves the slide up. Passing **Stream** leaves OBS on its current scene.
4. **A returning holder yields.** A device that comes back after losing a role learns it, drops the role and shows a notice. It cannot win the role back by itself.

"Always allow this device" is not offered for Projection.

## Where OBS lives

- OBS is connected from the **Stream holder**. On a stream machine that is OBS on that machine, so the OBS password stays there.
- When an item goes live, Projection sends its scene as a cue to the Stream holder. If the Stream holder is offline the cue waits and applies when it returns; Projection never switches OBS itself while Stream is held, so there are no double switches.
- With nobody holding Stream, Projection keeps the OBS duty as it does today.
- One-click OBS setup on the Stream holder writes the Projection machine's network address into the browser source (never `localhost`), with a way to pin the address when a machine has more than one.
- The OBS drawer, scene follower, preview and mutes run on the Stream holder.

## Proposed details (not yet grilled; confirm or change)

- **Defaults.** A single device holds all three. Pairing gives the first (pairing) device Projection and Operator; Stream is unheld, so Projection keeps the OBS duty until someone takes Stream.
- **Restarts.** Each role is a record `{ holder, name, epoch }` like the lease, kept in `roles.json` beside `paired.json`, device-local. A restart, crash or sleep leaves a holder that others see as offline; the takeover time applies. A device with no paired devices holds all roles whatever an old record says.
- **Two asks at once.** As in 0008 and 0009: the higher `(epoch, device key)` wins a simultaneous claim; the holder's prompt answers one ask at a time, and a second asker is told who is already asking, and is not queued.
- **Operator across a Projection hand-over.** The Operator seat is kept and the new Projection holder announces its state to the Operator. (Today a lease change ends driving.)
- **Migration.** The lease becomes the Projection record keeping its epoch; the driver seat becomes Operator (the "always allowed" list in `driver.json` carries over). Sync protocol 5.
- **Roster.** The status bar shows the roles this device holds ("Runs the screens", "Runs the service", "Runs the stream"), and for the others who holds them; "Presenting" and "Driving" are replaced by these. Setup › Devices lists each device with its roles and the takeover time. A device with no role is just "Planning".
- **Teams check.** Who may hold a role is decided at `Roles::claim` and `Roles::allow` and nowhere else (T18).
- **Audio.** Stays outside Lampstand. An output-device selector for media playback, or an NDI audio feed, are later tickets.
