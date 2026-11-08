# Lampstand videos

Everything here is made with [Remotion](https://www.remotion.dev) (React to MP4) on one parametric system:
`core/` (shared scenes, motion, duration, validation, rendering) and `projects/lampstand/` (Lampstand's
design tokens, scenes and scene lists). Every video is a JSON scene list in `projects/lampstand/videos/`.

- **The site's hero tour** (`site/media/lampstand-tour.mp4`, 72 s, 1920x1080, music and sound effects, no
  voice): `videos/hero-tour.json`. A camera glides across tilted window planes in 3D; captions are short and
  plain ("animation steps", not "builds").
- **Explainers and training** (`videos/*.json`, `../training/videos/*.json`): scene lists of titles,
  screenshots with callouts, roles, devices and so on, optionally narrated.

## Render the hero

```bash
cd video
npm install
npm run hero          # checks tokens, validates the scene list, writes out/hero-tour.mp4
npm run hero:sheet    # one still per scene, out/hero-tour-sheet.png
npm run studio        # live preview and scrubbing (composition "Tour")
```

The render is a high-quality master (H.264 CRF 18, about 95 MB). The site's copy
(`site/media/lampstand-tour.mp4`, about 8 MB) is a 720p re-encode of it:
`ffmpeg -i out/hero-tour.mp4 -vf scale=1280:720:flags=lanczos -c:v libx264 -crf 28 -preset slow -pix_fmt yuv420p -movflags +faststart -c:a aac -b:a 96k site/media/lampstand-tour.mp4`.
The poster (`site/media/tour-poster.jpg`) is the frame where the camera reaches the wall, before its caption:
`ffmpeg -ss 26.45 -i out/hero-tour.mp4 -frames:v 1 -vf scale=1280:720 -q:v 3 site/media/tour-poster.jpg`.
Update the size in `site/index.html` if it changes. `out/` is not committed.

## Design system

`projects/lampstand/tokens.ts` holds every value a video uses, and nothing else hardcodes one:

- **Colours:** the site palette (`site/style.css`, DESIGN.md: goldenrod paper, toner ink, stamp violet,
  the screen navy), the app's own palette (`app/src/lib/kit/theme.css`), and the role colours in both
  forms: `roles.doc` for paper (Projection blue, Operator green, Stream purple, Audio orange, as in
  `docs/diagrams`) and `roles.app` for the app's dark panels. `npm run lamp:tokens` fails if `app` drifts
  from theme.css.
- **Type:** Old Standard TT and Courier Prime (the site's fonts, SIL OFL, in `public/fonts`), sizes,
  tracking and leading.
- **Spacing:** margins, the caption card, key caps, radii.
- **Motion:** the explainer spring and stagger, the app's ease, the tour camera (rest and drift), caption
  and key timings, film fades.

`theme.ts` (explainers) and `tour/` (the hero) are both built from it.

## The hero's scene list

`videos/hero-tour.json` has `"composition": "Tour"`, a `world` and `scenes`:

- `world.windows`: each window plane, a screenshot from `public/shots` placed in the world (`x`, `y`, `z`,
  `yaw`, `scale`) with its title bar.
- `world.screen`: the projector the congregation sees, drawn with lyrics (not captured: the real output is
  the native wgpu renderer). `world.backdrop`: the paper table with its folds and big typed labels.
- `scenes`: one caption each. `open` (big, lower left), `stop` (lower left) or `close` (centred end card);
  `tag`, `title`, `sub`, `keys` (key caps pressed in turn); `stops`, the camera positions it holds, each `on`
  a window (offsets from its centre) with distance `d`, `yaw`, `pitch`, `roll`, and `atSec` for a second
  stop; `captionIn`/`captionOut` and `sfx` for timing.

The parts (`tour/parts.tsx`: backdrop, window plane, screen plane, caption card, key caps) and the
timeline (`tour/timeline.ts`: camera path, caption windows, sound cues) are reusable; adding a window or a
caption is a JSON edit. `npm run hero:validate` checks ids, stop order and that every file exists.

## Sound

The background music is `public/music/lampstand-intro.m4a` (supplied by the project owner; only the first 91 seconds play, faded in and out).

The effects (camera whooshes, caption stamps, key clicks, an end bell) are
synthesised from noise and sine waves by `audio/make-sfx.mjs`, so nothing is
licensed. `npm run sfx` rewrites `public/sfx/*.wav`. The cues follow the scene list
(`tour/timeline.ts`): a whoosh mid-glide, a stamp per caption, a click per key, and a scene's own `sfx`.

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
npm run capture:remote  # the phone shot, public/shots/9-remote.png (no capture server needed)
```

`capture/shoot.mjs` drives Chrome through each moment (Get started strip, Plan,
quick add, decks, the Present wall, the builds panel, Focus, the Sound box open
and folded, OBS, Setup › Screens). `capture/remote.mjs` pairs Lampstand Remote at
phone size with the server's stand-in (`cargo run -p lampstand-server --example
remote_fixture`); set `LAMPSTAND_REMOTE_FIXTURE` to a built fixture to skip the build.
Set `CHROME=/path/to/chrome` if Chrome isn't in the usual macOS place.

No claims about downloads, users or dates appear in the video, in line with
`PRODUCT.md`.

## Explainer videos

Agents: use the `lampstand-video` skill (`.claude/skills/lampstand-video`). Read `projects/lampstand/MANIFEST.md`, write `projects/lampstand/videos/<name>.json`, then:

```bash
npm run lamp:validate
npm run lamp:sheet -- projects/lampstand/videos/<name>.json --sheet    # contact sheet in out/
npm run lamp:render -- projects/lampstand/videos/<name>.json           # out/<name>.mp4 (add --aspect 9:16 or 1:1)
```
