# 0005. Live stream: manage OBS from Lampstand, don't replace it

- Status: accepted
- Date: 2026-10-20

## Question

OBS combines the cameras, audio, scenes and our graphics into the feed
that goes to the stream. Should Lampstand build its own compositor and
encoder, or keep OBS and drive it?

## Decision

Keep OBS as the streaming engine, and manage it from Lampstand so a
volunteer never has to open OBS during a service. This keeps the PRD
non-goal ("Replacing OBS") and turns OB-4 to OB-6, plus the new OB-7 to
OB-9, into one piece of work:

1. **Graphics in:** the browser source (OB-1, OB-2; T43 built it, T86 wires it in).
2. **Control out:** a client for the OBS WebSocket (v5, OBS 28 and later) in
   `lampstand-connectors` (T88).
3. **Status in Present:** connected, streaming, recording, dropped frames, and a
   warning when OBS goes away. When OBS isn't connected, Lampstand says why
   (not running, not installed, or its WebSocket server is off) and offers to
   open it (OB-5, T89).
4. **Scenes from the service order and by hand:** a service item can name an OBS
   scene to switch to when it goes live (OB-4, T90), and Present shows OBS's
   scenes as buttons with mute toggles for its audio inputs (OB-8, T91).
5. **Stream and record from the service order** (OB-6, T92).
6. **One-click setup:** Lampstand creates a "Lampstand" scene collection in OBS,
   with starter scenes and the browser source already pointing at the stream
   output (OB-7, T93).
7. **Stream destination and virtual camera:** set where OBS streams (a listed
   service or a custom server, with its key) and turn the virtual camera on and
   off (OB-9, T94). OBS sends to one destination; several at once needs a
   plugin or a restream service.

## Why not our own compositor

A first version is one GStreamer pipeline: cameras and our offscreen-rendered
stream output into `compositor`, microphones into `audiomixer`, a hardware
H.264 encoder and `rtmp2sink`. That's about 4 to 6 weeks. This is a guess,
not a measurement.

Most of the cost comes after that: keeping audio and video in sync for each
source, level meters and monitoring, hardware encoders that behave
differently on each OS, capture-device quirks and permissions, reconnecting,
streaming to several platforms, and the larger GStreamer runtime. It would
also run on the laptop that already drives the projectors. OBS does all of
this, is free, and most streaming churches already use it.

## What would change this

Revisit when churches ask for single-app streaming on one laptop more than
they value OBS, or when a hosted product needs a stream pipeline on a server.
Then compare:

- **GStreamer pipeline.** It shares its frame path with NDI, Spout and Syphon
  (OB-3), so that work would be reused.
- **Embedding libobs.** It's GPL-2.0-or-later, so it can go in a GPL-3.0
  project, and Streamlabs Desktop did the same. It's a heavy C dependency, and
  the Rust bindings haven't been checked. Spike it before choosing.

## Constraints on the integration

- **Pin to WebSocket protocol v5.** If the connection breaks, graphics still
  reach the stream, because the browser source doesn't depend on the protocol
  (PRD risk table).
- **The OBS password is a secret for one device.** Keep it in device-local
  config, written with the T79 permissions (or the OS keychain), and never in a
  synced document.
- **Losing OBS must never block the service.** Projection keeps going, and
  Present only shows a warning.
- **Setup is additive.** It touches only the collection and sources it created,
  found by name, and never edits the church's own scenes.
