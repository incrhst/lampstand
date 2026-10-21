// node --test --experimental-strip-types (npm test)
import { test } from "node:test";
import assert from "node:assert/strict";
import { thumbnailLoader } from "./mediaThumbs.ts";

test("a thumbnail is asked for once however many rows show it", async () => {
  let asked = 0;
  const load = thumbnailLoader(async (blob) => {
    asked++;
    return `data:${blob}`;
  });
  const [a, b] = await Promise.all([load("x", "x.mp4", 96), load("x", "x.mp4", 96)]);
  assert.equal(a, "data:x");
  assert.equal(b, "data:x");
  assert.equal(asked, 1);
  assert.equal(await load("x", "x.mp4", 96), "data:x");
  assert.equal(asked, 1, "remembered");
  await load("x", "x.mp4", 200);
  assert.equal(asked, 2, "another size is another picture");
});

test("a failure is remembered as nothing, not asked again", async () => {
  let asked = 0;
  const load = thumbnailLoader(async () => {
    asked++;
    throw new Error("not on this device");
  });
  assert.equal(await load("gone", "a.mp4", 96), null);
  assert.equal(await load("gone", "a.mp4", 96), null);
  assert.equal(asked, 1);
});

test("only a few are asked for at once", async () => {
  let running = 0;
  let most = 0;
  const releases: (() => void)[] = [];
  const load = thumbnailLoader(
    (blob) =>
      new Promise<string>((resolve) => {
        running++;
        most = Math.max(most, running);
        releases.push(() => {
          running--;
          resolve(`data:${blob}`);
        });
      }),
    2,
  );
  const all = ["a", "b", "c", "d", "e"].map((b) => load(b, `${b}.mp4`, 96));
  await new Promise((r) => setTimeout(r, 5));
  assert.equal(running, 2, "two at a time");
  while (releases.length) {
    releases.shift()!();
    await new Promise((r) => setTimeout(r, 1));
  }
  assert.deepEqual(await Promise.all(all), ["data:a", "data:b", "data:c", "data:d", "data:e"]);
  assert.equal(most, 2);
});
