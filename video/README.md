# Lampstand demo video

A 72-second, silent, 1920×1080 fly-over of the Lampstand interface, made with
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
