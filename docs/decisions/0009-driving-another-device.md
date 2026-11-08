# 0009. Driving: operate the presenting device from another one, over the sync session

- Status: accepted
- Date: 2026-10-23

## Question

One paired device holds the presenter lease ([0008](0008-presenter-lease.md))
and owns the screens, the renderer, OBS control and the media. A volunteer
with a laptop at the back of the room should still be able to run the
service: see what is on air, press Next, jump to a chorus, take an item
live. How do commands reach the presenting device, and who is allowed to
send them?

## Decision

A second, lighter idea beside the lease: the **driver**. While one device
presents, one other paired device at a time may drive it. Nothing about who
presents changes, and nothing is rendered on the driver: its screens stay
dark, as a Planning device's do.

- **Asking and allowing.** A paired device presses *Drive from this device*;
  the presenter shows "Mary's laptop wants to drive. Allow / Always allow
  this device / Not now". *Always allow* is remembered per device
  (`driver.json`, device-only) and answers later asks at once. It can be
  switched off per device in Setup › Devices.
- **One driver at a time.** A second asker is told who is driving and is not
  queued. Even an always-allowed device does not displace the driver.
- **The presenter decides, always.** It can *Take back control* at once. Its
  own keyboard and clicker never pass through any of this, so they work
  whether or not someone is driving.
- **Commands are the operator's own requests.** The driver's app sends the
  same JSON `Request` its screen would have run locally, limited to what an
  operator does on air: every `Live` command, opening and closing the
  service on air, a loop change on air and OBS scene and mute choices. The
  presenter applies them on its app thread exactly as it applies a key
  press. Editing a service, settings and screens are not driveable; those
  stay on the device they belong to (and services sync anyway).
- **State comes back as the events the local screen already uses.** The
  presenter mirrors `Event::Live`, `Event::Loaded` and `Event::Obs` to the
  driver, and sends all three when a driver starts. The driver's Present
  screen is the ordinary one, fed by the presenter's events instead of its
  own engine, so there is no second implementation of the operator view.
- **Only the active driver counts.** The presenter ignores a command unless
  the sender is the active driver and this device still holds the lease.
  Frames travel inside the encrypted sync session between devices that
  proved their device keys at the handshake, so "who sent it" is known, not
  claimed.
- **Failure is quiet.** If the presenter goes offline the driver's controls
  are disabled and its bar says so; if the driver goes offline the presenter
  keeps what is on air and simply regains control. A new presenter starts
  with nobody driving.

## Transport: the sync session, not `/remote`

The tablet and phone remote ([0006](0006-tablet-remote-in-the-browser.md))
is a web page on the local server for devices that cannot run Lampstand,
paired with a PIN over plain HTTP. A second Lampstand does not have those
limits, and already has a connection to the presenter that is authenticated
by device keys, encrypted and kept alive with a heartbeat, which also tells
us when the other side is gone. Driving rides on that connection as six
frames (`DriveAsk`, `DriveCancel`, `DriveAnswer`, `DriveState`,
`DriveCommand`, `DriveLive`; sync protocol 4). `lampstand-sync` carries the
payloads as opaque bytes and applies the rules (who may drive, who is asked,
who is remembered); the app decides what they say. The PIN remote is
unchanged and can be used in the same service.

A command crosses to the presenter and the new state comes back in about
20 ms on one machine (`app/src-tauri/tests/sync.rs`), well inside the 50 ms
budget for the click itself; on a LAN the difference is the network.

## Not decided here

- A driver without Lampstand installed: that is the tablet remote.
- Several drivers at once. One at a time keeps two people from pressing
  Next together; if churches ask for more, the rule is in one place
  (`driver::Seat`).
- Whether a driver may edit the service on the presenter live. Edits sync
  as they always do and the presenter reloads the open service without
  changing what is on air.
