# 0008. Presenter lease: one device drives the screens, claimed by asking

- Status: accepted
- Date: 2026-10-23

## Question

Sync is peer to peer and every device holds a full copy, but nothing says
which device is *the one that presents*. Two devices could open the same
service live and light the same screens, and a planner's laptop could take
the screens from the sanctuary PC by accident. How is "who may present"
defined, without making one device the master copy and without a server?

## Decision

Presenting is a **lease** held by one paired device, kept by every device:

```
Lease { holder: device key | none, name, epoch }
```

- **Only presenting is single-holder.** Documents still merge peer to peer
  as before. A device that does not hold the lease is *Planning*: it edits
  and syncs freely, but its live commands and "Open for live" are refused
  and its screens stay dark.
- **Leases merge as a maximum.** Ordered by `(epoch, holder key)`. Every
  claim or hand-over adds one to the epoch, so a lease never goes back to
  an older holder, and two devices claiming at the same moment agree on a
  winner (the higher key) with no further talk. The loser becomes Planning
  and is told.
- **Asking.** A Planning device sends `Ask` to the presenter, which shows
  "Mary's laptop wants to present. Allow / Not now". Allow writes a new
  lease naming the requester; Not now sends `Declined`. The asker sees
  waiting, then presenting or "not now". An ask is only possible while the
  presenter is connected.
- **Taking over without asking** is possible only when the presenter has
  been *offline* for the takeover time (default two minutes, set per
  device, Setup › Devices), or when nobody holds the lease (a library
  paired before leases, or the holder was unpaired). Offline means no
  connection: sessions send a heartbeat every 5 s and end after 30 s of
  silence, so a half-open socket is not mistaken for a live presenter, and
  the clock runs from the last frame heard. A brief blip is a reconnect
  within the backoff, well inside the time. The claiming device is told
  how long the presenter has been offline and asked to confirm.
- **A returning presenter yields.** It keeps believing it presents until
  it hears the newer lease; then it becomes Planning, takes its service off
  air and closes its screens, and shows a notice. It cannot win the role
  back by itself, because its lease is older.
- **A device with no paired devices is always its own presenter**, whatever
  an old lease says, so single-machine churches see nothing new.
- **First holder.** The device that showed the pairing code claims epoch 1
  when the pairing succeeds: it is the presenting machine.
- **What happens to what is on air.** Allowing hands over the screens, so
  the old presenter's service is taken off air as it becomes Planning. The
  prompt says so when something is on air ("Allow and clear the screens"):
  nothing leaves the air without the presenter pressing that button. The
  new presenter opens the service itself.

## Where the lease lives

On the sync wire, not in an Automerge document: frames `Lease`, `Ask`,
`Cancel`, `Declined` and `Beat` (protocol version 3), and a small
`presenter.json` beside `paired.json`, device-local. Reasons:

- Asks and heartbeats are messages, not state; they need a live
  connection. A document would still need the same frames beside it.
- The merge rule is explicit and tested (a max register), rather than
  whichever concurrent write an Automerge map happens to expose.
- It keeps the presenter out of the team document, which T18 will make
  signed. T18 can then gate *who may hold* the lease by team role at the two
  places that grant it, `Presenter::claim` and `Presenter::allow`, and move
  the record into the signed team document without changing the rules.

## Consequences

- Protocol 3: a device on version 2 is refused with a message saying which
  side should update.
- Honest-device model, like the rest of sync until T18: a modified client
  could send a lease with a high epoch. Signed membership and roles are what
  stop that.
- A partition leaves two presenters until it heals: the presenter on the
  smaller side is not told it was replaced until the devices meet again.
  That is the price of working offline. The takeover time keeps it rare,
  and the old presenter never resumes silently afterwards.
- The takeover time is per device. Each device decides how long it waits.
