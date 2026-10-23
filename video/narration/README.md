# Narrated clips for the website

Thirteen narrated clips, each under 30 seconds, one per stage of a church's
journey from download to Sunday. They sit beside the matching sections of the
docs and together on `site/docs/watch.html`, each with captions (WebVTT) and a
transcript, as `PRODUCT.md` asks.

- **Voice:** `en-GB-Chirp3-HD-Aoede` (UK, female) from Google Cloud
  text-to-speech through the `narrate` skill, rate 0.9, 0.7 s between
  sentences. It is a synthetic voice, and the Watch page says so.
- **Scripts:** `scripts/NN-name.txt`, short declaratives. The transcripts on
  the Watch page are generated from these files.
- **Footage:** the real Svelte UI, run with the fake bridge in `../capture`,
  driven in time with the voice. The install clip is a drawn card.

| # | Clip | Footage | Length |
|---|------|---------|--------|
| 1 | Why one laptop | Landing; the Get started strip, Plan, Setup, then live | 21.6 s |
| 2 | Install it | Drawn card (an installer is not part of the app) | 24.0 s |
| 3 | Try the Welcome service | Get started, then Present with Space, back, blank | 19.1 s |
| 4 | Choose and calibrate your screen | Setup, Screens, Add window, Recalibrate | 17.2 s |
| 5 | Bring your songs across | Plan library, Add files, with a format list callout | 19.0 s |
| 6 | Plan the order of service | New service, quick add John 3:16, Open for live | 22.6 s |
| 7 | PowerPoint and Google Slides | Decks filter, then a deck live with its notes | 17.5 s |
| 8 | Run the service | Keys, verse jump, blank, speaker notes | 22.2 s |
| 9 | Loop the pre-service slides | Loop, the progress bar, Looping to stop | 19.2 s |
| 10 | Your church's look | Setup, Church look, name, Make my theme | 19.8 s |
| 11 | Feed your live stream | Setup, OBS, then Present with OBS status | 17.3 s |
| 12 | Plan together during the week | Setup, Devices, Pair a device | 21.8 s |
| 13 | Sunday morning, offline | Present live, Help, About, backup folder callout | 20.7 s |

## Make or remake them

```bash
cd video
npm ci && (cd ../app && npm ci)
node_modules/.bin/vite --config capture/vite.config.ts --port 1441   # terminal 1
node narration/build.mjs [name-fragment]                              # terminal 2
node narration/watch-page.mjs                                         # rewrites site/docs/watch.html
```

1. Read each script (from a folder outside the repo, because `uv` trips over a
   broken `~/.venv` above it):
   `uv run ~/.claude/skills/narrate/narrate.py --voice en-GB-Chirp3-HD-Aoede --rate 0.9 --pause 0.7 --file scripts/NN-name.txt --out out/audio/NN-name.mp3`
2. `build.mjs` finds where each sentence starts in the audio, runs the steps
   listed for that clip in `build.mjs` at those moments (a cursor ring, key
   caps and callouts are drawn over the page), records screenshots, and muxes
   footage, voice, poster and captions into `out/clips/`, copying them to
   `site/docs/clips/narrated/`.
3. Set `APP` if the capture server is not on port 1441, and `CHROME` if Chrome
   is not in the usual macOS place. `DEBUG=1` prints when each step ran.

`out/` is not committed; the published clips are in `site/docs/clips/narrated/`.

## Honesty notes

`PRODUCT.md` says not to claim what is not shipped, so the scripts leave out
things the docs describe but the code does not have yet:

- **Tablet and phone remote.** There is no QR code or remote page in the app
  (`crates/lampstand-server` lists it as Phase 2), though `using.html` says
  Present shows one. The sync clip stands in for it.
- **OBS scene switching and scene buttons** (T90, T91) are still in review.
  The stream clip covers the browser source and the status in Present.
- The Windows installer is described as unsigned, as in Getting started.
- The pairing code, and the fidelity report on the slides clip, are described
  in words: the fake bridge does not draw them.
