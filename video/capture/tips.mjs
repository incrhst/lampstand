// Records the quick-tip clips under ../app/public/tips from the real UI with
// the fake bridge. Needs a capture server (npx vite --config capture/vite.config.ts
// --port N) and Chrome; set APP to its address (default port 1431) and
// CHROME to Chrome's path. Needs ffmpeg for the crop and trim.
import { chromium } from "playwright-core";
import { copyFileSync, mkdirSync, rmSync } from "node:fs";
import { execFileSync } from "node:child_process";

const APP = process.env.APP ?? "http://localhost:1431/";
const OUT = new URL("../../app/public/tips/", import.meta.url).pathname;
// The website docs show the same clips.
const SITE = new URL("../../site/docs/clips/", import.meta.url).pathname;
const RAW = new URL("../out/tips-raw/", import.meta.url).pathname;
mkdirSync(OUT, { recursive: true });
mkdirSync(SITE, { recursive: true });
rmSync(RAW, { recursive: true, force: true });
mkdirSync(RAW, { recursive: true });
const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const browser = await chromium.launch({ executablePath: chrome });
const W = 1280, H = 800;

// Playwright's own video recorder needs a downloaded ffmpeg, so frames are
// screenshotted at about 12 per second and encoded with the system ffmpeg.
const FPS = 12;
const crops = { narrow: "400:620:0:110", wide: "1000:620:0:110", full: "1280:690:0:110", top: "1280:690:0:0" };

async function clip(name, scene, crop, script, { open = "Pre-service and service" } = {}) {
  const dir = `${RAW}${name}/`;
  mkdirSync(dir, { recursive: true });
  const ctx = await browser.newContext({ viewport: { width: W, height: H } });
  const p = await ctx.newPage();
  await p.addInitScript(([s]) => {
    window.__scene = s;
    localStorage.setItem("lampstand.getstarted.hidden", "1");
    localStorage.setItem("lampstand.tips.dismissed", JSON.stringify(["loop-group", "loop-progress", "fold-group", "quick-add", "jump-keys", "blank-screen", "safe-area"]));
  }, [scene]);
  p.on("pageerror", (e) => console.log(name, "pageerror", e.message));
  await p.goto(APP);
  await p.waitForTimeout(500);
  if (open) {
    await p.getByText(open).first().click();
    await p.getByRole("button", { name: /Open for live/ }).click();
    await p.waitForTimeout(700);
  }
  let n = 0, recording = true;
  const began = Date.now();
  const grab = (async () => {
    while (recording) {
      const t = Date.now();
      await p.screenshot({ path: `${dir}${String(n++).padStart(4, "0")}.png`, type: "png" });
      await p.waitForTimeout(Math.max(0, 1000 / FPS - (Date.now() - t)));
    }
  })();
  await script(p);
  recording = false;
  await grab;
  // Screenshots take longer than 1/FPS, so play the frames back at the rate they were taken.
  const rate = n / ((Date.now() - began) / 1000);
  await ctx.close();
  const [cw, ch, cx, cy] = crop.split(":");
  execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-framerate", rate.toFixed(2), "-i", `${dir}%04d.png`, "-vf", `crop=${cw}:${ch}:${cx}:${cy},fps=24,format=yuv420p`, "-an", "-c:v", "libx264", "-crf", "30", "-preset", "slow", "-movflags", "+faststart", `${OUT}${name}.mp4`]);
  copyFileSync(`${OUT}${name}.mp4`, `${SITE}${name}.mp4`);
  console.log("clip", name, n, "frames");
}

const wait = (p, ms) => p.waitForTimeout(ms);
const startItem = async (p) => {
  await p.keyboard.press("Shift+ArrowDown"); await p.keyboard.press("Enter");
  await wait(p, 250); await p.keyboard.press(" ");
};

await clip("loop-group", "tips", crops.narrow, async (p) => {
  await wait(p, 900);
  await p.getByRole("button", { name: /^Loop$/ }).first().click();
  await wait(p, 2200);
});
await clip("loop-progress", "tips-loop", crops.wide, async (p) => {
  await startItem(p);
  await wait(p, 7000);
});
await clip("fold-group", "tips", crops.narrow, async (p) => {
  await wait(p, 800);
  await p.getByRole("button", { name: /Pre-service/ }).first().click();
  await wait(p, 1500);
  await p.getByRole("button", { name: /Pre-service/ }).first().click();
  await wait(p, 1500);
});
await clip("quick-add", "tips", crops.full, async (p) => {
  await startItem(p);
  await wait(p, 800);
  await p.keyboard.press("/");
  await wait(p, 500);
  await p.keyboard.type("amazing", { delay: 120 });
  await wait(p, 1800);
});
await clip("jump-keys", "tips", crops.full, async (p) => {
  await startItem(p);
  await wait(p, 1200);
  await p.keyboard.press("3");
  await wait(p, 1500);
  await p.keyboard.press("1");
  await wait(p, 1500);
});
await clip("blank-screen", "tips", crops.full, async (p) => {
  await startItem(p);
  await wait(p, 1200);
  await p.keyboard.press(".");
  await wait(p, 1500);
  await p.keyboard.press(".");
  await wait(p, 1500);
});
await clip("safe-area", "tips", crops.top, async (p) => {
  await startItem(p);
  await wait(p, 1000);
  await p.getByRole("button", { name: /more/i }).first().click();
  await wait(p, 900);
  await p.getByText(/safe area/i).first().click();
  await wait(p, 1500);
});

// Clips for the website docs only: the basic jobs, from the default demo service.
await clip("plan-to-present", "default", crops.full, async (p) => {
  await wait(p, 600);
  await p.getByText("Sunday service").first().click();
  await wait(p, 1500);
  await p.getByRole("button", { name: /Open for live/ }).click();
  await wait(p, 1500);
}, { open: null });
await clip("present-next", "default", crops.full, async (p) => {
  await startItem(p);
  await wait(p, 1200);
  for (let i = 0; i < 3; i++) { await p.keyboard.press(" "); await wait(p, 1100); }
}, { open: "Sunday service" });
await clip("setup-screens", "default", crops.full, async (p) => {
  await wait(p, 600);
  await p.getByText("Setup").first().click();
  await wait(p, 1800);
}, { open: null });
await browser.close();
