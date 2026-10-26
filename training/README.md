# Training services

Lampstand teaches the Present screen by having a volunteer run it (#190). Training 0, 1 and 2 are
services bundled with the app: they are in every install, are read-only in Plan, have **Reset
training**, stay out of CCLI reports and never sync. Present lists them under "Learn by running it";
Plan lists them under Training.

Their decks, songs and scripture are bundled and work offline. The training videos are not in the
installer: they are on the website and download the first time a training service is opened (see
[Training videos](#training-videos)), so a training video can change without a release.

| Service | For | Time | Ends with | Outline |
| --- | --- | --- | --- | --- |
| Training 0: Orientation | Everyone | 5 min | Plan vs Present, what Setup is for | [outlines/training-0.md](outlines/training-0.md) |
| Training 1: Run a service | Operators | 10 min | A practice service with no wrong slide and no desktop showing | [outlines/training-1.md](outlines/training-1.md) |
| Training 2: Confident operator | Operators | 30 min | Recovering from five surprises | [outlines/training-2.md](outlines/training-2.md) |

Training 3 to 5 (Planner, Setup and the stream, Admin) are for a later release.

## What is here

| Path | What |
| --- | --- |
| `services/training-N.json` | Each service's items and the trainer's notes on each item. The app reads these. |
| `decks/build_decks.py` | The lesson decks' content, slide by slide, with the trainer's script and the Level 2 surprises in the speaker notes. Writes `decks/*.pptx`. |
| `decks/*.pptx` | The decks the app embeds. Real PowerPoint files, imported by the same deck importer as a church's own, so they double as fidelity fixtures (click builds and Fade transitions included). |
| `videos/*.json` | The video scripts: scene lists for the parametric Remotion framework in `video/`, with the spoken words in each scene's `say`. |
| `media/*.mp4` | The rendered training videos (1280x720 H.264, mono AAC). The source the publish step copies to the website; the app does not embed them. |
| `../site/training/` | The published training videos and their manifest, `videos.json`, served at https://lampstand.dbain.com/training/. Written by `node training/build.mjs publish`; never edit by hand. |
| `cheat-sheet.html`, `cheat-sheet.pdf` | One page of keys and controls to print and keep by the projection computer. |
| `build.mjs` | Rebuilds the decks, the videos and the sheet from the above, and publishes the videos. |

Content is free of licence questions: the seeded public-domain hymns (Amazing Grace, Holy, Holy,
Holy), the bundled KJV and WEB, and our own words.

## Rebuild

```bash
cd video && npm install && cd ..        # once
node training/build.mjs                 # decks, key sheet, videos, then publish
node training/build.mjs decks           # only the decks (needs uv)
node training/build.mjs sheet           # only the key sheet (needs Chrome)
node training/build.mjs videos [name]   # narrate, render and compress (needs ffmpeg)
node training/build.mjs publish         # copy the videos to site/training/ with the manifest
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
slide and build with the live engine, with the training videos served from `site/training/` and with
no network at all, and decodes each published video with Lampstand's own decoder. It fails if
`site/training/videos.json` does not match the files beside it.

## Training videos

The website publishes the training videos. `site/training/` holds them and a manifest,
`videos.json`:

```json
{
  "version": 1,
  "videos": [
    { "id": "t1-welcome", "file": "t1-welcome.mp4", "size": 691112, "blake3": "8929…ec85" }
  ]
}
```

`blake3` is the file's BLAKE3 hash, which is also its blob hash in Lampstand, so a training service's
video item names its video before it has arrived. The Pages workflow deploys `site/` to
https://lampstand.dbain.com on every push to `main` that touches it, so the manifest is at
https://lampstand.dbain.com/training/videos.json and each video beside it.

In the app (`app/src-tauri/src/training_videos.rs`):

- Opening a training service that has videos (in Plan or Present), or Reset training, starts a
  download on its own thread when a video is missing or the manifest was last fetched more than a
  day ago (Reset training always fetches it). Nothing downloads at start or for any other service.
- Each video is streamed to a temporary file in the data folder's `training-videos/`, refused unless
  its size and hash match the manifest (the temporary file is removed either way), then put in the
  blob store, which renames it into place. The live engine and output windows play it from there
  like any other video.
- The manifest last fetched is saved in `training-videos/videos.json`, so videos already downloaded
  keep working offline. Before the website has been reached, the app uses the manifest it was built
  with.
- Present shows the download's progress. Until a video has arrived, its item shows "The training
  videos download the first time. Connect to the internet to watch them." in its place, and the
  rest of the training service (decks, songs, scripture) runs as usual.
- A release may name a training video the website has not published yet. That is never an error:
  the item names a placeholder until a manifest lists the file. Once the website has answered and
  its manifest does not list the video, the item shows "This training video isn't ready yet. Check
  back soon." (offline, it shows the message above). When a later manifest lists it, it downloads
  and plays with no app update.

### Update a training video without a release

1. Change its script in `videos/<name>.json` and render it: `node training/build.mjs videos <name>`.
2. Publish it: `node training/build.mjs publish`. This copies `media/*.mp4` into `site/training/`
   (removing any that are gone) and writes `site/training/videos.json`.
3. Commit `training/` and `site/training/` together, open a PR and merge it. CI checks that the
   manifest matches the files and that each video decodes.

The website has the new video a few minutes after the merge. An installed copy fetches it the next
time a training service is opened after a day has passed since its last check, or at once after
Reset training. A new item in `services/*.json` needs a release, but its video need not be ready
for it: ship the item, and publish its video whenever it is done.

## How it is kept out of sync

Training services, their decks and their copies of the hymns are not documents. The app builds them
from these files at every start (`app/src-tauri/src/training.rs`) and gives them to the library's
training set (`crates/lampstand-library/src/training.rs`), which lookups by ID find but the repo
never holds. Nothing is written to the team index, so no peer is offered them and an older
Lampstand never hears of them: no new document kind, field or protocol version. The decks and
training videos are blobs in this device's store, which peers only fetch for documents they hold,
so each device downloads its own training videos from the website.
