// Retakes the window screenshots the video flies over. Needs the capture
// server running (npm run capture:serve) and Google Chrome installed.
import { chromium } from "playwright-core";
import { mkdirSync } from "node:fs";

const APP = process.env.APP ?? "http://localhost:1431/";
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
    if (s !== "fresh") localStorage.setItem("lampstand.tips.dismissed", JSON.stringify(["loop-group", "loop-progress", "fold-group", "quick-add", "jump-keys", "blank-screen", "logo-bug", "safe-area", "connect-device"]));
    for (const [k, v] of Object.entries(storage)) localStorage.setItem(k, v);
  }, [scene, storage]);
  p.on("pageerror", (e) => console.log(name, "pageerror", e.message));
  p.on("console", (m) => m.type() === "warning" && console.log(name, m.text().slice(0, 160)));
  await p.goto(APP);
  await p.waitForTimeout(700);
  // "Start here" points at the Plan | Present tabs at every launch; close it.
  const point = p.getByRole("button", { name: "Close Start here" });
  if (await point.count()) await point.click();
  await script(p);
  await p.waitForTimeout(500);
  await p.screenshot({ path: `${OUT}${name}.png` });
  console.log("shot", name);
  await ctx.close();
}

/** Plan's services list is a page inside Library & controls; choose the Sunday service there. */
const openSunday = async (p) => {
  await p.locator("#library-and-controls").getByRole("button", { name: /^Services/ }).first().click();
  await p.waitForTimeout(300);
  // The mock's Sunday service has a fixed date, so it may be among the older ones.
  const older = p.getByRole("button", { name: /^Show \d+ older/ }).locator("visible=true");
  if (await older.count()) await older.first().click();
  await p.getByText("Sunday service").locator("visible=true").first().click();
  await p.waitForTimeout(300);
};

const only = process.argv[2];
const shots = {
  "1-getstarted": ["fresh", async () => {}],
  "2-plan": ["default", async (p) => { await openSunday(p); }],
  "3-quickadd": ["default", async (p) => {
    await openSunday(p);
    await p.keyboard.press("/"); await p.keyboard.type("john 3:16", { delay: 20 });
  }],
  "4-decks": ["default", async (p) => {
    await openSunday(p);
    // The decks are a filter in the Library, a page inside Library & controls.
    await p.locator("#library-and-controls").getByRole("button", { name: "Back", exact: true }).first().click();
    await p.waitForTimeout(200);
    await p.locator("#library-and-controls").getByText("Browse the library").first().click();
    await p.waitForTimeout(300);
    await p.getByRole("button", { name: /^Decks/ }).first().click();
  }],
  "5-present": ["default", async (p) => {
    await p.evaluate(() => window.__obsQuiet());
    await openSunday(p);
    await p.getByRole("button", { name: /Open for live/ }).click();
    await p.waitForTimeout(400);
    await p.keyboard.press("Shift+ArrowDown"); await p.keyboard.press("Enter");
    await p.waitForTimeout(200); await p.keyboard.press(" ");
  }],
  // The sermon deck one click into slide 2's builds: the builds panel shows.
  "5b-present-deck": ["default", async (p) => {
    await p.evaluate(() => window.__obsQuiet());
    await openSunday(p);
    await p.getByRole("button", { name: /Open for live/ }).click();
    await p.waitForTimeout(400);
    for (let i = 0; i < 4; i++) await p.keyboard.press("Shift+ArrowDown");
    await p.keyboard.press("Enter"); await p.waitForTimeout(200);
    await p.keyboard.press(" "); await p.keyboard.press(" ");
  }],
  // Look up, press down: a song on air, one item added to Ready, Idle, Blank and Clear text under Next.
  "5c-press": ["default", async (p) => {
    await p.evaluate(() => window.__obsQuiet());
    await openSunday(p);
    await p.getByRole("button", { name: /Open for live/ }).click();
    await p.waitForTimeout(400);
    await p.keyboard.press("Shift+ArrowDown"); await p.keyboard.press("Enter");
    await p.waitForTimeout(200); await p.keyboard.press(" ");
    // Add John 3:16 to Ready from its row on the wall.
    await p.locator(".wall-row").filter({ hasText: "John 3:16" }).locator(".pin").first().click();
    await p.mouse.move(0, 0);
  }],
  // Sound: Next has started Amazing Grace and its backing track; Stop the sound now sits under the tiles.
  "5d-sound": ["default", async (p) => {
    await p.evaluate(() => window.__obsQuiet());
    await p.evaluate(() => window.__soundOn());
    await openSunday(p);
    await p.getByRole("button", { name: /Open for live/ }).click();
    await p.waitForTimeout(400);
    await p.keyboard.press("Shift+ArrowDown"); await p.keyboard.press("Enter");
  }],
  "6-setup": ["default", async (p) => { await p.getByRole("button", { name: "Settings" }).first().click(); }],
  "7-obs": ["default", async (p) => {
    await p.evaluate(() => window.__obsOn());
    await p.getByRole("button", { name: "Settings" }).first().click(); await p.getByText("Send pictures, control scenes").click();
  }],
  "8-present-obs": ["default", async (p) => {
    await p.evaluate(() => window.__obsOn());
    await openSunday(p);
    await p.getByRole("button", { name: /Open for live/ }).click();
    await p.waitForTimeout(400); await p.keyboard.press("Shift+ArrowDown"); await p.keyboard.press("Enter");
  }],
};
for (const [n, [scene, fn, storage]] of Object.entries(shots)) if (!only || n.includes(only)) await shot(n, scene, fn, storage);
await browser.close();
