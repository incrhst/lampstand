// T100 and 0.9.1: Lampstand Remote at phone size (and a tablet), and a speaker's phone. `cargo run --example remote_fixture`
// serves the real page and routes with a stand-in for the app. Each test
// pairs with a fresh PIN, then presses the page's buttons and checks that
// what is on screen follows, including a change made on the laptop.

import { expect, test, type Cookie, type Page } from "@playwright/test";
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../../../..");

let child: ChildProcessWithoutNullStreams | undefined;
let base = "";
let lines: string[] = [];
let waiting: Array<() => boolean> = [];

function nextLine(match: (l: string) => boolean, ms = 600_000): Promise<string> {
  return new Promise((resolveLine, reject) => {
    const timer = setTimeout(() => reject(new Error("fixture: timed out")), ms);
    const check = () => {
      const i = lines.findIndex(match);
      if (i < 0) return false;
      clearTimeout(timer);
      resolveLine(lines.splice(i, 1)[0]);
      return true;
    };
    if (!check()) waiting.push(() => check());
  });
}

async function say(command: string, answer: string): Promise<string> {
  child!.stdin.write(`${command}\n`);
  return nextLine((l) => l.startsWith(answer), 10_000);
}

test.use({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, deviceScaleFactor: 2 });

test.beforeAll(async () => {
  test.setTimeout(900_000);
  const bin = process.env.LAMPSTAND_REMOTE_FIXTURE;
  child = bin
    ? spawn(bin, [], { cwd: root })
    : spawn("cargo", ["run", "-q", "-p", "lampstand-server", "--example", "remote_fixture"], { cwd: root });
  let buffer = "";
  child.stdout.on("data", (d) => {
    buffer += String(d);
    const parts = buffer.split("\n");
    buffer = parts.pop() ?? "";
    lines.push(...parts);
    waiting = waiting.filter((w) => !w());
  });
  child.stderr.on("data", (d) => process.stderr.write(d));
  const ready = await nextLine((l) => l.startsWith("READY"));
  base = `http://127.0.0.1:${ready.split(" ")[1]}`;
});

test.afterAll(() => {
  child?.kill();
});

/** The session cookie from the first pairing. Each address may try five PINs a minute, so the tests pair once and share it. */
let session: Cookie[] = [];

async function pair(page: Page) {
  await page.goto(`${base}/remote`);
  await expect(page.getByText("Type the PIN shown on the laptop.")).toBeVisible();
  const pin = (await say("pin", "PIN")).split(" ")[1];
  await page.getByPlaceholder("000 000").fill(`${pin.slice(0, 3)} ${pin.slice(3)}`);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.getByRole("status").filter({ hasText: "Connected" })).toBeVisible();
  session = await page.context().cookies();
}

/** Opens the remote as a paired phone. */
async function connect(page: Page) {
  if (session.length === 0) return pair(page);
  await page.context().addCookies(session);
  await page.goto(`${base}/remote`);
  await expect(page.getByRole("status").filter({ hasText: "Connected" })).toBeVisible();
}

/** What the top strip says: LIVE, IDLE or BLANK, the item and its slide. */
const said = (page: Page) => page.locator(".said");
const card = (page: Page, name: string) => page.getByRole("button", { name, exact: true });

test("a wrong PIN is refused and the right one pairs", async ({ page }) => {
  await page.goto(`${base}/remote`);
  const pin = (await say("pin", "PIN")).split(" ")[1];
  const wrong = String((Number(pin) + 1) % 1_000_000).padStart(6, "0");
  await page.getByPlaceholder("000 000").fill(wrong);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.getByRole("alert").filter({ hasText: "not right" })).toBeVisible();
  await page.getByPlaceholder("000 000").fill(pin);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.getByText("Sunday morning")).toBeVisible();
  session = await page.context().cookies();
  expect(session.find((c) => c.name === "lampstand_remote")?.httpOnly).toBe(true);
});

