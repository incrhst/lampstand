// T100: Lampstand Remote, at phone size. `cargo run --example remote_fixture`
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

const what = (page: Page) => page.locator(".now .what");
const coming = (page: Page) => page.locator(".next .about p");

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

test("Next and Back move the slide, and now and next follow", async ({ page }) => {
  await connect(page);
  await expect(what(page)).toHaveText("Nothing on screen yet. Next shows the first slide.");
  await expect(coming(page)).toHaveText("Amazing Grace · V1");
  await page.getByRole("button", { name: "Next" }).click();
  await expect(what(page)).toHaveText("Amazing Grace · V1");
  await expect(page.locator(".now .screen")).toContainText("Amazing Grace, V1, line one");
  await expect(coming(page)).toHaveText("Amazing Grace · V2");
  await page.getByRole("button", { name: "Next" }).click();
  await expect(what(page)).toHaveText("Amazing Grace · V2");
  await page.getByRole("button", { name: "Back" }).click();
  await expect(what(page)).toHaveText("Amazing Grace · V1");
  // The big buttons are well over 44 pt, for a thumb.
  const next = await page.getByRole("button", { name: "Next" }).boundingBox();
  expect(next!.height).toBeGreaterThan(80);
});

test("a change made on the laptop shows on the phone", async ({ page }) => {
  await connect(page);
  const before = await what(page).textContent();
  await say("next", "MOVED");
  await expect(what(page)).not.toHaveText(before ?? "");
});

test("Black toggles and says so", async ({ page }) => {
  await connect(page);
  const black = page.getByRole("button", { name: "Black" });
  await black.click();
  await expect(black).toHaveAttribute("aria-pressed", "true");
  await expect(what(page)).toHaveText("The screen is black");
  await black.click();
  await expect(black).toHaveAttribute("aria-pressed", "false");
});

test("the Sound box plays, pauses and stops", async ({ page }) => {
  await connect(page);
  await page.getByRole("button", { name: "Play Walk-out music" }).click();
  await expect(page.locator(".sound .playing")).toContainText("Playing");
  await page.getByRole("button", { name: "Pause" }).click();
  await expect(page.locator(".sound .playing")).toContainText("Paused");
  await page.getByRole("button", { name: "Stop the sound now" }).click();
  await expect(page.locator(".sound .playing")).toContainText("Nothing is playing.");
});

test("a revoked phone is sent back to the PIN form", async ({ page }) => {
  await connect(page);
  await say("revoke", "REVOKED");
  await expect(page.getByText("This remote was disconnected on the laptop.")).toBeVisible();
  await expect(page.getByPlaceholder("000 000")).toBeVisible();
});
