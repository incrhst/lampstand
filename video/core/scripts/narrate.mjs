// node core/scripts/narrate.mjs <scene list.json> [--voice V] [--rate R]
// Turns each scene's "say" text into public/narration/<slug>/NN.mp3 with the `narrate` skill's Google backend,
// then writes "narration" and a "durationSec" (audio + a breath) back into the JSON. Re-runs only for changed text.
// Needs ~/.claude/skills/narrate and Google ADC. Cwd is a temp dir because old uv trips over a broken ~/.venv.
import { readFileSync, writeFileSync, mkdirSync, existsSync, copyFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { basename, resolve, join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir, tmpdir } from "node:os";

const args = process.argv.slice(2);
const val = (n, d) => (args.includes(n) ? args[args.indexOf(n) + 1] : d);
const file = resolve(args[0]);
const voice = val("--voice", "en-US-Chirp3-HD-Algenib");
const rate = val("--rate", "0.9");
// Shared pronunciation map (IPA for words the voice gets wrong, e.g. the mode "Present"). Applies to every narration.
const pronFile = join(dirname(fileURLToPath(import.meta.url)), "..", "pronunciations.json");
const pronIpa = JSON.parse(readFileSync(pronFile, "utf8")).ipa ?? {};
// Only lines that use a mapped word depend on the map, so adding an entry re-voices just those lines.
const pronFor = (say) => { const e = Object.entries(pronIpa).filter(([k]) => say.includes(k)); return e.length ? `|${JSON.stringify(e)}` : ""; };
const slug = basename(file, ".json");
const dir = resolve("public/narration", slug);
mkdirSync(dir, { recursive: true });
const lock = join(dir, "lock.json");
const seen = existsSync(lock) ? JSON.parse(readFileSync(lock, "utf8")) : {};
const list = JSON.parse(readFileSync(file, "utf8"));
const BREATH = 1.6; // seconds after the last word
const keepMin = { handover: 9 }; // animated scenes need their own time

list.scenes.forEach((s, i) => {
  if (!s.say) return;
  const name = `${String(i + 1).padStart(2, "0")}.mp3`;
  const out = join(dir, name);
  const h = createHash("sha1").update(`${voice}|${rate}|${s.say}${pronFor(s.say)}`).digest("hex");
  if (seen[name] !== h || !existsSync(out)) {
    const tmp = join(tmpdir(), `narrate-${slug}-${name}`);
    execFileSync("uv", ["run", "--with", "google-cloud-texttospeech>=2.38", "python", join(homedir(), ".claude/skills/narrate/narrate.py"), "--text", s.say, "--voice", voice, "--rate", rate, "--pronunciations", pronFile, "--out", tmp], { cwd: tmpdir(), stdio: ["ignore", "inherit", "inherit"] });
    copyFileSync(tmp, out);
    seen[name] = h;
  }
  const secs = Number(execFileSync("ffprobe", ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", out]).toString());
  s.narration = `narration/${slug}/${name}`;
  s.durationSec = Math.max(Math.ceil((secs + 0.3 + BREATH) * 2) / 2, keepMin[s.type] ?? 0, s.type === "handover" && s.outcome === "cut" ? 11 : 0);
  console.log(`${name} ${secs.toFixed(1)}s -> scene ${s.durationSec}s`);
});
writeFileSync(lock, JSON.stringify(seen, null, 1));
writeFileSync(file, JSON.stringify(list, null, 1) + "\n");
