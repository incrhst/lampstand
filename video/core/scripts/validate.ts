// npx tsx core/scripts/validate.ts projects/lampstand projects/lampstand/videos/x.json [...]
// Parses scene lists with the project's zod schema, checks that referenced files exist, prints the length.
import { readFileSync, existsSync } from "node:fs";
import { resolve, join } from "node:path";
import { pathToFileURL } from "node:url";
import { sceneFrames } from "../src/duration";

const [projectArg, ...files] = process.argv.slice(2);
if (!projectArg || !files.length) { console.error("usage: validate.ts <project dir> <scene list.json>..."); process.exit(2); }
const dir = resolve(projectArg);
const publicDir = resolve(dir, "../../public");
const { VideoProps } = await import(pathToFileURL(join(dir, "schema.ts")).href);
const { defaultSec } = await import(pathToFileURL(join(dir, "duration.ts")).href);

let bad = 0;
for (const f of files) {
  const raw = JSON.parse(readFileSync(f, "utf8"));
  const r = VideoProps.safeParse(raw);
  if (!r.success) {
    bad++;
    console.error(`FAIL ${f}`);
    for (const i of r.error.issues) console.error(`  scenes.${i.path.slice(1).join(".")}: ${i.message}`);
    continue;
  }
  const missing: string[] = [];
  const walk = (o: any) => { if (o && typeof o === "object") for (const [k, v] of Object.entries(o)) { if (k === "src" && typeof v === "string" && !existsSync(join(publicDir, v))) missing.push(v); else walk(v); } };
  walk(r.data.scenes);
  if (missing.length) { bad++; console.error(`FAIL ${f}\n  missing files in public/: ${missing.join(", ")}`); continue; }
  const frames = r.data.scenes.reduce((n: number, s: any) => n + sceneFrames(s, r.data.fps, defaultSec), 0);
  console.log(`ok   ${f}  ${r.data.scenes.length} scenes, ${(frames / r.data.fps).toFixed(1)}s, ${r.data.aspect}`);
}
process.exit(bad ? 1 : 0);
