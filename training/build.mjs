// Rebuilds the bundled training material (#190) from its sources in training/.
//
//   node training/build.mjs                 decks, key sheet, videos, then publish
//   node training/build.mjs decks           only the PPTX decks
//   node training/build.mjs videos          only the videos (narration, render, compress)
//   node training/build.mjs videos t1-welcome   one video
//   node training/build.mjs sheet           the printable key sheet (Chrome prints it to PDF)
//   node training/build.mjs publish         copy the videos to the website, with their manifest
//   --no-narrate                            render with the narration already in video/public
//
// Decks: training/decks/build_decks.py writes training/decks/*.pptx (python-pptx, through uv),
// which the app embeds (app/src-tauri/src/training.rs).
// Videos: each training/videos/<name>.json is a scene list for the parametric Remotion framework
// in video/ (theme `lampstand`). The build narrates it (video/core/scripts/narrate.mjs: Google
// Cloud TTS, Chirp 3 HD voice Algenib, which needs Google credentials; it only re-synthesises
// lines whose words changed), renders it at 1920x1080, and compresses it to 1280x720 H.264 with
// mono AAC into training/media/<name>.mp4.
// Publish: the app does not embed the videos. `cargo xtask training-videos` copies
// training/media/*.mp4 into site/training/ and writes site/training/videos.json (each video's
// file, size and BLAKE3 hash). Merged to main, the website serves them at
// https://lampstand.dbain.com/training/, and installed copies download a new or changed video
// the next time a training service is opened: no release needed.
//
// The screenshots the videos show come from scripted runs of the real UI:
// `npm run capture:serve` and `npm run capture:training` in video/.
//
// Needs: node, uv, ffmpeg, cargo; `npm install` in video/ once.
import { execFileSync } from "node:child_process";
import { readdirSync, statSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";

const here = dirname(fileURLToPath(import.meta.url));
const video = resolve(here, "../video");
const args = process.argv.slice(2);
const flags = new Set(args.filter((a) => a.startsWith("--")));
const [what = "all", only] = args.filter((a) => !a.startsWith("--"));
const run = (cmd, argv, cwd) => execFileSync(cmd, argv, { cwd, stdio: "inherit" });

if (what === "all" || what === "decks") {
  // Old uv trips over a broken ~/.venv when run from inside the home folder, so run from a temp dir.
  run("uv", ["run", "--quiet", "--with", "python-pptx==1.0.2", "python", join(here, "decks/build_decks.py")], tmpdir());
}

if (what === "all" || what === "sheet") {
  // Chrome through video/'s playwright-core; set CHROME if it is not in the usual macOS place.
  const { chromium } = createRequire(join(video, "package.json"))("playwright-core");
  const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
  const browser = await chromium.launch({ executablePath: chrome });
  const page = await browser.newPage();
  await page.goto(pathToFileURL(join(here, "cheat-sheet.html")).href);
  await page.pdf({ path: join(here, "cheat-sheet.pdf"), preferCSSPageSize: true, printBackground: true });
  await browser.close();
  console.log("cheat-sheet.pdf");
}

if (what === "all" || what === "videos") {
  const out = join(here, "media");
  mkdirSync(out, { recursive: true });
  const names = readdirSync(join(here, "videos"))
    .filter((f) => f.endsWith(".json"))
    .map((f) => f.replace(/\.json$/, ""))
    .filter((n) => !only || n === only);
  run("npx", ["tsx", "core/scripts/validate.ts", "projects/lampstand", ...names.map((n) => join(here, "videos", `${n}.json`))], video);
  for (const name of names) {
    const list = join(here, "videos", `${name}.json`);
    if (!flags.has("--no-narrate")) run("node", ["core/scripts/narrate.mjs", list], video);
    const full = join(video, "out", `${name}.mp4`);
    run("node", ["--import", "tsx", "core/scripts/render.mjs", "projects/lampstand", list, "--out", full], video);
    const small = join(out, `${name}.mp4`);
    // Flat graphics compress well: 720p, CRF 30, 30 fps, mono speech. Main profile without
    // B-frames keeps it simple for Lampstand's own pure-Rust decoder.
    run("ffmpeg", [
      "-y", "-loglevel", "error", "-i", full,
      "-vf", "scale=1280:720:flags=lanczos", "-r", "30",
      "-c:v", "libx264", "-profile:v", "main", "-bf", "0", "-preset", "veryslow", "-crf", "30", "-pix_fmt", "yuv420p",
      "-c:a", "aac", "-ac", "1", "-b:a", "64k",
      "-movflags", "+faststart", "-map_metadata", "-1", small,
    ]);
    console.log(`${name}.mp4: ${(statSync(small).size / 1024).toFixed(0)} KB`);
  }
  if (what === "videos") console.log("Then publish them: node training/build.mjs publish");
}

if (what === "all" || what === "publish") {
  // The hashes are BLAKE3, the app's blob hash, so the xtask writes the manifest.
  run("cargo", ["xtask", "training-videos"], resolve(here, ".."));
}
