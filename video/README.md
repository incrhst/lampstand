# Lampstand demo video

A 72-second, 1920×1080 (background music and sound effects, no voice) fly-over of the Lampstand interface, made with
[Remotion](https://www.remotion.dev) (React → MP4). The camera glides across
tilted window planes in 3D; captions are short and plain ("animation steps",
not "builds"). Colours and type come from `site/style.css`; the fonts are the
site's own (`public/fonts`, SIL OFL).

## Render

```bash
cd video
npm install
npm run render        # writes out/lampstand-demo.mp4
npm run studio        # live preview and scrubbing
```

`out/` is not committed.

## Sound

The background music is `public/music/lampstand-intro.m4a` (supplied by the project owner; only the first 72 seconds play, faded in and out).

The effects (camera whooshes, caption stamps, key clicks, an end bell) are
synthesised from noise and sine waves by `audio/make-sfx.mjs`, so nothing is
licensed. `npm run sfx` rewrites `public/sfx/*.wav`; the cues are in
`src/timeline.ts` (`cues`).

## What is real and what is staged

- **The window planes are the real Svelte UI** (`app/src`), run in a browser
  with the Tauri bridge swapped for a scripted fake (`capture/mock-*.ts`).
  The app itself is not modified, and nothing reads a real data folder.
- **The projector plane is drawn for the video**, not captured: the real
  output is rendered natively by the wgpu renderer, which a browser can't show.
  It uses the site's screen colours.
- The sample songs, verses and decks are made up for the demo (public-domain
  hymns, KJV John 3:16).

## Retake the screenshots

```bash
npm run capture:serve   # terminal 1: the UI with the fake bridge, port 1431
npm run capture         # terminal 2: writes public/shots/*.png
```

`capture/shoot.mjs` drives Chrome through each moment (Get started strip, Plan,
quick add, decks, Present with Shift+Down and Enter, OBS, Setup › Screens).
Set `CHROME=/path/to/chrome` if Chrome isn't in the usual macOS place.

## Change the film

Everything is data in `src/timeline.ts`: where each plane sits, where the camera
rests (`cams`), and each caption's words and timing (`captions`). The 3D is plain
CSS transforms in `src/Demo.tsx`.

No claims about downloads, users or dates appear in the video, in line with
`PRODUCT.md`.

## Explainer videos from scene lists (parametric)

Besides the fly-over above, `video/core` (shared scenes, motion, validation) and `video/projects/lampstand`
(theme, roles, devices, screenshot callouts) render short explainer videos from a JSON scene list.
Read `projects/lampstand/MANIFEST.md`, write `projects/lampstand/videos/<name>.json`, then:

```bash
npm run lamp:validate
npm run lamp:sheet -- projects/lampstand/videos/<name>.json --sheet    # contact sheet in out/
npm run lamp:render -- projects/lampstand/videos/<name>.json           # out/<name>.mp4 (add --aspect 9:16 or 1:1)
```
