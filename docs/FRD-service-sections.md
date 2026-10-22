# Functional requirements: service list, item cards and looping sections

Status: draft. Companion to the [PRD](PRD.md) (planning PL, live operation LV, PowerPoint PS) and the [plan contract](plan-contract.md). The interactive mockups are in the [Plan view item cards](https://claude.ai/artifact/TBjFWUuHhUXmakfWHn6L1Q) canvas.

## 1. Purpose

Planning a service has three pain points today:

1. The Services list cannot rename, delete or take a service off air, and old services crowd out the ones that matter.
2. Each item in the service order is a dense row. Song order, OBS scene and loop settings all show at once, and most items use none of them.
3. A header only divides the order. There is no clear sense of what belongs to it, and no obvious way to make a section loop, for example slides that play while people arrive.

This document specifies the changes. Requirements use the PRD's table style. P0 is the release this work ships in, P1 follows, P2 is later.

## 2. Terms

| Term | Meaning |
| --- | --- |
| Item | One entry in a service's order: a song, scripture, deck, media file, countdown, announcements or header. |
| Header | An item of kind `Header`. It has a title and no slides of its own. |
| Section | A header and every item after it, up to the next header. Items before the first header are in no section. |
| Loop | A section or item that repeats while it is on air. |
| Interval | How long each still slide stays on screen in a loop. |
| Pass | One run through a looping section from its first item to its last. |
| Natural timing | A length the content carries: a video's duration, or an animation that starts by itself. |
| Operator | The person running the service from the Present tab. |

## 3. Scope

In scope: the Services list, the Plan view's service order, section loops, and the loop controls in the Present tab.

Out of scope: nested sections, per-slide timing editors, scheduling a service to start by itself, and the collaborative planner (T49).

## 4. Services list

Shipped in 0.4.0; listed here so the document is complete.

| ID | Requirement | Priority |
| --- | --- | --- |
| SL-1 | The selected service's row offers Rename, Delete and, if it is open for live, Off air. | P0 |
| SL-2 | Rename is in place: Enter saves, Esc cancels, a blank name is refused. A rename of the open service reaches the operator view at once. | P0 |
| SL-3 | Delete needs a second click ("Delete?") and is permanent. Deleting the open service takes it off air first. | P0 |
| SL-4 | Off air clears the screen and unloads the service, so an update may install. The service itself is kept. | P0 |
| SL-5 | Services that started before today are hidden. A "Show N older" toggle reveals them, and the selected service is always shown. | P0 |
| SL-6 | The older-services cutoff is configurable, and old services can be archived instead of deleted. | P2 |

## 5. Item cards

| ID | Requirement | Priority |
| --- | --- | --- |
| IC-1 | Each item shows as a card: grip, thumbnail, title, subtitle. It does not show controls the item has not used. | P0 |
| IC-2 | A setting that has a value shows as a small tag on the card: "Custom order", "Loops 8 s", "OBS: Slides only". An unset setting shows nothing. | P0 |
| IC-3 | Clicking a card opens its options below it. One card is open at a time. The chevron has an accessible name, and `aria-expanded` is set. | P0 |
| IC-4 | A song's options hold its sequence for this service (section chips, "+ Add", "Use song's own order"). Every item's options hold its loop interval and OBS scene (the OBS control stays hidden where OBS is not in use) and Remove. | P0 |
| IC-5 | Cards reorder by drag on the grip, or by the up and down keys on the grip, as rows do now. | P0 |
| IC-6 | Titles wrap or truncate and never push the other columns out of the card. A long song author does not break the layout. | P0 |

## 6. Headers and sections

### 6.1 Definition

A section is a header and the items after it, up to the next header. Nothing is stored beyond the header and the item order. There is no indent flag.

| ID | Requirement | Priority |
| --- | --- | --- |
| HS-1 | A section is defined as above. Items before the first header belong to no section. | P0 |
| HS-2 | Items in a section are drawn inset on a rail, under a header that shows its item count ("3 items"). | P0 |
| HS-3 | A header is drawn as a slim label, not a card, and has Remove and a grip. | P0 |
| HS-4 | Removing a header removes only the header. Its items stay and join the previous section, or no section if there is none. | P0 |
| HS-5 | Existing services need no migration, because the rule is the rule they already followed. | P0 |

### 6.2 Creating a header

| ID | Requirement | Priority |
| --- | --- | --- |
| HC-1 | Hovering the gap between two cards, above the first card, or below the last, shows a gold line with "+ Header here". The control is also reachable by keyboard focus. | P0 |
| HC-2 | A toolbar "Add header" button starts the same flow at the end of the order. | P0 |
| HC-3 | The flow opens a header row in place with an autofocused name field. Enter adds, Esc cancels. | P0 |
| HC-4 | Common names are offered as one-tap chips: Pre-service, Greeting, Announcements, Praise and Worship, Communion, Offering, Sermon, Post-service. A chip fills the field, which stays editable. | P0 |
| HC-5 | While naming, the cards that will fall under the new header are highlighted and counted ("The 2 cards highlighted below will sit under this header"). A header at the very end says it starts empty. | P0 |
| HC-6 | A blank name is refused. | P0 |
| HC-7 | The list of common names is editable in Setup, per church. | P2 |

### 6.3 Moving a header

| ID | Requirement | Priority |
| --- | --- | --- |
| HM-1 | Dragging a header's grip turns every gap into a "Drop here" target. | P0 |
| HM-2 | Over a gap, the guidance line says where the section will start and how many cards it will cover. | P0 |
| HM-3 | Only the boundary moves. The cards stay where they are, so the sections on either side change size. Dragging a card still moves only that card. | P0 |
| HM-4 | Releasing outside a gap cancels the move. | P0 |
| HM-5 | A header can be moved by keyboard: up and down on its grip move it one gap. | P1 |

## 7. Looping sections

| ID | Requirement | Priority |
| --- | --- | --- |
| LS-1 | Every header has a "Loop section" control. It is quiet until used, and becomes a gold "Loops · 8 s" tag once set. | P0 |
| LS-2 | The control opens a panel with an interval of Off, 5, 8, 12 or 20 s, and a strip showing the section's items in order with "back to the start". | P0 |
| LS-3 | Naming a header Pre-service or Post-service opens the loop panel straight away, because those sections nearly always loop. Other headers do not. | P0 |
| LS-4 | A looping section's cards show a gold rail. | P0 |
| LS-5 | A looping section that has no items says so and offers the library. | P0 |
| LS-6 | An individual item can still loop on its own (the existing item-level loop). If both apply, the section's loop repeats the item's own once per pass. | P1 |
| LS-7 | A custom interval, in seconds, can be typed. | P1 |
| LS-8 | The library's Add puts items at the end of the selected section. With no section selected, at the end of the service. | P0 |

### 7.1 Timing rules

The interval is a default for still slides. Content with its own timing keeps it.

| ID | Requirement | Priority |
| --- | --- | --- |
| LT-1 | An image or an announcement slide stays up for the interval. | P0 |
| LT-2 | A video plays through once per pass and ignores the interval. | P0 |
| LT-3 | A video that is set to loop by itself would hold the section forever. The loop panel flags it and offers "Play it once instead". | P0 |
| LT-4 | A deck slide with no animation stays up for the interval. | P0 |
| LT-5 | A deck slide with animations that start by themselves plays them on arrival. The interval counts from when they finish, so a slide never cuts away mid-animation. | P0 |
| LT-6 | Click-only builds are skipped to their finished state in an unattended loop. The slide shows fully built, then waits for the interval. | P0 |
| LT-7 | A deck item may override this: "Hold each step for N s" reveals click-only builds one step at a time at that rate. | P2 |
| LT-8 | The loop panel says which slides are affected by LT-6 ("2 slides have click-only animations, shown fully built"). | P0 |
| LT-9 | A transition (cut, crossfade, slide) happens inside the interval and adds nothing to it. The interval means start to start. | P0 |
| LT-10 | The loop panel lets an image slideshow choose its transition and its order (as listed or shuffled). Shuffling does not change the length. | P1 |

### 7.2 Length estimate

| ID | Requirement | Priority |
| --- | --- | --- |
| LE-1 | The loop panel shows the length of one pass, with a line per item: "6 slides × 8 s = 0:48", "Welcome video 0:45". | P0 |
| LE-2 | Estimate rules: image slides and deck slides are N × interval; automatic animations add their own length; videos add their duration; click-only builds add nothing. | P0 |
| LE-3 | When any component has no fixed length (a countdown, a live source), the total reads "At least 1:27" and names that component. | P0 |
| LE-4 | The estimate updates as items or the interval change, and when files are added to or removed from a folder slideshow. | P0 |
| LE-5 | A video's duration is read from the file when it is added and stored with the media record, so the estimate needs no playback. An older file with no stored duration is read the first time it is needed. | P0 |
| LE-6 | If the estimate is for the pre-service section and the service has a start time, the panel shows whether whole passes fit before it ("3 passes finish at 9:58"). | P1 |

## 8. Presenting

| ID | Requirement | Priority |
| --- | --- | --- |
| PR-1 | The Present tab lists the service's sections, marks the one on air, and shows its loop state ("Looping · 8 s per slide"). | P0 |
| PR-2 | The loop controls are available on air: a switch (loop on or off) and the interval choices. They need no trip back to Plan. | P0 |
| PR-3 | A change applies from the next slide. The slide on screen is not cut short. | P0 |
| PR-4 | A live change belongs to this service's run. It is not written back to the saved section, and the panel says so. Changing it for every service is done in Plan. | P0 |
| PR-5 | With the loop off, the section stops on its last slide. A video in progress plays to the end. | P0 |
| PR-6 | The operator sees a progress bar for the slide on screen, and the same estimate as Plan ("Pass 2, 0:31 left"). | P1 |
| PR-7 | Controls cover the section on air. Other sections' loops are changed in Plan or when they come on air. | P0 |
| PR-8 | Previous slide and Next slide work while a loop is running, and restart the interval. | P0 |
| PR-9 | Starting the service ends the loop. The first item after the section goes live as normal. | P0 |

## 9. Data and compatibility

- **Header and section:** no new fields. A section is derived (HS-1).
- **Loop:** each item's existing `loop_seconds` is the section's interval when set on a header (already documented that way in `lampstand-core`). Live changes (PR-2) live in the live engine's state for this run only.
- **Media duration (LE-5):** one new optional field on the media record. Existing records read as unknown and are filled in lazily.
- **Moving a header (HM-3):** changes only the header's position, so it merges as an ordinary move in the sync layer.
- **Version:** items saved before any of this existed load unchanged.

## 10. Non-functional

| ID | Requirement |
| --- | --- |
| NF-1 | Every control is a real button or input with an accessible name. Hover-only controls (HC-1) are also reachable by keyboard focus. |
| NF-2 | Text meets 4.5:1 contrast on the dark theme. |
| NF-3 | A service of 200 items renders and drags without visible lag. |
| NF-4 | The loop never skips or repeats a slide because of a live interval change. |
| NF-5 | An unattended loop that runs for hours does not grow in memory or fall behind its interval. |

## 11. Acceptance

1. Create a header named Pre-service above the first card. The loop panel opens. Set 8 s. The header shows "Loops · 8 s", its cards show a gold rail, and the panel shows a length for one pass.
2. Add a Greeting header after the second card. The Pre-service section now covers two cards, and its estimate drops accordingly.
3. Drag Greeting down one card. The pre-service section grows by one card, and the estimate rises.
4. Put a self-looping video in the section. The panel warns and offers to play it once.
5. Open the service for live. The section loops. Change 8 s to 12 s on air. The slide on screen finishes its 8 s, and the next one holds for 12.
6. Turn the loop off. The section stops on its last slide.
7. Reopen the service in Plan. The saved interval is still 8 s.

## 12. Open questions

1. Should a looped video be blocked inside a looping section, or only warned about (LT-3)?
2. Is the loop-on-air change worth saving back to the service on request ("Keep for every service")?
3. For a deck with click-only builds, is "skip to final state" (LT-6) right for churches that design builds to run unattended, or should automatic mode be the default for decks that have timings set?
4. Do common header names need to follow the church's language, as scripture translations do?
5. Does a section's loop stop at the end of a pass when the service start time arrives, or at once?
