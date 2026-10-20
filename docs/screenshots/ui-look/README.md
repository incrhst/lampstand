# UI look: before and after

The Svelte app restyled after the reference React mock: its feel, layouts and
good ideas, not its code. Taken from a debug `.app` bundle with a fresh
`LAMPSTAND_DATA_DIR`, the English song database imported, one window output.

| Before | After |
| --- | --- |
| `before-plan.jpg` | `after-plan.jpg` |
| `before-operate.jpg` | `after-operate.jpg` |
| `before-outputs.jpg` | `after-outputs.jpg`, `after-calibrate.jpg` |
| | `after-calibrate-4x5.jpg`: the 4:5 screen preset on a 16:9 window |
| | `after-output-4x5.jpg`: the real output window with that safe area |

## Taken from the reference

- Palette (near-black blue-grey, gold primary with dark text, green ready,
  red live, amber warnings), type scale, 6 px radius, thin dividers.
- Shell: gold wordmark, Plan / Present tabs, "Live service: …" and Setup on
  the right, a status bar (connection left, background work middle, screens
  right, amber "not calibrated"). The connection and screens items open
  Setup › Devices and Setup › Screens. (Present is the operator view,
  `routes/operate/`.)
- Plan: three columns (services, order, library); numbered order rows with a
  drag handle, a type icon, title and subtitle; library tabs, a search field
  with an icon, "In service" ticks, lyric snippets.
- Present: a header bar with Safe area toggle and Show / Logo / Blank; the
  running order on the left with the live item in gold; a LIVE pane with a
  red ring and a dashed safe-area guide; slide cards with a LIVE badge;
  Next, notes and screens on the right.
- Setup › Screens (`routes/outputs/`): a page of screens, each with a safe-area preview, and a per-screen
  calibration view (numbered grid, edge numbers around a mini screen).
- Kit pieces for the setup wizard (T55): step rail, progress bar, segmented
  decision rows.

## Placeholders and what backs them

Shown as a dashed `Placeholder` pill naming the ticket, or a disabled control
with the reason as its tooltip:

| Spot | Ticket |
| --- | --- |
| Readiness banner in Plan and the Readiness pill in Present | T36 |
| Sync status in the status bar ("This device only") | T18 |
| Slide thumbnails (slides are drawn as text) | T47 |
| The calibration grid on the projector itself (it shows in the app only) | T26 |
| Library tabs Scripture and Media | T48 |
| Library tab Decks, Add deck | T28 |
| Setup wizard screen (the kit has its step rail and progress bar) | T55 |

Left out: service dates and times (`starts_at` cannot be set yet; rows say
"No date set"), per-item "On this device", song authors in search results
(`SongHit` has none, so duplicate titles look alike), speaker notes (planner
notes show instead), video controls, the fidelity report.

"Not calibrated" is real but approximate: an output with no safe-area margins
counts as not calibrated, since outputs keep no calibration date.
