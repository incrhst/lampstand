// Retakes the window screenshots the video flies over. Needs the capture
// server running (npm run capture:serve) and Google Chrome installed.
import { chromium } from "playwright-core";
import { mkdirSync } from "node:fs";

const APP = "http://localhost:1431/";
const OUT = new URL("../public/shots/", import.meta.url).pathname;
mkdirSync(OUT, { recursive: true });
const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const browser = await chromium.launch({ executablePath: chrome });

async function shot(name, scene, script, storage = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 2 });
  const p = await ctx.newPage();
  await p.addInitScript(([s, storage]) => {
    window.__scene = s;
    if (s !== "fresh") localStorage.setItem("lampstand.getstarted.hidden", "1");
    // The quick tips have been read, so the wall shows in full.
    if (s !== "fresh") localStorage.setItem("lampstand.tips.dismissed", JSON.stringify(["loop-group", "loop-progress", "fold-group", "quick-add", "jump-keys", "blank-screen", "logo-bug", "safe-area"]));
    for (const [k, v] of Object.entries(storage)) localStorage.setItem(k, v);
  }, [scene, storage]);
  p.on("pageerror", (e) => console.log(name, "pageerror", e.message));
  p.on("console", (m) => m.type() === "warning" && console.log(name, m.text().slice(0, 160)));
  await p.goto(APP);
  await p.waitForTimeout(700);
  await script(p);
  await p.waitForTimeout(500);
  await p.screenshot({ path: `${OUT}${name}.png` });
  console.log("shot", name);
  await ctx.close();
}

const only = process.argv[2];
const shots = {
  "1-getstarted": ["fresh", async () => {}],
  "2-plan": ["default", async (p) => { await p.getByText("Sunday service").first().click(); }],
  "3-quickadd": ["default", async (p) => {
    await p.getByText("Sunday service").first().click();
    await p.keyboard.press("/"); await p.keyboard.type("john 3:16", { delay: 20 });
  }],
  "4-decks": ["default", async (p) => {
    await p.getByText("Sunday service").first().click();
    await p.getByRole("button", { name: "Decks" }).click();
  }],
  "5-present": ["default", async (p) => {
    await p.getByText("Sunday service").first().click();
    await p.getByRole("button", { name: /Open for live/ }).click();
    await p.waitForTimeout(400);
    await p.keyboard.press("Shift+ArrowDown"); await p.keyboard.press("Enter");
    await p.waitForTimeout(200); await p.keyboard.press(" ");
  }],
  // The sermon deck one click into slide 2's builds: the builds panel shows.
  "5b-present-deck": ["default", async (p) => {
    await p.getByText("Sunday service").first().click();
    await p.getByRole("button", { name: /Open for live/ }).click();
    await p.waitForTimeout(400);
    for (let i = 0; i < 4; i++) await p.keyboard.press("Shift+ArrowDown");
    await p.keyboard.press("Enter"); await p.waitForTimeout(200);
    await p.keyboard.press(" "); await p.keyboard.press(" ");
  }],
  // Focus (F): the cards only, with the pill.
  "5c-focus": ["default", async (p) => {
    await p.getByText("Sunday service").first().click();
    await p.getByRole("button", { name: /Open for live/ }).click();
    await p.waitForTimeout(400);
    await p.keyboard.press("Shift+ArrowDown"); await p.keyboard.press("Enter");
    await p.waitForTimeout(200); await p.keyboard.press(" "); await p.keyboard.press(" ");
    await p.keyboard.press("f");
  }],
  // Sound: Next has started Amazing Grace and its backing track; the Sound box open, then folded.
  ...Object.fromEntries([["5d-sound", "1"], ["5e-sound-folded", "0"]].map(([n, open]) => [n, ["default", async (p) => {
    await p.evaluate(() => window.__soundOn());
    await p.getByText("Sunday service").first().click();
    await p.getByRole("button", { name: /Open for live/ }).click();
    await p.waitForTimeout(400);
    await p.keyboard.press("Shift+ArrowDown"); await p.keyboard.press("Enter");
  }, { "lampstand.sound.open.during": open }]])),
  "6-setup": ["default", async (p) => { await p.getByText("Setup").first().click(); }],
  "7-obs": ["default", async (p) => {
    await p.evaluate(() => window.__obsOn());
    await p.getByText("Setup").first().click(); await p.getByText("Send pictures, control scenes").click();
  }],
  "8-present-obs": ["default", async (p) => {
    await p.evaluate(() => window.__obsOn());
    await p.getByText("Sunday service").first().click();
    await p.getByRole("button", { name: /Open for live/ }).click();
    await p.waitForTimeout(400); await p.keyboard.press("Shift+ArrowDown"); await p.keyboard.press("Enter");
  }],
};
for (const [n, [scene, fn, storage]] of Object.entries(shots)) if (!only || n.includes(only)) await shot(n, scene, fn, storage);
await browser.close();
