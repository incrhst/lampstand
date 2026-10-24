---
name: lampstand-video
description: Make or change a short Lampstand explainer video (intro, philosophy, roles, devices, a feature walk-through) from a JSON scene list using the parametric Remotion framework in video/. Use when asked for a Lampstand video, explainer, promo cut or vertical/square version, or to add a scene type to that framework.
---

# Lampstand explainer videos

Videos are data. A normal video is one JSON file; you never write React for it.

## Make a video

1. Read `video/projects/lampstand/MANIFEST.md` and nothing else (not `src/`, `scenes/` or `core/`). It lists every scene type, its props and an example.
2. Copy a sample from `video/projects/lampstand/videos/` (`sample-what-is-lampstand.json` is a 60s intro, `sample-roles-explained.json` covers roles and devices) and edit it. Save as `videos/<slug>.json`.
3. From `video/` run `npm install` once, then:
   - `npm run lamp:validate` fixes props and missing files from the error text.
   - `npm run lamp:sheet -- projects/lampstand/videos/<slug>.json --sheet` writes `out/<slug>-sheet.png`. Look at that one image, not single frames. Fix by editing the JSON (wording, `durationSec`, `tone`). Two rounds at most, then show the user.
   - `npm run lamp:render -- projects/lampstand/videos/<slug>.json` writes `out/<slug>.mp4`. Add `--aspect 9:16` or `--aspect 1:1` for other shapes (also on `lamp:sheet`).
4. Report format, size, length, and what you did not run (for example other aspects). `out/` is not committed.

## Writing rules

- Audience: church volunteers aged 15 to 70. Plain words, short lines, no jargon (say "the screens", not "outputs"; "the person holding it", not "lease holder"). Role names and plain descriptions live in `video/projects/lampstand/roles.ts`; reuse them.
- Honest about status (PRODUCT.md): never say a feature is shipped, give dates, download counts, user numbers or testimonials. The roles design is 0.8 and still being built; label it so ("Lampstand 0.8").
- One idea per scene, at most 5 bullets, a 60s video is about 8 to 10 scenes. Set `durationSec` to tighten. Alternate `tone` (paper, table, screen) for rhythm.
- Philosophy lines come from PRODUCT.md principles: Sunday must not fail; built for volunteers; free and open; honest about status.
- Role handovers have their own animated scene, `handover` (outcomes `allow`, `decline`, `cut`); use it rather than describing the steps in bullets alone.
- Screenshots are in `video/public/shots/` (`callouts` x and y are percent of the picture). Roles diagrams are in `video/public/diagrams/`. Retake screenshots with `npm run capture` in `video/`; they come from the real UI with a fake bridge, so do not claim a staged projector picture is real.

## Extend the framework

Only when the existing scenes cannot say it. One-off: add a component to `video/projects/lampstand/custom/`, register it in `customRegistry`, use `{"type":"custom","component":"Name"}`. Used twice: promote it to a project scene in the same change (`schema.ts` union, `scenes/`, `registry.ts`, `duration.ts`, `MANIFEST.md`, and a sample video). A scene a second project needs moves to `video/core/src/core-scenes/`. Every scene takes colours, fonts and spacing from `theme.ts`, uses the motion presets and `FitText`, works in 16:9, 9:16 and 1:1, and has a default length. After any change run `npm run lamp:typecheck`, `lamp:validate`, and a contact sheet in at least two aspects. Keep MANIFEST.md in step and under about 1k tokens.

## Don't

Do not touch `crates/` or `app/`. The older fly-over (`video/src`, `npm run render`) is separate; leave it alone unless asked.
