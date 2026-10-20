import { test } from "node:test";
import assert from "node:assert/strict";
import { droppedPercent, isConnected, notice, obsPill, pollImages, sceneNotice, upcomingScene } from "./obs.ts";
import type { ObsOutputView, ObsView } from "./api";

const connected = { Connected: { obs_version: "30.2.3", websocket_version: "5.5.2" } } as const;

function view(over: Partial<ObsView> = {}): ObsView {
  return {
    host: "localhost",
    port: 4455,
    has_password: false,
    connection: connected,
    problem: null,
    relevant: true,
    stream: null,
    record: null,
    scenes: [],
    program_scene: null,
    held: false,
    scene_problem: null,
    audio: [],
    ...over,
  };
}

function output(over: Partial<ObsOutputView> = {}): ObsOutputView {
  return { active: false, reconnecting: false, timecode: "", skipped_frames: 0, total_frames: 0, ...over };
}

test("churches that don't stream never see OBS", () => {
  const quiet = view({ relevant: false, connection: { Disconnected: { reason: "x" } }, problem: "NotRunning" });
  assert.equal(obsPill(quiet), null);
  assert.equal(notice(quiet), null);
  assert.equal(obsPill(null), null);
});

test("connected and idle is ready", () => {
  const pill = obsPill(view({ stream: output(), record: output() }));
  assert.equal(pill?.tone, "ready");
  assert.equal(pill?.text, "OBS ready");
  assert.match(pill!.title, /30\.2\.3/);
});

test("streaming shows the timecode, and recording too", () => {
  const stream = output({ active: true, timecode: "00:12:34", skipped_frames: 3, total_frames: 45000 });
  const pill = obsPill(view({ stream, record: output({ active: true, timecode: "00:10:00" }) }));
  assert.equal(pill?.tone, "live");
  assert.equal(pill?.text, "LIVE 00:12:34 · REC 00:10:00");
  assert.match(pill!.title, /3 of 45000 frames dropped/);
});

test("dropping frames turns the pill to a warning", () => {
  const stream = output({ active: true, timecode: "00:01:00", skipped_frames: 90, total_frames: 3600 });
  const pill = obsPill(view({ stream }));
  assert.equal(pill?.tone, "warn");
  assert.equal(pill?.text, "LIVE 00:01:00 · 2.5% dropped");
});

test("a reconnecting stream says so", () => {
  const pill = obsPill(view({ stream: output({ active: true, reconnecting: true, timecode: "00:05:00" }) }));
  assert.equal(pill?.tone, "warn");
  assert.equal(pill?.text, "Stream reconnecting");
});

test("recording alone is gold", () => {
  const pill = obsPill(view({ stream: output(), record: output({ active: true, timecode: "00:00:42" }) }));
  assert.deepEqual([pill?.tone, pill?.text], ["gold", "REC 00:00:42"]);
});

