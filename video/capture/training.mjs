// Takes the screenshots the training videos (#190) use, from the real UI
// with the fake bridge: run `npm run capture:serve` first, then
// `npm run capture:training`. Writes public/shots/training/*.png.
// Each one is a scripted run of Present or Plan on the mock's training
// services, so a UI change is a retake, not a reshoot.
import { chromium } from "playwright-core";
import { mkdirSync } from "node:fs";

const APP = "http://localhost:1431/";
const OUT = new URL("../public/shots/training/", import.meta.url).pathname;
mkdirSync(OUT, { recursive: true });
const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const browser = await chromium.launch({ executablePath: chrome });

async function shot(name, script) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 2 });
  const p = await ctx.newPage();
  await p.addInitScript(() => {
    window.__scene = "training";
    localStorage.setItem("lampstand.getstarted.hidden", "1");
    // The quick tips have been read: the order of service shows in full.
    localStorage.setItem("lampstand.tips.dismissed", JSON.stringify(["loop-group", "loop-progress", "fold-group", "quick-add", "jump-keys", "blank-screen", "logo-bug", "safe-area"]));
  });
  p.on("pageerror", (e) => console.log(name, "pageerror", e.message));
  await p.goto(APP);
  await p.waitForTimeout(700);
  await script(p);
  await p.waitForTimeout(500);
  await p.screenshot({ path: `${OUT}${name}.png` });
  console.log("shot", name);
  await ctx.close();
}

const present = async (p) => {
  await p.getByRole("button", { name: "Present", exact: true }).first().click();
  await p.waitForTimeout(300);
};
/** Opens a training service for live from Present's list. */
const openTraining = async (p, title) => {
  await present(p);
  await p.getByRole("button", { name: `Open ${title} for live` }).click();
  await p.waitForTimeout(400);
};
/** Shift+Down `n` times, then Enter, then `spaces` presses of Space. */
const go = async (p, n, spaces = 0) => {
  for (let i = 0; i < n; i++) await p.keyboard.press("Shift+ArrowDown");
  await p.keyboard.press("Enter");
  await p.waitForTimeout(200);
  for (let i = 0; i < spaces; i++) await p.keyboard.press(" ");
  await p.waitForTimeout(200);
};

const only = process.argv[2];
const shots = {
  // Present with nothing open: the church's services, then Training.
  "pick": present,
  // Plan with a training service chosen: read-only, Reset training.
  "plan": async (p) => {
    await p.getByRole("button", { name: "Plan", exact: true }).first().click();
    await p.getByText("Training 1: Run a service").first().click();
  },
  // Training 1, Amazing Grace on air.
  "song": async (p) => { await openTraining(p, "Training 1: Run a service"); await go(p, 3, 1); },
  // Training 1, the sermon deck with its builds and notes.
  "deck": async (p) => { await openTraining(p, "Training 1: Run a service"); await go(p, 8, 2); },
  // The same, blanked with the full stop.
  "blank": async (p) => { await openTraining(p, "Training 1: Run a service"); await go(p, 8, 2); await p.keyboard.press("."); },
  // Training 2: Shift+Down has chosen Amazing Grace while Holy, Holy, Holy is on air.
  "choose": async (p) => {
    await openTraining(p, "Training 2: Confident operator");
    await go(p, 1);
    await p.keyboard.press("Shift+ArrowDown");
  },
  // Training 2: Quick add with Romans 8:28 typed.
  "quickadd": async (p) => {
    await openTraining(p, "Training 2: Confident operator");
    await go(p, 3);
    await p.keyboard.press("/");
    await p.keyboard.type("rom 8:28", { delay: 20 });
    await p.waitForTimeout(400);
  },
};
for (const [n, fn] of Object.entries(shots)) if (!only || n.includes(only)) await shot(n, fn);
await browser.close();
