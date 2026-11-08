// Takes the phone shot of Lampstand Remote the hero tour shows: the real page
// at phone size, paired with the server's stand-in for the app
// (crates/lampstand-server/examples/remote_fixture.rs). Writes
// public/shots/9-remote.png. `npm run capture:remote`; set
// LAMPSTAND_REMOTE_FIXTURE to a built fixture to skip `cargo run`.
import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../..", import.meta.url));
const OUT = new URL("../public/shots/9-remote.png", import.meta.url).pathname;
const bin = process.env.LAMPSTAND_REMOTE_FIXTURE;
const child = bin
  ? spawn(bin, [], { cwd: root })
  : spawn("cargo", ["run", "-q", "-p", "lampstand-server", "--example", "remote_fixture"], { cwd: root });
child.stderr.on("data", (d) => process.stderr.write(d));
let exited = false;
child.on("exit", () => (exited = true));

let buffer = "";
const lines = [];
child.stdout.on("data", (d) => {
  buffer += String(d);
  const parts = buffer.split("\n");
  buffer = parts.pop() ?? "";
  lines.push(...parts);
});
async function line(prefix) {
  for (let i = 0; i < 6000; i++) {
    const at = lines.findIndex((l) => l.startsWith(prefix));
    if (at >= 0) return lines.splice(at, 1)[0];
    if (exited) throw new Error(`fixture: exited before ${prefix}`);
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`fixture: no ${prefix}`);
}
const say = (command, answer) => (child.stdin.write(`${command}\n`), line(answer));

let browser;
try {
  const base = `http://127.0.0.1:${(await line("READY")).split(" ")[1]}`;
  const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
  browser = await chromium.launch({ executablePath: chrome });
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, deviceScaleFactor: 2 });
  const p = await ctx.newPage();
  await p.goto(`${base}/remote`);
  await p.getByText("Type the PIN shown on the laptop.").waitFor();
  const pin = (await say("pin", "PIN")).split(" ")[1];
  await p.getByPlaceholder("000 000").fill(`${pin.slice(0, 3)} ${pin.slice(3)}`);
  await p.getByRole("button", { name: "Connect" }).click();
  await p.getByRole("status").filter({ hasText: "Connected" }).waitFor();
  // Into the song, as on a Sunday, with its backing track playing (the stand-in's track waits for Play).
  await say("next", "MOVED");
  await p.getByRole("button", { name: /^Play/ }).first().click();
  await p.waitForTimeout(800);
  // The stand-in's lyrics are placeholders ("Amazing Grace, V1, line one"): show the hymn's real words,
  // under the service name the other shots use.
  await p.evaluate(() => {
    const real = {
      V1: ["Amazing grace, how sweet the sound", "That saved a wretch like me"],
      V2: ["'Twas grace that taught my heart to fear", "And grace my fears relieved"],
    };
    const walk = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    for (let n = walk.nextNode(); n; n = walk.nextNode()) {
      const m = n.textContent.match(/^Amazing Grace, (V\d), line one$/);
      if (m && real[m[1]]) n.textContent = real[m[1]][0];
      else if (n.textContent === "and line two") {
        n.textContent = real[n.parentElement?.closest("[class*=next]") ? "V2" : "V1"][1];
      } else if (n.textContent === "Sunday morning") n.textContent = "Sunday service";
    }
  });
  await p.screenshot({ path: OUT });
  console.log("shot 9-remote");
} finally {
  await browser?.close();
  child.kill();
}