test("each problem says what to do", () => {
  const down = { Disconnected: { reason: "nothing is listening" } } as const;
  const cases = [
    ["NotRunning", "open", /isn't running/],
    ["NotInstalled", "get", /obsproject\.com/],
    ["ServerOff", "setup", /WebSocket server is off/],
    ["PasswordRequired", "setup", /password/],
    ["WrongPassword", "setup", /refused/],
    ["Unreachable", "setup", /localhost:4455 doesn't answer/],
  ] as const;
  for (const [problem, action, message] of cases) {
    const n = notice(view({ connection: down, problem }));
    assert.equal(n?.action, action, problem);
    assert.match(n!.message, message, problem);
    assert.deepEqual([obsPill(view({ connection: down, problem }))?.tone], ["warn"]);
  }
  assert.equal(notice(view({ connection: down, problem: "ServerOff" }))?.steps.length, 4);
});

test("connecting is neutral and has no notice", () => {
  const v = view({ connection: "Connecting" });
  assert.equal(obsPill(v)?.tone, "neutral");
  assert.equal(notice(v), null);
});

test("dropped frames are a percentage to one decimal", () => {
  assert.equal(droppedPercent(output({ skipped_frames: 1, total_frames: 3 })), 33.3);
  assert.equal(droppedPercent(output()), null);
});

test("a missing scene warns and says what's on air", () => {
  const n = sceneNotice(view({ program_scene: "Worship", scene_problem: { scene: "Pastor", reason: "Missing" } }));
  assert.equal(n?.tone, "warn");
  assert.match(n!.message, /no scene “Pastor”, so it stayed on “Worship”/);
  assert.equal(n?.resume, false);
});

test("a scene waiting for OBS is quiet when OBS's own notice shows", () => {
  const waiting = { scene: "Sermon", reason: "NotConnected" } as const;
  const down = { Disconnected: { reason: "x" } } as const;
  assert.equal(sceneNotice(view({ connection: down, problem: "NotRunning", scene_problem: waiting })), null);
  assert.match(sceneNotice(view({ scene_problem: waiting }))!.message, /switches to “Sermon” when it connects/);
});

test("a failed switch shows OBS's reason", () => {
  const n = sceneNotice(view({ scene_problem: { scene: "Sermon", reason: { Failed: { message: "OBS did not answer in time" } } } }));
  assert.match(n!.message, /didn't switch to “Sermon”: OBS did not answer in time/);
});

test("a hand pick offers Resume", () => {
  const n = sceneNotice(view({ held: true, program_scene: "Wide" }));
  assert.deepEqual([n?.tone, n?.resume], ["neutral", true]);
  assert.match(n!.message, /“Wide”, picked by hand/);
  assert.equal(sceneNotice(view()), null);
  assert.equal(sceneNotice(view({ relevant: false, held: true, program_scene: "Wide" })), null);
});

test("the strip names the next item's scene, unless OBS is on it already", () => {
  const order = ["Countdown", null, "Sermon", " Worship "];
  assert.equal(upcomingScene(order, -1, "Wide"), "Countdown", "before the service, the first item");
  assert.equal(upcomingScene(order, 0, "Countdown"), null, "the next item names no scene");
  assert.equal(upcomingScene(order, 1, "Worship"), "Sermon");
  assert.equal(upcomingScene(order, 2, "Worship"), null, "already on air");
  assert.equal(upcomingScene(order, 3, "Sermon"), null, "the end of the service");
});

test("connected means connected", () => {
  assert.equal(isConnected(view()), true);
  assert.equal(isConnected(view({ connection: "Connecting" })), false);
  assert.equal(isConnected(null), false);
});

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

test("the live preview asks again only after each picture arrives, until stopped", async () => {
  let asked = 0;
  let inFlight = 0;
  let most = 0;
  const shown: string[] = [];
  const stop = pollImages(
    async () => {
      asked++;
      inFlight++;
      most = Math.max(most, inFlight);
      await sleep(15);
      inFlight--;
      return `frame ${asked}`;
    },
    (image) => shown.push(image),
    5,
  );
  await sleep(120);
  stop();
  const after = asked;
  await sleep(60);
  assert.ok(after >= 3, `asked ${after} times`);
  assert.equal(most, 1, "never two at once");
  assert.ok(asked <= after + 1, "stops asking once stopped");
  assert.deepEqual(shown.slice(0, 2), ["frame 1", "frame 2"]);
});

test("a failed picture is reported and the preview keeps trying", async () => {
  let asked = 0;
  const errors: unknown[] = [];
  const shown: string[] = [];
  const stop = pollImages(
    async () => {
      asked++;
      if (asked === 1) throw new Error("OBS isn't connected");
      return "frame";
    },
    (image) => shown.push(image),
    5,
    (e) => errors.push(e),
  );
  await sleep(40);
  stop();
  assert.equal(errors.length, 1);
  assert.ok(shown.length >= 1);
});
