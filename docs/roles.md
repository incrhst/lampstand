# Roles: who runs what

Status: built for 0.8.0 (and 0.9.0 for settling a refused takeover, showing a record that could not be checked, and the role bar), except the parts marked **Planned** (see "Built and planned" below). Decisions: [0010](decisions/0010-roles-and-handoff.md); the map is [issue 114](https://github.com/incrhst/lampstand/issues/114). Builds on the presenter lease ([0008](decisions/0008-presenter-lease.md)) and driving ([0009](decisions/0009-driving-another-device.md)), which became the Projection and Operator roles. The page for volunteers is [the Roles page on the website](../site/docs/roles.html). Ideas set aside for later are in [roles-future.md](roles-future.md).

A **role** is a job someone is doing right now. It is not a permission; who may do a job is the Teams work (T18).

Roles are built and tested on macOS and Windows, and also run on Linux CI (the Linux builds pass); we have not checked real Linux hardware.

## Built and planned

| Part | State |
| --- | --- |
| The four roles, asking, allowing, taking over, signed role records, the roster | Built (#128, #129, #130, #131) |
| Moving from 0.7.0, the Operator seat, "Hand the service back"; waiting by the holder's takeover setting as well as one's own (PR #160, protocol 6) | Built (#128, #129, #160) |
| Stream holder owns OBS, OBS cues, the stream desk view | Built (#132, #133, #134) |
| Sound in the order of the service, Setup > Sound, playing, the output chooser, the panic stop | Built (#135, #136) |
| Why a sound cannot play (file not on the Audio device, unreadable or unsupported, output not working, missing or absent), shown on the Operator and Projection screens; a dead output is dropped and retried, and a chosen output that is gone stays silent with no fallback (protocol 7) | Built (#162) |
| Settling a claim that the holder refuses (a signed refusal, a hand-back and a cooldown; protocol 8) | Built (#183; design from PR #160's description, issue #153) |
| Every claim judged by the device it was taken from, with its verdict signed into the record (protocol 9) | Built (PR #205) |
| "Send sound to the stream" (the OBS browser source plays Lampstand's sound) | Planned (#137); needs the OBS prototype (#125), which has not been run |
| A music-only item that plays on its own starts when Next reaches it; the Sound box folds, with a queue that follows the service | Built (#181, #191) |
| A warning in the role bar that names the sound item that will not play | Built (#182, see "Audio warnings") |
| The tablet and phone remote, the browser stage page, a Stream view in the browser | Planned (decision 0006; not in the app yet) |
| Showing the volunteer that a role's record could not be checked | Built (#184): the roster and the status bar say so, with what to do |
| The role bar: a strip in the role colours for the roles a machine holds, in the status bar and beside each device in Setup > Devices | Built (0.9.0) |

## The four roles

| Role | In plain words | What it owns | Needs |
| --- | --- | --- | --- |
| **Projection** | Runs the screens | Projector and stage screens, the live engine, the local server OBS reads | screens set up on that device |
| **Operator** | Runs the service | Next, previous, jump, take an item live | Projection held |
| **Stream** | Runs the stream | OBS scene, mutes, preview (Lampstand controls only OBS) | nothing |
| **Audio** | Runs the sound | The sound Lampstand plays: video sound, music-only items, backing tracks; the sound output | Projection, for new cues |

The people-facing words are "the screens", "the service", "the stream" and "the sound" (`app/src/lib/roles.ts`, `lampstand_sync::wording`). The names Projection, Operator, Stream and Audio are for developers and for this document.

**Projection is tied to the screens.** It belongs to the machine wired to the projector and screens, and does not move by itself. Only a device that has screens set up (a projector, a stage display, or its own display as an output) can hold it. The app tells the sync node whenever an output is turned on or removed (`SyncNode::set_screens`). On any other device the Take button is greyed out with "No screens are set up on this device". Moving Projection to another machine means that machine is connected to the screens, for example a backup laptop on the same projector switch. Pass it on only then.

**Projection can run unattended.** Nobody has to sit at the Projection machine. It opens its screens when Lampstand starts and keeps the service ready; the Operator, the stream and the planning happen on other devices.

All four have one holder at a time. A device may hold any mix. A device with no role still edits and syncs freely, as a planning device did in 0.7.0. A device with nothing paired holds all four, whatever an old record says, and shows no role words.

### What each role needs

- **Projection** depends on nothing. Without screens a device cannot take it.
- **Operator** needs Projection to be held by a device the Operator can reach. While the Projection holder is offline the Operator's controls wait and the seat is kept; they wake when the holder is back. The Operator seat cannot be taken or handed on while nobody holds Projection ("The service can't be run until someone runs the screens.").
- **Audio** can be held anywhere. It receives its cues (play, pause, stop) from Projection, so it starts nothing new without it; what is playing keeps playing and its stop stays on the Audio holder.
- **Stream** depends on nothing: the stream desk works if the house machine drops.

## Set-ups: one machine, two, three or more

You can start with one machine and add the others later. Nothing about the service changes; each role moves to another device by asking, except Projection, which stays with the machine wired to the screens.

### How to read the pictures

Each machine is a box and the roles it holds are the coloured pills inside it. A role has the same colour everywhere: **Projection blue, Operator green, Stream purple, Audio orange**. An arrow takes the colour of the role that owns it (blue for the picture and for graphics and scene cues, orange for sound, green for next, jump and "what is on air", purple for scene control and the Virtual Camera). Grey is equipment, room sound and the network; a dashed line is a link over the church network or a helper. The pictures are drawn by `diagrams/generate.py` (not changed by the build; see [diagrams/README.md](diagrams/README.md)). The website uses copies in `site/docs/diagrams/`.

### 1. One machine, all roles

One computer runs the screens, the service and OBS. It holds all four (automatically; nothing to set up). OBS and Zoom, if used, run on the same computer. The house sound is whatever that computer plays, so for Zoom use a mixer feed into the computer. Nothing is paired.

![One computer holds all four roles](diagrams/roles-one-machine.svg)

### 2. Two machines

- **Machine A, at the projector:** Projection, Operator and Audio. It can sit unattended; the service can be run from B.
- **Machine B, the stream machine:** Stream. Runs OBS, Zoom and the OBS Virtual Camera, and connects to its own OBS.

Pair B with A. A shows the pairing code and B types it; the device that showed the code takes Projection, Operator and Audio (if it has screens), and Stream is left unheld, so A keeps the OBS duty until B takes Stream. B presses Take on "Runs the stream"; A allows (and can say "Always allow this device"). Use a wired network with fixed addresses (ask whoever looks after your network). B's OBS shows A's pictures through a browser source that reads A's network address.

To set that browser source up in one click:

1. **On B (the stream machine):** in OBS turn on the WebSocket server (Tools > WebSocket Server Settings > Enable WebSocket server, then Apply). In Lampstand on B, go to Setup > OBS > Control OBS, connect to this computer (`localhost`) and press "Save and test". The Stream holder uses its own OBS connection; without it scene cues have no OBS to switch.
2. **On A:** in Setup > OBS > "Send to OBS", press "Add a picture for OBS" and turn it on. "Set up OBS" stays greyed ("Add a picture for OBS above and turn it on first.") until then.
3. **Still on A:** under "Control OBS" type B's address in "Computer" and its password, then "Save and test". Otherwise "Set up OBS" says "Lampstand isn't connected to OBS yet. Save and test the connection first."
4. **On A:** press "Set up OBS" (it appears when OBS is on another computer, and refuses while OBS is streaming or recording). It makes a "Lampstand" collection in B's OBS with the scenes Slides, Worship and Pastor and slides, each showing a browser source named "Lampstand stream" whose address is A's network address, never `localhost`. 5. "Use this address" only matters if A has several network addresses: it pins the one used.

This is the built form of the one-click setup: it runs on the machine that serves the pictures, not on the stream holder (the spec first said the stream holder; see "Where OBS lives").

Variant: B is a second operator laptop (Operator) and OBS stays on A.

![Two machines: the projector machine and the stream machine](diagrams/roles-two-machines.svg)

### 3. Three machines

- **A, at the projector:** Projection, and Audio unless it moves. It can run unattended.
- **B, back of the room:** Operator (runs the service, sees what is on air).
- **C, stream machine:** Stream (OBS, Zoom, Virtual Camera).

Operator is disabled if A drops and wakes when Projection is held again. C keeps working without A, but scene changes wait for A (the stream desk says "{name} runs the screens and is offline. Scene changes wait for it.").

![Three machines: projector, operator and stream](diagrams/roles-three-machines.svg)

### 4. Four machines, with the sound desk

Where the mixer is far from the projector, add a machine at the desk that holds Audio. Backing tracks and video sound then play from there, straight into the mixer.

![Four machines: with Audio at the sound desk](diagrams/roles-four-machines.svg)

### Audio at the sound desk

Where the machine running the screens is far from the mixer, spin Audio off to a machine at the desk: its own laptop, or B or C above. That device takes Audio, and from then on Lampstand's sound plays from its output into the mixer. Projection keeps rendering and stays silent. Passing Audio stops whatever is playing, and the prompt says so.

### What comes from the house: cameras, microphones, the band

None of it goes through Lampstand, and no role owns it. Lampstand never carries camera video or sound from the room.

- **Cameras** belong to OBS. Plug them into the stream machine (USB or an HDMI capture box), or bring them in over the network (network cameras, NDI or RTSP) into OBS. Putting the stream machine near the cameras and the mixer is the simplest.
- **Microphones, the band and the house mix** come from the church's mixing board. Whatever the mixer sends has to reach the machine running Zoom and the OBS Virtual Camera. One common way, as an example and not a requirement: a feed from the board into a USB audio interface on the stream machine, used as the Zoom microphone (with "Original sound" on) and as an OBS audio source.
- **Lampstand's own graphics** reach OBS as the browser source, so the stream shows camera and graphics together, switched by the Stream holder.

### Sound that Lampstand plays

A video, a music-only item or a backing track has sound. It plays in the house, once, from the **Audio holder's** output (the machine running the screens unless Audio has moved). Pick the output in Setup > Sound ("Where the sound comes out"), so it goes to the mixer's interface and not the projector's. Changing the output stops what is playing. A chosen output that is not connected falls back to the computer's default, and the screen says so.

**Built (#136):**

- **Backing tracks.** A music-only item sits in the service on its own; a backing track is attached to a song. A song's backing track that "plays on its own" starts when the song goes live. A music-only item has no slides, but **Next stops on it** (#181): it goes live, the screens keep the slide before it, and if it "plays on its own" it starts, on the device that runs the sound. If other sound is playing, that sound fades out over a second and the new one starts. A sound set to "Waits for someone to press play" waits for Play whichever it is, from the Sound box in Present. Previous passes over a music-only item, so going back never starts sound; inside a looping section the loop passes over it, as before.
- **Controls.** Play, Pause, Resume, "Fade out" and the panic stop "Stop the sound now", in the sound panel of Present on any device (a request goes to whichever device holds Audio; Play, Pause, Resume and Stop count only from the devices that run the screens or the service, the panic stop from any paired device; it says "The device that runs the sound is not connected." if it is away). From a phone or tablet helper: **Planned**, since the remote is not built.
- **If Projection is away**, what is playing keeps playing and can still be stopped, but nothing new starts.
- **Files** travel by sync to every device that has the service (not only the Audio holder, because Audio can move), for any service that starts later, or started less than 36 hours ago; one with no date is not fetched. Plan shows "File missing" on an item whose file is not on this device, and whether it is on its way.
- Count-in, a click for the band, and key or tempo changes are later features.

**Planned (#137): "Send sound to the stream"** (Setup > OBS, off by default). When it is on, the stream output's OBS browser source would also play that sound, and OBS would capture it, so the stream's sound and picture match and the Stream holder has OBS's own mute and fader for it. It needs a prototype first (ticket #125, "can the OBS browser source play Lampstand's sound through OBS?"), which has not been run. None of this exists in the app: there is no such switch, no "Not sent to the stream" effect (the per-item option "Don't send this sound to the stream" and its "Not sent to the stream" tag exist and change nothing yet), and Lampstand does not touch OBS's audio settings.

Until then, the mixer route is the way, and the lip-sync tip applies: delay the mixer source in OBS (Advanced Audio Properties, Sync Offset) until it matches the picture.

The rules planned for #137, for approval when it is built:

1. A music-only item is a scene with sound and no picture. With the option on, the stream output plays it as it plays a video's sound.
2. Every item has "Don't send this sound to the stream" (for copyright, or because the sound reaches the stream another way). It is off unless set. The house is unaffected.
3. Background music that runs under several items follows the item that started it, and stops with it, for the house and the stream alike.
4. If the stream output's page is not connected when a sound starts, the sound does not fall back to anything. The Stream holder's view says "Sound is not reaching the stream", and the house is unaffected.
5. With the option off, nothing changes: the stream hears whatever the mixer sends.
6. Do not do both. If the mixer already sends the computer's channel to the stream, leave the option off, or the sound doubles. The setup screen would say so.
7. **The one OBS audio exception** (decision 0010, point 8): Lampstand may set "Control audio via OBS" on its **own** browser source only, so OBS treats the page's sound as an audio source. It never changes another source, and never any other OBS audio setting. Today "Set up OBS" does not set it.

### Audio in the order of the service

Sound is part of the service, so it lives in the service order and syncs like the rest of the plan, whoever holds Audio. The Audio role only decides which device plays it.

- **Built.** A music-only item or a song's backing track has a name, a file, a length and "plays on its own" or "waits"; the order shows them, and the service heading shows the music-only total ("Sound: 12:30", or "at least" when a file gave no length). Any device can plan sound, with no role. **Setup > Sound > "Use sound in services"** (off by default) decides whether Plan offers the buttons that add sound ("Add sound", "Add a backing track"). Sound already in a service always shows, with the switch on or off. A church that never turns it on sees nothing about sound.
- **The Operator sees it.** Present's Sound box shows what plays now with its position, the controls above, and the service's sound as a queue that follows the service (#191): "Up next" (the first sound after the live item), and "Show all (n)" for the whole list, with sound already played greyed under "Played (n)" and still playable. A backing track keeps its own entry. When the service reaches a sound that waits for Play, that entry is highlighted ("Waiting for Play") until it plays. The box folds to one line: what plays, "Fade out" and "Stop the sound now", which is never hidden; a warning and a sound waiting for Play still show. It is open before the service starts and folded once it is under way; each choice is remembered on that computer. The phone remote's Sound box has the same queue (Up next, the waiting sound, "Show all"), always open. The roster says who runs the sound.
- **Moving Audio never edits the order.** The order is the plan; only the playing changes hands.
- **Not yet.** The planned end time does not count the music, because there is no planned end time yet (the service heading shows the music's total). "Play next" (line up a sound to start when the one playing ends) is not built: Play replaces what is playing.
- **Older versions.** A Lampstand without sound cannot open a service with a music-only item; it says the service was saved by a newer Lampstand (details in ARCHITECTURE).

### Audio warnings

**The role bar** (the notices across the top of every screen) warns ahead of a sound item when no device holds Audio, naming the next sound the service will reach (the one on air first): **"Nobody runs the sound, so {item} won't play."** (#182; the wording is settled). The Sound box says the rest when it applies: **"Nobody runs the sound."** (no device holds Audio; the core's refusal, the same sentence), **"The device that runs the sound is not connected."** (the Audio holder is offline), a message when a sound failed to start, and "This computer's sound output is not working. Choose another in Setup > Sound." on the Audio holder.

A device that has only just connected, before it has heard who holds what, is refused with "Nobody is presenting." (the 0.7.0 wording, kept in `wording.rs` `Denied::Settling`); whether it should change is a question for the owner.

**Stream warnings: none.** Nothing warns that nobody runs the stream, because with nobody on Stream the Projection holder keeps the OBS duty and nothing is lost. The one stream line is on the device that waits for the stream machine: "{name} runs the stream and is offline. OBS will catch up when it is back."

### Confidence screens and stage displays

A confidence screen (speaker's notes, next slide, clock) is not a role. It can be:

- **A second display on the Projection machine,** set up as a stage output. Today's way, limited by the cable run to the stage.
- **A browser page on any device** (a tablet on a music stand, a TV with a browser), opened from a QR code like the tablet remote. It would show what Projection is showing, never hold anything, and never affect the main screens if it drops off Wi-Fi. **Planned**: the browser stage page (PRD LV-5, PS-7; decision 0006) is built separately from the roles work and is not in the app yet.

A native stage output on a second Lampstand machine would be a later Stage display role ([roles-future.md](roles-future.md)).

![A confidence screen as a second display or a tablet page](diagrams/roles-confidence-screen.svg)

### Phones and tablets

A phone or tablet in the browser is a **helper**: it acts for the device that holds a role, by sending Operator-type commands (next, previous, show, black, logo, clear) to the Projection machine, and later the panic stop for music. It never holds a role and does not appear in a seat. **Planned**: the tablet remote (pairing by QR code and PIN, decision 0006, tickets T99 to T101) is not in the app yet. Today a second laptop can run the service by holding the Operator role. A browser view for Stream is a later feature.

## How a role moves

The same rules for every role.

1. **Ask.** A device presses **Take** for a role. The current holder sees "Mary's laptop wants to run the stream." with **Allow**, **Not now** and, for Operator and Stream only, **Always allow this device**. It works only while the holder is connected. The asker sees "Waiting for {holder} to allow..." and then either holds the role, or "{holder} said not now." with **Ask again**.
2. **One ask at a time.** A holder answers one ask at a time. A second asker is not queued; it is told "{name} is already asking." with **Ask again**.
3. **Take over.** If nobody holds the role, Take works at once with no prompt. If the holder has been offline for the takeover time, another device may take it after confirming ("Mary's laptop has been offline for 5 minutes. Run the stream from here?"). The takeover time is set on each device in Setup > Devices (default two minutes). A claimant also waits a **10 second margin** on top (`TAKEOVER_MARGIN`): with the default it can take over from 2 minutes 10 seconds of silence. Until then the button explains how long is left. A short network drop never counts. A claimant also waits by the longer of its own takeover setting and the one the holder tells it (PR #160).
4. **What is on air.** Passing **Projection** clears the screens ("Allow and clear the screens"), and passing **Audio** stops what is playing ("Allow and stop the music"). Passing **Operator** leaves the slide up. Passing **Stream** leaves OBS on its current scene.
5. **A returning holder yields.** A device that comes back after losing a role learns it, drops the role and shows a notice ("Mary's laptop took over running the stream while this device was offline. This device no longer runs the stream."). It cannot win the role back by itself.
6. **A takeover the holder turns down is handed back.** If the holder had not really been away (the link failed in one direction only, or devices dropped at different times), it refuses the claim with a signed refusal (a `Reclaim` step, below). The claimant hands the role back at once, as a returning holder does, and shows "Sanctuary had not been offline long enough, so it kept running the stream. This device no longer runs the stream. You can ask Sanctuary for it." Any device that had taken the claim in comes back too. For **five minutes** after that (`REFUSAL_COOLDOWN`) the claimant cannot take the role over from that holder again, however long it seems gone; the button says "Sanctuary turned down this device's takeover. You can take over in 4 minutes." Asking still works, because the holder decides.
7. **Always allow** is offered only for Operator and Stream. It is not offered for Projection or Audio, because handing either over clears something on air. It is remembered per device, listed in Setup > Devices ("Always allowed", "Stop always allowing {name}").
8. **Roles are independent.** Handing Projection to another device does not move the Operator seat or Audio. The Operator seat is kept across a Projection hand-over, and the new Projection holder announces its state to the Operator.
9. **The Projection holder takes the Operator seat back whenever it likes,** without being allowed. The Operator sees "{name} took back running the service. This device no longer runs the service."
10. **"Hand the service back"** is the Operator's own button (it was "Stop driving" in 0.7.0). It gives the seat to the Projection holder, which takes it. It is silent: no notice on either side. If the Projection holder is not connected, the seat is simply let go and nobody holds Operator.

Who may hold a role is checked in one place, `Roles::claim` and `Roles::allow` (`check_teams`). Today it always says yes; Teams (T18) will plug in there.

### Signed role records

Each role is a record `{ holder, name, epoch }` with the last **eight** signed steps that led to it (a claim, a hand-over or a release, each signed by the device that did it). A device takes in a newer record only if its steps join the state it has and were signed by devices it knows, so a stranger cannot invent a hand-over. A record that cannot be joined (a device missed more than eight steps) is kept aside as **unverified** until a record that joins arrives, or until the device is paired again (unpair, pair). A device that lost its role file is taught again by the devices it is paired with.

**What the volunteer sees (#184).** A device in that state keeps what it had, and says so where it shows who runs what. In Setup > Devices > Who runs what, under the holder: "Can't check who runs the stream. This device missed too many changes while it was away, so who it shows may be out of date. To fix it, unpair one of the devices below and pair it again." The status bar adds "Can't check who runs the stream" to the roster, and that role's segment of the role bar is hatched.

**Who judges a takeover (protocol 9).** A claim is judged by the device it was taken from, because only that device knows whether it was really away. It accepts by signing an **ack** onto the claim, or refuses with a reclaim (below). Either way the verdict travels in the record, so every device reads it the same way, whichever device passed the record on. A device that sees the old holder connected does not take a claim against it until the holder has accepted it; it passes the record on to the holder and waits for the answer. A device that sees the old holder gone for the takeover time takes the claim, and the holder judges it if it comes back. So a device can't take a role from a holder others can see, however many steps it signs on top of its claim.

**A refused takeover (#183, protocol 8).** A step may also be a **reclaim**: the holder's signed refusal of a claim made against it. It is signed by the holder the claim was made against. It sits on that claim, or (protocol 9) on whatever the claimant built on it before the refusal arrived, such as a hand-over to another device. It gives the role back to that holder, past every step it cancels, so every device that took the claim in, the claimant, and anyone the role was passed on to come back to the holder; no record goes backwards. Only the holder the claim was made against can make one, and only on a claim it has not accepted. The cooldown is kept in memory (a restart ends it). This closes the gap issue #153 left (one-way link failure, devices that dropped at different times): the claimant learns as soon as the holder's record reaches it, which in the one-way case is when its connection is replaced (a silent session ends after 30 seconds) and the records are sent again.

**The role bar.** A thin strip of equal segments, one per role a machine holds, always Projection, Operator, Stream, Audio, in the colours above (`--role-*` in `app/src/lib/kit/theme.css`, lightened from the docs' colours to stand out on the app's dark panels). It is in the status bar for this machine and in Setup > Devices for this machine and each paired one, from what this machine has heard. A machine that runs nothing shows an empty grey strip. Its label is the roster's words ("Runs the screens and the service").

### Defaults, restarts, moving from 0.7.0

- **Pairing.** The device that shows the pairing code takes the roles nobody holds: Projection, Operator and Audio, each only if the rules allow. **A first device with no screens gets none of the three** (Audio goes only with Projection). Stream is left unheld, so Projection keeps the OBS duty until someone takes Stream.
- **Restarts.** Roles are kept in `roles.json` beside `paired.json`, device-local, and when each paired device was last heard is kept in `contact.json`. A restart, crash or sleep leaves a holder that others see as offline; the takeover time applies.
- **Moving from 0.7.0.** A device reads `presenter.json` and `driver.json` once into `roles.json` and leaves the old files. The lease becomes the Projection record with its epoch and takeover time. **Operator and Audio go to the Projection holder,** at the same epoch. The "always allowed to drive" list becomes Operator's. Stream is unheld. Sync protocol 9 (5 had the roles; 6 adds the takeover setting a device tells the others; 7 adds the frame in which the Audio holder says why a sound did not play; 8, in 0.9.0-preview.1, adds the reclaim step; 9 adds the ack on a claim); devices on an older version are refused with the usual message saying which side should update.
- **Roster.** The status bar shows what this device runs ("Runs the screens and the service") and who runs the rest ("Mary's laptop runs the stream"); a device with no role says "Planning". "Presenting" and "Driving" are gone. Setup > Devices has the "Who runs what" panel, with each role, who has it, a Take button, the always-allowed devices and the takeover time.

## Where OBS lives

- OBS is connected from the **Stream holder**. On a stream machine that is OBS on that machine, so the OBS password stays there.
- When an item goes live, Projection sends its scene as a cue (a scene name or nothing; no address, password or path) to the Stream holder. If the Stream holder is offline the newest cue waits and applies when it returns (a newer cue replaces an older one); Projection never switches OBS itself while Stream is held, so there are no double switches.
- With nobody holding Stream, Projection keeps the OBS duty, as it did in 0.7.0. A device with nothing paired drives its own OBS.
- The OBS drawer, scene follower, preview and mutes run on the Stream holder. On a device that does not own OBS, Present shows no OBS strip, and a scene or mute picked by hand says "OBS is run by the device that runs the stream." Setup > OBS works everywhere, so a machine can be set up before it takes Stream.
- A **stream desk** (a device that runs the stream, not the service or the screens, with other devices paired) shows OBS in Present even with no service open, and keeps the Present keyboard shortcuts off only on a stream desk with no service open, or inside the docked OBS panel (where Space and Enter belong to the focused OBS button).
- "Set up OBS" and "Use this address" (Setup > OBS) appear when OBS is on another computer. They only touch the "Lampstand" collection and the scenes and browser source it makes; they refuse to run while OBS is streaming or recording.
- Lampstand never edits OBS's settings otherwise (it can start OBS minimised). The one planned exception is "Control audio via OBS" on its own browser source (#137, above).

## Decided during the build

These were proposed in the first design and are now decided, or were decided by the owner while the roles were built:

- Projection holder retakes Operator unasked; "Hand the service back" is a silent hand-back.
- Moving from 0.7.0 gives Operator and Audio to the Projection holder; a first device with no screens gets none of the three.
- Roles are independent on a Projection hand-over.
- A claimant waits the takeover time plus 10 seconds.
- One ask at a time; always-allow only for Operator and Stream.
- Role records are signed, eight steps, with an "unverified" state, shown in the roster (#184).
- A takeover the holder refuses is answered with a signed reclaim; the claimant hands back and waits five minutes before it can take over from that holder again (#183).
- Every claim is judged by the device it was taken from; a device that sees that device connected leaves the claim to it (PR #205).
- Stream warnings: none. Audio warnings: "Nobody runs the sound, so {item} won't play." in the role bar ahead of the item (settled by the owner, #182), and "Nobody runs the sound." in the Sound box.
- **A music-only item starts from Next** (#181) when it plays on its own, on the device that runs the sound; other sound playing fades out over a second first. Waiting sound is unchanged.
- **The Sound box queue** (#191): played sound stays, greyed under "Played (n)" and still playable; a backing track keeps its own entry; "Play next" is a later ticket.
- Setup > Sound "Use sound in services" off by default.
- Set up OBS runs on the machine that serves the pictures, only for a remote OBS.
- Send sound to the stream is off by default (when it is built).
- The OBS audio exception is planned, not built.
- **Teams check.** Who may hold a role is decided at `Roles::claim` and `Roles::allow` and nowhere else (T18).
- **Sound.** Room sound (microphones, the band) stays outside Lampstand and outside the roles. NDI audio is a later ticket.
