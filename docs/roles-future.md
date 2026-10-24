# Roles: ideas for later

Status: not part of 0.8.0. These came up while designing [roles](roles.md) ([decision 0010](decisions/0010-roles-and-handoff.md)) and were set aside on purpose, not rejected. Each says what it would add and what it would have to settle first.

## More roles

- **Stage display (a native stage machine).** A second Lampstand machine wired to a confidence or stage screen, drawing the stage layout with the same fonts and layout as the main screens. It would not be single-holder, since a church can have several stage screens: it would be "this device shows this stage output", assigned per screen. Until then a confidence screen is a second display on the Projection machine or a browser page on a tablet, which is enough for most churches.
- **Planner.** A device that is planning the next service rather than running this one. Every device can plan today with no role, so this only matters if a church wants to see who is planning, or to stop two people editing the same item.
- **Video or camera switcher.** Switching cameras and mixing video, kept in OBS for now. A role would only make sense if Lampstand took on the camera side, which decision 0005 leaves to OBS.

## People as well as devices

Roles belong to devices in 0.8.0. When Teams (T18) gives a device an owner, the roster can show the person ("Mary is running the stream") and a hand-over prompt can name them. The role record does not change. What T18 adds is who may hold each role, checked in one place (`Roles::claim` and `Roles::allow`).

## Phones and tablets as more than helpers

- **A Stream view in the browser.** Program preview and scene buttons on a phone, for an operator who walks the room. It needs its own security decision, because it would reach the OBS connection and the plain-HTTP limit in [decision 0006](decisions/0006-tablet-remote-in-the-browser.md) allows only narrow powers.
- **A browser stage page** (the planned stage display) is built apart from the roles work.

## Sound

- **Count-in and click.** A count-in before a backing track, and a click on a separate output for the band.
- **Key and tempo changes** for backing tracks.
- **NDI audio,** sending Lampstand's sound to OBS or another machine over the network instead of the browser-source route.
- **A mixer-style view,** if churches ask for levels in Lampstand. Today OBS or the church's mixer owns every level ([decision 0005](decisions/0005-manage-obs-not-replace.md)).

## Moving roles automatically

- **Role presets,** such as "Sunday set-up", so a church that always runs three machines does not ask for each role each week.
- **A hand-over note** shown to the new holder (what is on air, what is next) when a role changes hands mid-service.
- **Several holders for a role** (for example two Operators), if the one-at-a-time rule proves too strict. The rule is in one place, as it was for driving in [decision 0009](decisions/0009-driving-another-device.md).
