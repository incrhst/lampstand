// node core/scripts/render.mjs <project dir> <scene list.json> [--sheet] [--aspect 9:16] [--out file]
//   default: renders out/<slug>.mp4 ; --sheet: renders one still per scene and a contact sheet out/<slug>-sheet.png
import { bundle } from "@remotion/bundler";
import { renderMedia, renderStill, selectComposition } from "@remotion/renderer";
import { readFileSync, mkdirSync, rmSync } from "node:fs";
import { resolve, basename, join } from "node:path";
import { execFileSync } from "node:child_process";

const args = process.argv.slice(2);
const flag = (n) => args.includes(n);
const val = (n) => (args.includes(n) ? args[args.indexOf(n) + 1] : undefined);
const [projectDir, listFile] = args.filter((a, i) => !a.startsWith("--") && !(i && args[i - 1].startsWith("--") && ["--aspect", "--out", "--frac"].includes(args[i - 1])));
if (!projectDir || !listFile) { console.error("usage: render.mjs <project dir> <scene list.json> [--sheet] [--aspect A] [--frac 0.5 (sheet at that point of each scene)] [--out F]"); process.exit(2); }

const dir = resolve(projectDir);
let props = JSON.parse(readFileSync(listFile, "utf8"));
if (val("--aspect")) props.aspect = val("--aspect");
const slug = basename(listFile, ".json") + (val("--aspect") ? "-" + val("--aspect").replace(":", "x") : "");
const outDir = resolve("out");
mkdirSync(outDir, { recursive: true });

const serveUrl = await bundle({ entryPoint: join(dir, "index.ts"), publicDir: resolve("public") });
const id = props.composition ?? "Video"; // e.g. "Tour" for the site's fly-over (videos/hero-tour.json)
const comp = await selectComposition({ serveUrl, id, inputProps: props });
console.log(`composition: ${comp.width}x${comp.height} ${comp.fps}fps ${(comp.durationInFrames / comp.fps).toFixed(1)}s`);

if (flag("--sheet")) {
  // Each scene's hold point: just before its exit starts. Scene lengths mirror MasterVideo (calculateMetadata sums them).
  const { sceneFrames } = await import("../src/duration.ts");
  const { compositions } = await import(join(dir, "compositions.ts"));
  const { defaultSec, schema: VideoProps } = compositions[id];
  const sheetDir = join(outDir, `${slug}-sheet`);
  rmSync(sheetDir, { recursive: true, force: true });
  mkdirSync(sheetDir, { recursive: true });
  let start = 0;
  props = VideoProps.parse(props);
  for (let i = 0; i < props.scenes.length; i++) {
    const n = sceneFrames(props.scenes[i], props.fps ?? 30, defaultSec);
    const frac = val("--frac");
    const frame = start + (frac ? Math.round(n * Number(frac)) : Math.max(0, n - 16));
    await renderStill({ composition: comp, serveUrl, inputProps: props, frame, output: join(sheetDir, String(i + 1).padStart(2, "0") + ".png") });
    start += n;
  }
  const cols = comp.width < comp.height ? 5 : 4;
  const tileW = comp.width < comp.height ? 360 : 480;
  // xstack, not the tile filter: ffmpeg 8's tile drops all but the last frame of an image sequence here.
  const n = props.scenes.length;
  const ins = Array.from({ length: n }, (_, i) => ["-i", join(sheetDir, String(i + 1).padStart(2, "0") + ".png")]).flat();
  const cell = (k, d) => (k ? Array(k).fill(d).join("+") : "0");
  const scaled = Array.from({ length: n }, (_, i) => `[${i}:v]scale=${tileW}:-1,pad=iw+8:ih+8:0:0:color=0x222222[v${i}]`).join(";");
  const graph = n === 1 ? scaled.replace("[v0]", "") : `${scaled};${Array.from({ length: n }, (_, i) => `[v${i}]`).join("")}xstack=inputs=${n}:layout=${Array.from({ length: n }, (_, i) => `${cell(i % cols, "w0")}_${cell(Math.floor(i / cols), "h0")}`).join("|")}:fill=0x222222`;
  execFileSync("ffmpeg", ["-y", "-loglevel", "error", ...ins, "-filter_complex", graph, "-frames:v", "1", join(outDir, `${slug}-sheet.png`)]);
  console.log(`contact sheet: out/${slug}-sheet.png (${props.scenes.length} scenes)`);
} else {
  const out = val("--out") ?? join(outDir, `${slug}.mp4`);
  await renderMedia({ composition: comp, serveUrl, inputProps: props, codec: "h264", crf: 18, outputLocation: out, onProgress: ({ progress }) => process.stdout.write(`\r${Math.round(progress * 100)}%`) });
  console.log(`\nrendered ${out}`);
}
