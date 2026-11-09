// Takes the screenshots the training videos (#190) use, from the real UI
// with the fake bridge: run `npm run capture:serve` first, then
// `npm run capture:training` (set APP to the server's address if it is not
// port 1431). Writes public/shots/training/*.png.
// Each one is a scripted run of Present or Plan on the mock's training
// services, so a UI change is a retake, not a reshoot.
import { chromium } from "playwright-core";
import { mkdirSync, writeFileSync } from "node:fs";

const APP = process.env.APP ?? "http://localhost:1431/";
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
    localStorage.setItem("lampstand.tips.dismissed", JSON.stringify(["loop-group", "loop-progress", "fold-group", "quick-add", "jump-keys", "blank-screen", "logo-bug", "safe-area", "connect-device"]));
  });
  p.on("pageerror", (e) => console.log(name, "pageerror", e.message));
  await p.goto(APP);
  await p.waitForTimeout(700);
  // "Start here" points at the Plan | Present tabs at every launch; close it.
  const point = p.getByRole("button", { name: "Close Start here" });
  if (await point.count()) await point.click();
  await script(p);
  await p.waitForTimeout(500);
  await p.screenshot({ path: `${OUT}${name}.png` });
  // Where the controls a callout points at sit, as percentages of the
  // picture, so the video scripts' callouts can follow a layout change.
  const boxes = await p.evaluate((marks) => {
    const out = {};
    for (const [k, sel] of Object.entries(marks)) {
      const el = document.querySelector(sel);
      if (!el) continue;
      const r = el.getBoundingClientRect();
      if (!r.width) continue;
      out[k] = { x: +(((r.left + r.width / 2) / innerWidth) * 100).toFixed(1), y: +(((r.top + r.height / 2) / innerHeight) * 100).toFixed(1) };
    }
    return out;
  }, MARKS);
  writeFileSync(`${OUT}${name}.boxes.json`, JSON.stringify(boxes, null, 1));
  console.log("shot", name);
  await ctx.close();
}

const MARKS = {
  monitor: ".col-onair .monitor-screen",
  nextLine: ".look-next",
  steps: ".wall-steps-head",
  check: ".col-onair .check",
  size: ".size-toggle",
  previous: ".col-press .transport > :first-child",
  next: ".col-press .transport > :last-child",
  idle: ".screen-pick:not(.blank):not(.clear-text)",
  blank: ".screen-pick.blank",
  clear: ".screen-pick.clear-text",
  addReady: ".add-ready",
  press: ".col-press",
  live: ".slide.live",
  nextCard: ".slide.next",
  viewed: ".wall-row.viewed .wall-label",
  notes: ".wall-notes",
  add: ".wall-add",
  quickAdd: "[aria-modal='true'] input",
};

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
    await p.waitForTimeout(300);
    // The services list is a page inside Library & controls.
    await p.locator("#library-and-controls").getByRole("button", { name: /^Services/ }).first().click();
    await p.getByText("Training Session 1: Run a service").first().click();
  },
  // Training Session 1, Amazing Grace on air.
  "song": async (p) => { await openTraining(p, "Training Session 1: Run a service"); await go(p, 3, 1); },
  // The same: the right column, look up at On air, press down below it.
  "right": async (p) => { await openTraining(p, "Training Session 1: Run a service"); await go(p, 3, 1); },
  // The same with the right column narrower, so the slides get the room.
  "narrow": async (p) => {
    await openTraining(p, "Training Session 1: Run a service");
    await go(p, 3, 1);
    await p.getByRole("button", { name: "Narrower" }).click();
  },
  // The same with Idle chosen: the idle screen is up, the service stays open.
  "idle": async (p) => {
    await openTraining(p, "Training Session 1: Run a service");
    await go(p, 3, 1);
    // Idle is the first tile under Next.
    await p.locator("#ready-tray .screen-pick").filter({ hasText: "Idle" }).first().click();
  },
  // Training Session 1, the sermon deck one click into slide 2's builds: the builds panel shows.
  "deck": async (p) => { await openTraining(p, "Training Session 1: Run a service"); await go(p, 8, 2); },
  // The same, blanked with the full stop.
  "blank": async (p) => { await openTraining(p, "Training Session 1: Run a service"); await go(p, 8, 2); await p.keyboard.press("."); },
  // Training Session 2: Shift+Down has chosen Amazing Grace while Holy, Holy, Holy is on air.
  "choose": async (p) => {
    await openTraining(p, "Training Session 2: Confident operator");
    await go(p, 1);
    await p.keyboard.press("Shift+ArrowDown");
  },
  // Training Session 2: Quick add with Romans 8:28 typed.
  "quickadd": async (p) => {
    await openTraining(p, "Training Session 2: Confident operator");
    await go(p, 3);
    await p.keyboard.press("/");
    await p.keyboard.type("rom 8:28", { delay: 20 });
    await p.waitForTimeout(400);
  },
};
for (const [n, fn] of Object.entries(shots)) if (!only || n.includes(only)) await shot(n, fn);
await browser.close();