test("the phone sees the whole service as a wall", async ({ page }) => {
  await connect(page);
  await expect(page.getByRole("heading", { name: "Gathering" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Word" })).toBeVisible();
  await expect(page.locator(".row", { hasText: "Amazing Grace" })).toContainText("2 slides");
  // A music-only item is a dashed sound card.
  await expect(page.locator(".row", { hasText: "Walk-in" }).locator(".sound-card")).toContainText("Waits for Play");
  // Deck slides come as pictures from the laptop.
  await page.locator(".row", { hasText: "Notices" }).scrollIntoViewIfNeeded();
  await expect(card(page, "Notices · Slide 1").locator("img")).toHaveAttribute("src", /^data:image/);
  // Everything a thumb needs is at least 44 px; Next is the big one.
  for (const name of ["Back", "Blank", "Idle", "Stop the sound now"]) {
    const box = await page.getByRole("button", { name, exact: true }).boundingBox();
    expect(box!.height, name).toBeGreaterThanOrEqual(44);
  }
  const next = await page.locator(".drive .go").boundingBox();
  expect(next!.height).toBeGreaterThanOrEqual(64);
  // Nothing wider than the phone.
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
});

test("a tap selects a card, and Show this slide puts it on the screen", async ({ page }) => {
  await connect(page);
  const v1 = card(page, "Amazing Grace · V1");
  await v1.click();
  await expect(v1).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByText("Selected: Amazing Grace · V1")).toBeVisible();
  // Not on air yet.
  await expect(v1).not.toHaveAttribute("aria-current", "true");
  await page.getByRole("button", { name: "Show this slide" }).click();
  await expect(v1).toHaveAttribute("aria-current", "true");
  await expect(said(page)).toContainText("LIVE");
  await expect(said(page)).toContainText("Amazing Grace");
  await expect(page.getByText("Selected:")).toHaveCount(0);
  // Tapping a card twice shows it too; Cancel drops a selection.
  const v2 = card(page, "Amazing Grace · V2");
  await v2.click();
  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(v2).toHaveAttribute("aria-pressed", "false");
  await v2.click();
  await v2.click();
  await expect(v2).toHaveAttribute("aria-current", "true");
});

test("Next and Back move the slide, and a change on the laptop cancels a selection", async ({ page }) => {
  await connect(page);
  await say("show 2 0", "MOVED");
  await expect(said(page)).toContainText("V1");
  await page.getByRole("button", { name: /^Next/ }).click();
  await expect(said(page)).toContainText("V2");
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await expect(said(page)).toContainText("V1");
  await card(page, "Notices · Slide 1").click();
  await expect(page.getByText("Selected: Notices · Slide 1")).toBeVisible();
  await say("next", "MOVED");
  await expect(said(page)).toContainText("V2");
  await expect(page.getByText("Selected:")).toHaveCount(0);
});

test("a deck slide with builds says how many are left", async ({ page }) => {
  await connect(page);
  await say("show 4 1", "MOVED");
  await expect(page.locator(".drive .go")).toHaveText("Next · 2 left");
  await page.locator(".drive .go").click();
  await expect(said(page)).toContainText("build 1 of 2");
  await expect(page.locator(".drive .go")).toHaveText("Next · 1 left");
});

test("Blank toggles and says so", async ({ page }) => {
  await connect(page);
  const blank = page.getByRole("button", { name: "Blank", exact: true });
  await blank.click();
  await expect(blank).toHaveAttribute("aria-pressed", "true");
  await expect(said(page)).toContainText("BLANK");
  await blank.click();
  await expect(blank).toHaveAttribute("aria-pressed", "false");
});

test("the Sound box folds out; Stop the sound now stays in the bar", async ({ page }) => {
  await connect(page);
  await page.locator(".sound-handle").click();
  await page.getByRole("button", { name: /^Show all/ }).click();
  await page.getByRole("button", { name: "Play Walk-out music" }).click();
  await expect(page.locator(".sound .playing")).toContainText("Playing");
  await page.getByRole("button", { name: "Pause" }).click();
  await expect(page.locator(".sound .playing")).toContainText("Paused");
  await page.getByRole("button", { name: "Stop the sound now" }).click();
  await expect(page.locator(".sound .playing")).toContainText("Nothing is playing.");
});

test("on a tablet the rows become the desktop wall, with the controls along the top", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 768 });
  await connect(page);
  await expect(page.locator("main.remote.wide")).toBeVisible();
  await expect(page.getByRole("button", { name: "Previous" })).toBeVisible();
  const cards = page.locator(".row", { hasText: "Sermon" }).locator(".card");
  await expect(cards).toHaveCount(3);
  // A wrapping grid: all three cards in one row, side by side.
  const [a, b] = [await cards.nth(0).boundingBox(), await cards.nth(1).boundingBox()];
  expect(Math.abs(a!.y - b!.y)).toBeLessThan(2);
  // Touch still takes two taps.
  await cards.nth(2).click();
  await expect(page.getByText("Selected: Sermon · Slide 3")).toBeVisible();
});

test("a speaker's phone pages through its own item only", async ({ browser }) => {
  const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  const page = await context.newPage();
  await page.goto(`${base}/remote`);
  const pin = (await say("speaker", "PIN")).split(" ")[1];
  await page.getByPlaceholder("000 000").fill(pin);
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.getByText("Your slides", { exact: true })).toBeVisible();
  await say("show 2 0", "MOVED");
  await expect(page.getByText("Waiting for your slides")).toBeVisible();
  await expect(page.getByText("On the screen now: Amazing Grace · V1")).toBeVisible();
  await expect(page.getByRole("button", { name: "Next" })).toBeDisabled();

  await say("show 5 0", "MOVED");
  await expect(page.getByText("Slide 1 of 3")).toBeVisible();
  await expect(page.getByText("Welcome everyone.")).toBeVisible();
  await expect(page.getByRole("button", { name: "Back" })).toBeDisabled();
  await page.getByRole("button", { name: "Next" }).click();
  await expect(page.getByText("Slide 2 of 3 · build 0 of 1")).toBeVisible();
  await expect(page.getByText("1 click left on this slide")).toBeVisible();
  await page.getByRole("button", { name: "Next" }).click();
  await page.getByRole("button", { name: "Next" }).click();
  await expect(page.getByText("Slide 3 of 3")).toBeVisible();
  await expect(page.getByText("Last slide. The service team takes it from here.")).toBeVisible();
  await expect(page.getByRole("button", { name: "Next" })).toBeDisabled();
  const next = await page.getByRole("button", { name: "Next" }).boundingBox();
  expect(next!.height).toBeGreaterThanOrEqual(76);

  // Closing the service ends the speaker's phone.
  await say("close", "CLOSED");
  await expect(page.getByText("This remote was disconnected on the laptop.")).toBeVisible();
  await context.close();
});

test("a revoked phone is sent back to the PIN form", async ({ page }) => {
  await connect(page);
  await say("revoke", "REVOKED");
  await expect(page.getByText("This remote was disconnected on the laptop.")).toBeVisible();
  await expect(page.getByPlaceholder("000 000")).toBeVisible();
});
