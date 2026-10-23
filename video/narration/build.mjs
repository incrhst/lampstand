// Builds the narrated website clips: reads each script's audio (made with the
// narrate skill, see README.md), drives the real UI with the fake bridge in
// time with the voice, records it, and muxes footage, voice and captions.
//
//   node narration/build.mjs [name-fragment]
//
// Needs the capture server (APP below, default http://localhost:1441/ from
// `node_modules/.bin/vite --config capture/vite.config.ts --port 1441`),
// Chrome (CHROME to override) and ffmpeg. Output: narration/out/clips, and a
// copy in site/docs/clips/narrated for the docs pages.
import pw from "playwright-core";
import { copyFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { execFileSync, spawnSync } from "node:child_process";

const { chromium } = pw;
const APP = process.env.APP ?? "http://localhost:1441/";
const HERE = new URL("./", import.meta.url).pathname;
const SITE = new URL("../../site/docs/clips/narrated/", import.meta.url).pathname;
const OUT = `${HERE}out/clips/`;
const RAW = `${HERE}out/raw/`;
const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const W = 1280, H = 800, LEAD = 0.5, TAIL = 1.2;
mkdirSync(OUT, { recursive: true });
mkdirSync(SITE, { recursive: true });

const sh = (cmd, args) => execFileSync(cmd, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const splitSentences = (text) => text.trim().split(/(?<=[.?!])\s+/);

// Where each sentence starts in the voice track: the end of each pause the
// narrator leaves between sentences. Falls back to character proportions.
function sentenceStarts(mp3, sentences, duration) {
  const log = spawnSync("ffmpeg", ["-hide_banner", "-i", mp3, "-af", "silencedetect=noise=-38dB:d=0.3", "-f", "null", "-"], { encoding: "utf8" }).stderr;
  // The pauses between sentences are the longest silences; commas leave shorter ones.
  const gaps = [...log.matchAll(/silence_end: ([\d.]+) \| silence_duration: ([\d.]+)/g)].map((m) => ({ end: Number(m[1]), len: Number(m[2]) })).filter((g) => g.end < duration - 0.3);
  const ends = gaps.sort((a, b) => b.len - a.len).slice(0, sentences.length - 1).map((g) => g.end).sort((a, b) => a - b);
  if (ends.length === sentences.length - 1) return [0, ...ends];
  console.log(`  ${mp3.split("/").pop()}: found ${ends.length + 1} sentences, expected ${sentences.length}; using proportions`);
  const total = sentences.reduce((n, s) => n + s.length, 0);
  let acc = 0;
  return sentences.map((s) => { const t = (acc / total) * duration; acc += s.length; return t; });
}

const probe = (f) => Number(sh("ffprobe", ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", f]).trim());

// Drawn over the page while recording: a cursor ring, key caps and a callout.
const overlay = () => {
  const css = document.createElement("style");
  css.textContent = `
    #__cur{position:fixed;left:0;top:0;width:26px;height:26px;margin:-13px 0 0 -13px;border:3px solid #e9b949;border-radius:50%;background:rgba(233,185,73,.25);pointer-events:none;z-index:99999;transition:transform .28s cubic-bezier(.3,.7,.3,1),scale .12s;box-shadow:0 0 0 2px rgba(0,0,0,.35)}
    #__cur.down{scale:.7;background:rgba(233,185,73,.6)}
    #__key{position:fixed;right:32px;bottom:64px;display:flex;gap:8px;pointer-events:none;z-index:99999}
    #__key b{font:700 26px/1 "Courier New",monospace;color:#1d1a15;background:#f5f1e8;border:2px solid #1d1a15;border-bottom-width:6px;border-radius:8px;padding:12px 18px;min-width:30px;text-align:center;box-shadow:0 6px 20px rgba(0,0,0,.45)}
    #__chip{position:fixed;left:50%;bottom:56px;transform:translateX(-50%);max-width:1000px;pointer-events:none;z-index:99999;font:600 24px/1.35 system-ui,sans-serif;color:#f5f1e8;background:rgba(11,14,22,.94);border:2px solid #e9b949;border-radius:10px;padding:14px 24px;text-align:center;box-shadow:0 8px 30px rgba(0,0,0,.5)}
  `;
  document.head.append(css);
  const cur = document.createElement("div"); cur.id = "__cur"; cur.style.transform = "translate(640px,420px)";
  document.body.append(cur);
  addEventListener("mousemove", (e) => { cur.style.transform = `translate(${e.clientX}px,${e.clientY}px)`; }, true);
  addEventListener("mousedown", () => cur.classList.add("down"), true);
  addEventListener("mouseup", () => cur.classList.remove("down"), true);
  let kt, ct;
  window.__key = (labels, ms = 1300) => {
    document.getElementById("__key")?.remove();
    const k = document.createElement("div"); k.id = "__key";
    for (const l of [].concat(labels)) { const b = document.createElement("b"); b.textContent = l; k.append(b); }
    document.body.append(k); clearTimeout(kt); kt = setTimeout(() => k.remove(), ms);
  };
  window.__chip = (text, ms = 4500) => {
    document.getElementById("__chip")?.remove();
    const c = document.createElement("div"); c.id = "__chip"; c.textContent = text;
    document.body.append(c); clearTimeout(ct); if (ms) ct = setTimeout(() => c.remove(), ms);
  };
  window.__unchip = () => document.getElementById("__chip")?.remove();
};

const key = async (p, press, label = press, ms) => { await p.evaluate(([l, m]) => window.__key(l, m), [label, ms]); await p.keyboard.press(press); };
const chip = (p, text, ms) => p.evaluate(([t, m]) => window.__chip(t, m), [text, ms]);
const click = (p, name) => (name instanceof RegExp ? p.getByRole("button", { name }) : p.getByText(name)).first().click();
const openSunday = async (p) => { await click(p, "Sunday service"); await sleep(400); };
const goLive = async (p) => { await click(p, /Open for live/); await sleep(600); };
const startItem = async (p) => { await p.keyboard.press("Shift+ArrowDown"); await p.keyboard.press("Enter"); await sleep(250); await p.keyboard.press(" "); };

// The install card is drawn for the film, since an installer is not part of the app.
const installCard = `<!doctype html><meta charset=utf-8><style>
@font-face{font-family:"Old Standard TT";src:url(../../../site/fonts/old-standard-tt-700-normal.woff2);font-weight:700}
@font-face{font-family:"Courier Prime";src:url(../../../site/fonts/courier-prime-400-normal.woff2)}
*{box-sizing:border-box}body{margin:0;width:1280px;height:800px;background:#2b211a;color:#f5f1e8;font-family:"Courier Prime",monospace;display:grid;grid-template-rows:auto 1fr auto;padding:48px 64px;gap:28px}
h1{margin:0;font:700 44px "Old Standard TT",serif;letter-spacing:.08em;text-transform:uppercase;color:#f2c452}
.row{display:grid;grid-template-columns:repeat(3,1fr);gap:24px;align-content:start}
.p{border:3px solid #6b5640;border-radius:6px;padding:26px;opacity:.35;transition:all .4s;min-height:330px;display:flex;flex-direction:column;gap:14px}
.p h2{margin:0;font:700 34px "Old Standard TT",serif;color:#f2c452}.p .f{font-size:20px;opacity:.8}.p .s{font-size:23px;line-height:1.45;margin-top:auto}
body[data-step="0"] .p,body[data-step="1"] .p.mac,body[data-step="2"] .p.win{opacity:1;border-color:#e9b949;transform:translateY(-4px)}
body[data-step="1"] .p.mac,body[data-step="2"] .p.win{background:#3a2d22}
.n{font-size:26px;border-top:3px double #6b5640;padding-top:20px;min-height:70px;opacity:.35;transition:opacity .4s}
body[data-step="0"] .n.a,body[data-step="3"] .n.b{opacity:1;color:#f2c452}
</style><body data-step="0"><h1>Install Lampstand</h1>
<div class=row>
<div class="p mac"><h2>Mac</h2><div class=f>Lampstand.dmg</div><div class=s>Drag <b>Lampstand</b> into <b>Applications</b>. Open it, and choose <b>Open</b>.</div></div>
<div class="p win"><h2>Windows</h2><div class=f>Lampstand installer</div><div class=s>&ldquo;Windows protected your PC&rdquo;: choose <b>More info</b>, then <b>Run anyway</b>.</div></div>
<div class="p lin"><h2>Linux</h2><div class=f>AppImage or .deb</div><div class=s>Make the AppImage runnable and start it, or install the .deb.</div></div>
</div>
<div><div class="n a">Download from github.com/incrhst/lampstand/releases/latest</div><div class="n b">The internet is needed only to download and to update.</div></div>`;

// Each clip: the scene the fake bridge shows, what to do before recording,
// and steps as [sentence number, seconds after it starts, action].
const clips = {
  "01-why": { scene: "fresh", steps: [
    [0, 0.3, (p) => click(p, "Songs")],
    [1, 0.2, (p) => click(p, "Decks")],
    [2, 0.2, async (p) => { await click(p, "Setup"); await sleep(300); await click(p, "Send pictures, control scenes"); }],
    [3, 0.2, async (p) => { await click(p, "Plan"); await sleep(400); await openSunday(p); }],
    [5, 0.4, async (p) => { await goLive(p); await startItem(p); }],
  ] },
  "02-install": { card: installCard, steps: [
    [1, 0, (p) => p.evaluate(() => (document.body.dataset.step = "1"))],
    [2, 0, (p) => p.evaluate(() => (document.body.dataset.step = "2"))],
    [3, 0, (p) => p.evaluate(() => (document.body.dataset.step = "3"))],
  ] },
  "03-welcome": { scene: "fresh", steps: [
    [0, 1.2, (p) => click(p, "Try the Welcome service")],
    [1, 0.4, (p) => key(p, " ", "Space")],
    [2, 0.4, (p) => key(p, " ", "Space")],
    [3, 0.2, (p) => key(p, "ArrowLeft", "←")],
    [3, 2.4, (p) => key(p, ".", ".")],
    [4, 0.4, (p) => key(p, ".", ".")],
  ] },
  "04-screens": { scene: "default", steps: [
    [0, 1.4, (p) => click(p, "Setup")],
    [1, 0.5, (p) => click(p, "Add window")],
    [2, 0.4, (p) => click(p, "Recalibrate")],
  ] },
  "05-songs": { scene: "default", steps: [
    [0, 0.4, (p) => p.getByText("Add files").first().hover()],
    [1, 0.2, (p) => chip(p, "OpenLP · EasyWorship · ProPresenter · FreeShow · OpenSong · OpenLyrics · SongSelect", 0)],
    [2, 0.2, (p) => chip(p, "A report lists anything that did not come across", 0)],
  ] },
  "06-plan": { scene: "default", steps: [
    [0, 0.5, (p) => click(p, "New service")],
    [1, 0.6, async (p) => { await openSunday(p).catch(() => {}); await key(p, "/", "/"); await p.keyboard.type("john 3:16", { delay: 90 }); }],
    [2, 0.2, (p) => key(p, "Enter", "Enter")],
    [4, 0.5, (p) => click(p, /Open for live/)],
  ] },
  "07-slides": { scene: "default", prep: openSunday, steps: [
    [0, 0.4, (p) => click(p, "Decks")],
    [1, 0.4, async (p) => { await goLive(p); for (let i = 0; i < 4; i++) await p.keyboard.press("Shift+ArrowDown"); await p.keyboard.press("Enter"); await sleep(250); await p.keyboard.press(" "); }],
    [1, 3.2, (p) => key(p, " ", "Space")],
    [2, 0.2, (p) => chip(p, "Fidelity report: what played, and what could not be reproduced", 0)],
  ] },
  "08-present": { scene: "default", prep: async (p) => { await openSunday(p); await goLive(p); }, steps: [
    [0, 0.4, (p) => startItem(p)],
    [0, 2.2, (p) => key(p, " ", "Space")],
    [1, 0.3, (p) => key(p, "ArrowLeft", "←")],
    [2, 0.3, (p) => key(p, "3", "3")],
    [2, 3.0, (p) => key(p, "c", "C")],
    [3, 0.3, (p) => key(p, ".", ".")],
    [3, 1.8, (p) => key(p, ".", ".")],
    [4, 0.2, async (p) => { for (let i = 0; i < 4; i++) await p.keyboard.press("Shift+ArrowDown"); await p.keyboard.press("Enter"); await sleep(250); await p.keyboard.press(" "); }],
  ] },
  "09-loop": { scene: "tips-loop", prep: async (p) => { await click(p, "Pre-service and service"); await goLive(p); }, steps: [
    [1, 0.4, async (p) => { await p.keyboard.press("Shift+ArrowDown"); await p.keyboard.press("Enter"); await sleep(250); await p.keyboard.press(" "); await sleep(500); await click(p, /^Loop$/); }],
    [3, 0.4, (p) => click(p, /Looping/)],
  ] },
  "10-look": { scene: "default", steps: [
    [0, 0.6, async (p) => { await click(p, "Setup"); await sleep(500); await click(p, "Church look"); }],
    [1, 0.4, async (p) => { const n = p.getByRole("textbox").first(); await n.click(); await p.keyboard.type("Grace Chapel", { delay: 70 }); }],
    [2, 0.2, (p) => click(p, "Make my theme")],
    [3, 0.4, (p) => p.mouse.wheel(0, 380)],
  ] },
  "11-stream": { scene: "default", steps: [
    [0, 0.4, async (p) => { await click(p, "Setup"); await sleep(500); await click(p, "Send pictures, control scenes"); }],
    [2, 0.3, (p) => chip(p, "Lyrics as a browser source in OBS, no plugin", 0)],
    [3, 0.1, async (p) => { await p.evaluate(() => { window.__unchip(); window.__obsOn(); }); await p.getByRole("button", { name: "Plan", exact: true }).click(); await sleep(400); await openSunday(p); await goLive(p); await startItem(p); }],
  ] },
  "12-sync": { scene: "default", steps: [
    [0, 0.4, (p) => click(p, "Plan")],
    [1, 0.2, async (p) => { await click(p, "Setup"); await sleep(500); await click(p, "Pairing and sync"); }],
    [1, 2.6, (p) => click(p, "Pair a device")],
    [2, 0.3, (p) => chip(p, "A six-digit code, shown on the presenting laptop", 0)],
    [3, 0.4, async (p) => { await p.evaluate(() => window.__unchip()); await chip(p, "Changes travel between your church's own devices, encrypted", 0); }],
  ] },
  "13-sunday": { scene: "default", prep: async (p) => { await openSunday(p); await goLive(p); }, steps: [
    [0, 0.3, (p) => startItem(p)],
    [1, 0.3, (p) => chip(p, "Changes are saved as you go", 3500)],
    [2, 0.3, async (p) => { await click(p, "Help"); await sleep(500); await click(p, "About"); }],
    [3, 0.3, (p) => chip(p, "Back up the Lampstand folder: ~/Library/Application Support/org.lampstand.app (Mac)", 0)],
  ] },
};

const srtTime = (t) => {
  const ms = Math.round(t * 1000), pad = (n, w = 2) => String(n).padStart(w, "0");
  return `${pad(Math.floor(ms / 3600000))}:${pad(Math.floor(ms / 60000) % 60)}:${pad(Math.floor(ms / 1000) % 60)}.${pad(ms % 1000, 3)}`;
};

async function build(browser, id, spec) {
  const text = readFileSync(`${HERE}scripts/${id}.txt`, "utf8");
  const mp3 = `${HERE}out/audio/${id}.mp3`;
  const sentences = splitSentences(text);
  const duration = probe(mp3);
  const starts = sentenceStarts(mp3, sentences, duration);
  const total = LEAD + duration + TAIL;

  const dir = `${RAW}${id}/`;
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  const ctx = await browser.newContext({ viewport: { width: W, height: H } });
  const p = await ctx.newPage();
  await p.addInitScript(([s]) => {
    window.__scene = s;
    // The Get started strip shows only where a clip is about the start.
    if (s !== "fresh") localStorage.setItem("lampstand.getstarted.hidden", "1");
    localStorage.setItem("lampstand.tips.dismissed", JSON.stringify(["loop-group", "loop-progress", "fold-group", "quick-add", "jump-keys", "blank-screen", "safe-area"]));
  }, [spec.scene ?? "default"]);
  p.on("pageerror", (e) => console.log(`  ${id} pageerror`, e.message));
  if (spec.card) {
    const f = `${HERE}out/card-${id}.html`;
    writeFileSync(f, spec.card);
    await p.goto(`file://${f}`);
  } else {
    await p.goto(APP);
    await p.waitForTimeout(600);
    await p.evaluate(overlay);
    if (spec.prep) await spec.prep(p);
  }

  // Screenshots always show the page as it is now (a screencast lags behind in
  // headless Chrome), each stamped with the moment it was taken.
  const cdp = await ctx.newCDPSession(p);
  const frames = [];
  const began = Date.now();
  let recording = true;
  const grab = (async () => {
    while (recording) {
      const t = Date.now() / 1000;
      const { data } = await cdp.send("Page.captureScreenshot", { format: "jpeg", quality: 75, optimizeForSpeed: true });
      frames.push({ t, data });
    }
  })();
  for (const [i, off, fn] of [...spec.steps].sort((a, b) => starts[a[0]] + a[1] - starts[b[0]] - b[1])) {
    const at = (LEAD + starts[i] + off) * 1000;
    const wait = at - (Date.now() - began);
    if (wait > 0) await sleep(wait);
    if (process.env.DEBUG) console.log(`  ${id} step ${i}+${off} due ${(at / 1000).toFixed(2)} ran ${((Date.now() - began) / 1000).toFixed(2)}`);
    try { await fn(p); } catch (e) { console.log(`  ${id} step ${i}+${off}: ${e.message.split("\n")[0]}`); }
  }
  const rest = total * 1000 - (Date.now() - began);
  if (rest > 0) await sleep(rest);
  recording = false;
  await grab;
  const end = began / 1000 + total;
  await ctx.close();
  const n = frames.length;
  if (process.env.DEBUG) console.log(`  frames ${n}: first ${(frames[0].t - began / 1000).toFixed(2)} last ${(frames[n - 1].t - began / 1000).toFixed(2)} of ${total.toFixed(2)}`);
  const list = frames.map((f, i) => {
    writeFileSync(`${dir}${String(i).padStart(4, "0")}.jpg`, Buffer.from(f.data, "base64"));
    const from = i === 0 ? began / 1000 : f.t;
    const to = i + 1 < n ? frames[i + 1].t : end;
    return `file '${dir}${String(i).padStart(4, "0")}.jpg'\nduration ${Math.max(0.01, to - from).toFixed(3)}`;
  });
  list.push(`file '${dir}${String(n - 1).padStart(4, "0")}.jpg'`);
  writeFileSync(`${dir}list.txt`, list.join("\n"));

  const mp4 = `${OUT}${id}.mp4`;
  sh("ffmpeg", ["-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", `${dir}list.txt`, "-i", mp3,
    "-filter_complex", `[1:a]adelay=${LEAD * 1000}|${LEAD * 1000},apad[a]`, "-map", "0:v", "-map", "[a]",
    "-vf", "fps=24,format=yuv420p", "-t", total.toFixed(2), "-c:v", "libx264", "-crf", "27", "-preset", "slow",
    "-c:a", "aac", "-b:a", "96k", "-movflags", "+faststart", mp4]);
  sh("ffmpeg", ["-y", "-loglevel", "error", "-ss", (total * 0.7).toFixed(2), "-i", mp4, "-frames:v", "1", "-vf", "scale=960:-2", "-q:v", "4", `${OUT}${id}.jpg`]);

  const cues = sentences.map((s, i) => {
    const from = LEAD + starts[i];
    const to = i + 1 < sentences.length ? LEAD + starts[i + 1] - 0.1 : LEAD + duration;
    return `${i + 1}\n${srtTime(from)} --> ${srtTime(to)}\n${s}\n`;
  });
  writeFileSync(`${OUT}${id}.vtt`, `WEBVTT\n\n${cues.join("\n")}`);
  for (const ext of ["mp4", "vtt", "jpg"]) copyFileSync(`${OUT}${id}.${ext}`, `${SITE}${id}.${ext}`);
  console.log(`clip ${id}: ${total.toFixed(1)} s, ${n} frames`);
}

const only = process.argv[2];
const browser = await chromium.launch({ executablePath: chrome });
for (const [id, spec] of Object.entries(clips)) if (!only || id.includes(only)) await build(browser, id, spec);
await browser.close();
