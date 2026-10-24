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
const [projectDir, listFile] = args.filter((a, i) => !a.startsWith("--") && !(i && args[i - 1].startsWith("--") && ["--aspect", "--out"].includes(args[i - 1])));
if (!projectDir || !listFile) { console.error("usage: render.mjs <project dir> <scene list.json> [--sheet] [--aspect A] [--out F]"); process.exit(2); }

const dir = resolve(projectDir);
let props = JSON.parse(readFileSync(listFile, "utf8"));
if (val("--aspect")) props.aspect = val("--aspect");
const slug = basename(listFile, ".json") + (val("--aspect") ? "-" + val("--aspect").replace(":", "x") : "");
const outDir = resolve("out");
mkdirSync(outDir, { recursive: true });

const serveUrl = await bundle({ entryPoint: join(dir, "index.ts"), publicDir: resolve("public") });
const comp = await selectComposition({ serveUrl, id: "Video", inputProps: props });
console.log(`composition: ${comp.width}x${comp.height} ${comp.fps}fps ${(comp.durationInFrames / comp.fps).toFixed(1)}s`);

if (flag("--sheet")) {
  // Each scene's hold point: just before its exit starts. Scene lengths mirror MasterVideo (calculateMetadata sums them).
  const { sceneFrames } = await import("../src/duration.ts");
  const { defaultSec } = await import(join(dir, "duration.ts"));
  const { VideoProps } = await import(join(dir, "schema.ts"));
  const sheetDir = join(outDir, `${slug}-sheet`);
  rmSync(sheetDir, { recursive: true, force: true });
  mkdirSync(sheetDir, { recursive: true });
  let start = 0;
  props = VideoProps.parse(props);
  for (let i = 0; i < props.scenes.length; i++) {
    const n = sceneFrames(props.scenes[i], props.fps ?? 30, defaultSec);
    const frame = start + Math.max(0, n - 16);
    await renderStill({ composition: comp, serveUrl, inputProps: props, frame, output: join(sheetDir, String(i + 1).padStart(2, "0") + ".png") });
    start += n;
  }
  const cols = comp.width < comp.height ? 5 : 4;
  const rows = Math.ceil(props.scenes.length / cols);
  const tileW = comp.width < comp.height ? 360 : 480;
  execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-i", join(sheetDir, "%02d.png"), "-vf", `scale=${tileW}:-1,tile=${cols}x${rows}:padding=8:color=0x222222`, "-frames:v", "1", join(outDir, `${slug}-sheet.png`)]);
  console.log(`contact sheet: out/${slug}-sheet.png (${props.scenes.length} scenes)`);
} else {
  const out = val("--out") ?? join(outDir, `${slug}.mp4`);
  await renderMedia({ composition: comp, serveUrl, inputProps: props, codec: "h264", crf: 18, outputLocation: out, onProgress: ({ progress }) => process.stdout.write(`\r${Math.round(progress * 100)}%`) });
  console.log(`\nrendered ${out}`);
}
