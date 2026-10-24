# Lampstand video framework (theme `lampstand`)

Explainer videos are a JSON scene list. Read only this file, write `projects/lampstand/videos/<slug>.json`, run the commands below from `video/`.

Look: goldenrod paper, ink, stamp violet; scenes alternate `tone` `paper` | `table` (dark brown) | `screen` (dark navy). Plain volunteer words, no jargon. Don't claim features are shipped, dates, or user numbers (see PRODUCT.md).

```
{ "aspect": "16:9" | "9:16" | "1:1", "scenes": [ ... ] }
```
Every scene also takes optional: `durationSec`, `enter` (rise|fade|wipe|pop), `exit` (fade|drop|none), `tone`.

| type | props | example |
|---|---|---|
| title | kicker?, heading, sub? | `{"type":"title","kicker":"Lampstand 0.8","heading":"Roles explained","sub":"Who runs what"}` |
| bullets | heading?, items[1-5] | `{"type":"bullets","heading":"What we believe","items":["Works offline"]}` |
| stat | value, prefix?, suffix?, label | `{"type":"stat","value":10,"suffix":" minutes","label":"of training is the goal"}` |
| cta | heading, action, url? | `{"type":"cta","heading":"Follow along","action":"Star it on GitHub","url":"github.com/incrhst/lampstand"}` |
| quote | text, attribution? (philosophy) | `{"type":"quote","text":"Sunday must not fail.","attribution":"Our first promise"}` |
| roles | heading?, show[] (default all four), focus? | `{"type":"roles","focus":"operator"}` |
| devices | heading?, link?, machines[1-4]: name, caption?, kind (laptop\|projector\|desk\|phone), roles[] | `{"type":"devices","machines":[{"name":"At the projector","kind":"projector","roles":["projection","audio"]}]}` |
| diagram | src (`diagrams/*.svg`), heading?, caption? | `{"type":"diagram","src":"diagrams/roles-two-machines.svg"}` |
| screenshot | src (`shots/*.png`), heading?, ratio (default 1.6), callouts[0-4]: x%, y%, label | `{"type":"screenshot","src":"shots/5-present.png","callouts":[{"x":36,"y":71,"label":"Press Next"}]}` |
| handover | role, outcome (allow\|decline\|cut), holder{name,kind}, asker{name,kind}, caption? (animated: ask, prompt, job moves or stays) | `{"type":"handover","role":"stream","outcome":"cut","holder":{"name":"Stream desk","kind":"desk"},"asker":{"name":"Mary's laptop"}}` |
| custom | component, props | one-off components in `custom/` (none yet) |

Roles are `projection`, `operator`, `stream`, `audio`. Their plain wording lives in `roles.ts`, so every scene says it the same way. Role colours: Projection blue, Operator green, Stream purple, Audio orange.

Files: screenshots in `public/shots/` (1-getstarted, 2-plan, 3-quickadd, 4-decks, 5-present, 5b-present-deck, 6-setup, 7-obs, 8-present-obs, 2560x1600); diagrams in `public/diagrams/` (roles-one/two/three/four-machines, roles-confidence-screen; from docs/diagrams, 0.8 design).

Default length comes from the words and callouts; set `durationSec` to tighten. A 60-second video is roughly 8 to 10 scenes.

```bash
npm run lamp:validate                                             # all videos
npm run lamp:sheet -- projects/lampstand/videos/X.json --sheet [--aspect 9:16]   # out/X-sheet.png
npm run lamp:render -- projects/lampstand/videos/X.json [--aspect 1:1]           # out/X.mp4
```
Samples: `sample-what-is-lampstand.json` (59s intro), `sample-roles-explained.json` (roles and devices).
