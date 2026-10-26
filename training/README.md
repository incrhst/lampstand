# Training services

Lampstand teaches the Present screen by having a volunteer run it (#190). Training Sessions 0, 1 and 2 are
services bundled with the app: they are in every install, work offline, are read-only in Plan, have
**Reset training**, stay out of CCLI reports and never sync. Present lists them under "Learn by
running it"; Plan lists them under Training.

| Service | For | Time | Ends with | Outline |
| --- | --- | --- | --- | --- |
| Training Session 0: Orientation | Everyone | 5 min | Plan vs Present, what Setup is for | [outlines/training-0.md](outlines/training-0.md) |
| Training Session 1: Run a service | Operators | 10 min | A practice service with no wrong slide and no desktop showing | [outlines/training-1.md](outlines/training-1.md) |
| Training Session 2: Confident operator | Operators | 30 min | Recovering from five surprises | [outlines/training-2.md](outlines/training-2.md) |

Training Sessions 3 to 5 (Planner, Setup and the stream, Admin) are for a later release.

## What is here

| Path | What |
| --- | --- |
| `services/training-N.json` | Each service's items and the trainer's notes on each item. The app reads these. |
| `decks/build_decks.py` | The lesson decks' content, slide by slide, with the trainer's script and the Level 2 surprises in the speaker notes. Writes `decks/*.pptx`. |
| `decks/*.pptx` | The decks the app embeds. Real PowerPoint files, imported by the same deck importer as a church's own, so they double as fidelity fixtures (click builds and Fade transitions included). |
| `videos/*.json` | The video scripts: scene lists for the parametric Remotion framework in `video/`, with the spoken words in each scene's `say`. |
| `media/*.mp4` | The rendered videos the app embeds (1280x720 H.264, mono AAC). |
| `cheat-sheet.html`, `cheat-sheet.pdf` | One page of keys and controls to print and keep by the projection computer. |
| `build.mjs` | Rebuilds the decks, the videos and the sheet from the above. |

Content is free of licence questions: the seeded public-domain hymns (Amazing Grace, Holy, Holy,
Holy), the bundled KJV and WEB, and our own words.

## Rebuild

```bash
cd video && npm install && cd ..        # once
node training/build.mjs                 # decks, key sheet and videos
node training/build.mjs decks           # only the decks (needs uv)
node training/build.mjs sheet           # only the key sheet (needs Chrome)
node training/build.mjs videos [name]   # narrate, render and compress (needs ffmpeg)
```

Videos are regenerated, never re-filmed:

1. **Screens.** `npm run capture:serve` then `npm run capture:training` in `video/` drive the real
   Svelte UI (with the Tauri bridge swapped for the scripted fake in `video/capture/mock-core.ts`)
   through Present and Plan on the training services, and write `video/public/shots/training/*.png`.
   Retake them when the UI changes.
2. **Narration.** Each scene's `say` is spoken by Google Cloud TTS, Chirp 3 HD voice Algenib
   (`video/core/scripts/narrate.mjs`, through the `narrate` skill's `narrate.py`; it needs Google
   Application Default Credentials). Only lines whose words changed are synthesised again. The mp3s
   are kept in `video/public/narration/<name>/` and each scene's length follows its narration. Pass
   `--no-narrate` to render with the narration already there.
3. **Picture.** `video/core/scripts/render.mjs` renders the scene list at 1920x1080, and the build
   compresses it into `media/<name>.mp4`.

`cargo test -p lampstand-app --test training` loads each training service and steps through every
slide and build with the live engine, and decodes each video with Lampstand's own decoder.

## How it is kept out of sync

Training services, their decks and their copies of the hymns are not documents. The app builds them
from these files at every start (`app/src-tauri/src/training.rs`) and gives them to the library's
training set (`crates/lampstand-library/src/training.rs`), which lookups by ID find but the repo
never holds. Nothing is written to the team index, so no peer is offered them and an older
Lampstand never hears of them: no new document kind, field or protocol version. The decks and
videos are blobs in this device's store, which peers only fetch for documents they hold.
