// npm run lamp:tokens - checks that tokens.ts `app` matches app/src/lib/kit/theme.css, so the videos
// show the app's real colours. Fails with the differences; it never edits either file.
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { app } from "../tokens";

const css = readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), "../../../../app/src/lib/kit/theme.css"), "utf8");
const vars = new Map([...css.matchAll(/--([a-z-]+):\s*([^;]+);/g)].map((m) => [m[1], m[2].trim().toLowerCase()]));
const bad = Object.entries(app).filter(([k, v]) => vars.get(k) !== v.toLowerCase());
for (const [k, v] of bad) console.error(`--${k}: tokens.ts has ${v}, theme.css has ${vars.get(k) ?? "(missing)"}`);
const roleVars = [...vars.keys()].filter((k) => k.startsWith("role-") && !(k in app));
for (const k of roleVars) console.error(`--${k} is in theme.css but not in tokens.ts`);
if (bad.length || roleVars.length) process.exit(1);
console.log(`ok   tokens.ts matches app/src/lib/kit/theme.css (${Object.keys(app).length} colours)`);
