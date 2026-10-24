# Roles: who runs what

Status: accepted for 0.8.0; the "Proposed details" at the end were approved with it. Decisions: [0010](decisions/0010-roles-and-handoff.md), the map is [issue 114](https://github.com/incrhst/lampstand/issues/114). Builds on the presenter lease ([0008](decisions/0008-presenter-lease.md)) and driving ([0009](decisions/0009-driving-another-device.md)).

A **role** is a job someone is doing right now. It is not a permission; who may do a job is the Teams work (T18). Ideas set aside for later are in [roles-future.md](roles-future.md).

## The four roles

| Role | In plain words | What it owns | Needs |
| --- | --- | --- | --- |
| **Projection** | Runs the screens | Projector and stage screens, the live engine, the local server OBS reads | nothing |
| **Operator** | Runs the service | Next, previous, jump, take an item live | Projection held |
| **Stream** | Runs what goes out online | OBS scene, mutes, preview; OBS and Zoom | nothing |
| **Audio** | Runs the sound | The sound Lampstand plays: video sound, music-only items, backing tracks; the sound output | Projection, for new cues |

**Projection is tied to the screens.** It belongs to the machine that is wired to the projector and screens, and it does not move by itself. Only a device that has screens set up (a projector, a stage display, or its own display as the output) can hold Projection; the "Take" button on any other device is greyed out and says "No screens are set up on this device". Moving Projection to another machine means that machine is connected to the screens, for example a backup laptop on the same projector switch or a cable moved by hand. Pass it on only then.

**Projection can run unattended.** Nobody has to sit at the Projection machine. It opens its screens when Lampstand starts, keeps the service ready, and everything else (the Operator, the stream, planning) happens from other devices. This is the usual set-up in a larger church: a machine at the projector, and a volunteer with a laptop at the back running the service.

All four have one holder at a time. A device may hold any mix. A device with no role still edits and syncs freely, as a planning device does today.

## Set-ups: one machine, two, three or more

You can start with one machine and add the others later. Nothing about the service changes; each role moves to another device by asking, except Projection, which stays with the machine wired to the screens (above).

### How to read the pictures

Each machine is a box, and the roles it holds are the coloured pills inside it. A role has the same colour everywhere: **Projection blue, Operator green, Stream purple, Audio orange**. An arrow takes the colour of the role that owns it (blue for the picture and for graphics and scene cues, orange for sound, green for next, jump and "what is on air", purple for scene control and the Virtual Camera). Grey is equipment, room sound and the network; a dashed line is a link over the church network or a helper. The pictures are drawn by `diagrams/generate.py`.

### 1. One machine, all roles

One computer runs the screens, the service and OBS. It holds Projection, Operator, Stream and Audio (it holds them automatically; there is nothing to set up). OBS and Zoom, if used, run on the same computer. The house sound is whatever that computer plays, so for Zoom use a mixer feed into the computer. Nothing is paired.

![One computer holds all four roles](diagrams/roles-one-machine.svg)

### 2. Two machines

- **Machine A, at the projector:** Projection, Operator and Audio. Screens, projector, and the sound Lampstand plays. It can sit unattended; the service can be run from B.
- **Machine B, the stream machine:** Stream. Runs OBS, Zoom and the OBS Virtual Camera. Connects to its own OBS.

Pair B with A. B asks for Stream; A allows (and can say "always allow this device"). Use a wired network with reserved addresses. The browser source in B's OBS reads A's address.

Variant: B is a second operator laptop (Operator) and OBS stays on A.

![Two machines: the projector machine and the stream machine](diagrams/roles-two-machines.svg)

### 3. Three machines

- **A, at the projector:** Projection (screens, projector), and Audio unless it moves. It can run unattended.
- **B, back of the room:** Operator (runs the service, sees what is on air).
- **C, stream machine:** Stream (OBS, Zoom, Virtual Camera).

Operator is disabled if A drops, and wakes when Projection is held again. C keeps working without A. Phones and tablets act as helpers on any of these set-ups (below).

![Three machines: projector, operator and stream](diagrams/roles-three-machines.svg)

### 4. Four machines, with the sound desk

Where the mixer is far from the projector, add a machine at the desk that holds Audio. Backing tracks and video sound then play from there, straight into the mixer.

![Four machines: with Audio at the sound desk](diagrams/roles-four-machines.svg)

### Audio at the sound desk

Where the machine running the screens is far from the mixer, spin Audio off to a machine at the desk: its own laptop, or B or C above. That device asks for Audio, and from then on Lampstand's sound (video sound, backing tracks) plays from its output into the mixer. Projection keeps rendering and stays silent. Passing Audio stops whatever is playing, and the prompt says so.

### What comes from the house: cameras, microphones, the band

None of it goes through Lampstand, and no role owns it. Lampstand never carries camera video or sound from the room.

- **Cameras** belong to OBS. Plug them into the stream machine (USB or an HDMI capture box), or, where the stream machine is far from the cameras, bring them in over the network (network cameras, NDI or RTSP) into OBS. Putting the stream machine near the cameras and the mixer is the simplest.
- **Microphones, the band and the house mix** come from the church's mixing board. Each set-up sorts it out with its own mixer, and the source may be any device, several, or none. Whatever the mixer sends has to reach the machine running Zoom and the OBS Virtual Camera. One common way, as an example and not a requirement: a feed from the board into a USB audio interface on the stream machine, used as the Zoom microphone (with "Original sound" on) and as an OBS audio source.
- **Lampstand's own graphics** reach OBS as the browser source, so the stream shows camera and graphics together, switched by the Stream holder.

### Sound that Lampstand plays

A video, a music-only item or a backing track has sound. It always plays in the house, once, from the **Audio holder's** output (the machine running the screens unless Audio has moved). Pick the output in Setup › Sound, so it goes to the mixer's interface and not the projector's.

- **Backing tracks for singers.** A music-only item can be attached to a song or stand alone. It plays when the item goes live, or waits for someone to press play. It can play, pause and stop with a fade, and there is a panic stop that always works: on the Audio holder, from the Operator, and from a helper. The Audio holder starts it on Projection's cue from its own copy of the file. If Projection is away, what is playing keeps playing and can still be stopped, but nothing new starts. Count-in, a click for the band, and key or tempo changes are later features.
- **Option "Send sound to the stream"** (Setup › OBS, off by default). When it is on, the stream output also plays that sound in its OBS browser source, and OBS captures it. The stream's sound and picture then match. The Stream holder gets OBS's own mute and fader for it. The mixer feed then needs only the microphones.
- **Do not do both.** If the mixer already sends the computer's channel to the stream, leave the option off, or the sound doubles. The setup screen says so.
- **Music-only items** follow the same rule as a video's sound. The rules, proposed for approval:
  1. A music-only item is a scene with sound and no picture. With the option on, the stream output plays it, as it plays a video's sound.
  2. Every item has "Don't send this sound to the stream" (for copyright, or because the sound reaches the stream another way). It is off unless set. The house is unaffected.
  3. Background music that runs under several items follows the item that started it, and stops with it, for the house and the stream alike.
  4. If the stream output's page is not connected when a sound starts, the sound does not fall back to anything. The Stream holder's view says "Sound is not reaching the stream", and the house is unaffected.
  5. With the option off, nothing changes: the stream hears whatever the mixer sends.
- Until the prototype passes (ticket "Prototype: can the OBS browser source play Lampstand's sound through OBS?"), the mixer route is the documented way, and the lip-sync tip applies: delay the mixer source in OBS (Advanced Audio Properties, Sync Offset) until it matches the picture.

### Audio in the order of the service

Sound is part of the service, so it lives in the service order and syncs like the rest of the plan, whoever holds Audio. The Audio role only decides which device plays it.

- **Sound is an item, or part of one.** A music-only item (walk-in music, a song's backing track, a sound cue) sits in the order like any item, with a name, a file and a length. A song can also carry a backing track that plays with it. Videos keep their sound as part of the item.
- **It has a place and a time.** The order shows where the sound starts, how long it runs and whether it plays on its own when the item goes live or waits to be started. The service timing counts it, so the planned end time includes the music.
- **Planners can plan it.** Any device can add or change sound in the order, as it can plan anything else, with no role needed. Files travel by sync to the Audio holder ahead of the service, and Plan says if a file is missing there.
- **The Operator sees it.** The Present view shows what sound is playing and what is next, with the panic stop, whether Audio is on the same device or not.
- **The roster shows who plays it.** The status bar says who runs the sound, and Present warns if nobody holds Audio when a sound item is next.
- **Moving Audio never edits the order.** The order is the plan; only the playing changes hands.

### Confidence screens and stage displays

A confidence screen (speaker's notes, next slide, clock) is not a role. It can be:

- **A second display on the Projection machine,** set up as a stage output. Today's way, limited by the cable run to the stage.
- **A browser page on any device** (a tablet on a music stand, a laptop, a TV with a browser), opened from a QR code like the tablet remote. It is a helper: it shows what Projection is showing, never holds anything, and never affects the main screens if it drops off Wi-Fi. This is the answer when the stage is far from the projector.

The browser stage page is the planned stage display (PRD LV-5, PS-7; decision 0006 lists it as a later page) and is built separately from the roles work. A native stage output on a second Lampstand machine would be a later Stage display role; it would not be single-holder, since a church can have several stage screens.

![A confidence screen as a second display or a tablet page](diagrams/roles-confidence-screen.svg)

### Phones and tablets

A phone or tablet in the browser (the tablet remote, paired by QR code and PIN) is a **helper**: it acts for the device that holds a role, today by sending Operator-type commands (next, previous, show, black, logo, clear) to the Projection machine, and later the panic stop for music. It never holds a role, and does not appear in a seat. A browser view for Stream is a later feature.

## How a role moves

The same rules for every role.

1. **Ask.** A device presses its "Take" button for a role. The current holder sees "Mary's laptop wants to run the stream. Allow / Not now" (Operator and Stream also offer "Always allow this device"). It works only while the holder is connected.
2. **Take over.** If the holder has been offline for the takeover time (two minutes by default, Setup › Devices), or nobody holds the role, another device may take it after confirming.
3. **What is on air.** Passing **Projection** clears the screens, and passing **Audio** stops what is playing; the prompt says so ("Allow and clear the screens", "Allow and stop the music"). Passing **Operator** leaves the slide up. Passing **Stream** leaves OBS on its current scene.
4. **A returning holder yields.** A device that comes back after losing a role learns it, drops the role and shows a notice. It cannot win the role back by itself.

"Always allow this device" is not offered for Projection or Audio.

## Where OBS lives

- OBS is connected from the **Stream holder**. On a stream machine that is OBS on that machine, so the OBS password stays there.
- When an item goes live, Projection sends its scene as a cue to the Stream holder. If the Stream holder is offline the cue waits and applies when it returns; Projection never switches OBS itself while Stream is held, so there are no double switches.
- With nobody holding Stream, Projection keeps the OBS duty as it does today.
- One-click OBS setup on the Stream holder writes the Projection machine's network address into the browser source (never `localhost`), with a way to pin the address when a machine has more than one.
- The OBS drawer, scene follower, preview and mutes run on the Stream holder.

## Proposed details (not yet grilled; confirm or change)

- **Defaults.** A single device holds all four. Pairing gives the first (pairing) device Projection, Operator and Audio, so sound keeps playing where it does today; Stream is unheld, so Projection keeps the OBS duty until someone takes Stream.
- **Restarts.** Each role is a record `{ holder, name, epoch }` like the lease, kept in `roles.json` beside `paired.json`, device-local. A restart, crash or sleep leaves a holder that others see as offline; the takeover time applies. A device with no paired devices holds all roles whatever an old record says.
- **Two asks at once.** As in 0008 and 0009: the higher `(epoch, device key)` wins a simultaneous claim; the holder's prompt answers one ask at a time, and a second asker is told who is already asking, and is not queued.
- **Operator across a Projection hand-over.** The Operator seat is kept and the new Projection holder announces its state to the Operator. (Today a lease change ends driving.)
- **Migration.** The lease becomes the Projection record keeping its epoch; the driver seat becomes Operator (the "always allowed" list in `driver.json` carries over). Sync protocol 5.
- **Roster.** The status bar shows the roles this device holds ("Runs the screens", "Runs the service", "Runs the stream", "Runs the sound"), and for the others who holds them; "Presenting" and "Driving" are replaced by these. Setup › Devices lists each device with its roles and the takeover time. A device with no role is just "Planning".
- **Teams check.** Who may hold a role is decided at `Roles::claim` and `Roles::allow` and nowhere else (T18).
- **Sound.** Room sound (microphones, the band) stays outside Lampstand and outside the roles. Sound that Lampstand plays belongs to the Audio role and goes with the picture to OBS when the church turns it on (above). NDI audio is a later ticket.
- **Service order.** A sound item or a song's backing track is stored in the service document (name, file, length, "plays on its own" or "wait", "don't send to the stream"), so it syncs and counts in the service timing. This needs a new item kind or a node in the existing one; to be settled in the build ticket.
- **Audio and the stream option.** The Audio holder plays for the house from its own copy of the file; the stream page plays its own copy through OBS. The two are not locked to each other, which is fine for the room and for the stream.
