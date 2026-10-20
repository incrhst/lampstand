# Library panel

The planner's library after the simplified-library redesign: one search
across songs, scripture, decks and media, with filter chips, recently used,
drag into the order, and file drops.

These were taken in a browser against the Vite dev server with the Tauri IPC
stubbed by sample data (the shell's screen capture was not available), so
the library contents are made up and the status bar shows a leftover error
from the page's first load without Tauri. The panel itself is the real code.

| File | Shows |
| --- | --- |
| `search-in-service.jpg` | A search, chip counts, "In service" and "Add again" |
| `lyric-snippets.jpg` | Lyric hits with the matching line |
| `reference.jpg` | "Ps 23" read as a passage, with the translation choice |
| `drag-into-order.jpg` | A row carried into the order, with the drop line |
| `drop-overlay.jpg` | Files dragged over the panel |
| `drop-summary.jpg` | After the drop: "Added 3 songs · 1 Bible · 1 deck · 1 media file" |
| `empty.jpg` | An empty library with no service picked |
