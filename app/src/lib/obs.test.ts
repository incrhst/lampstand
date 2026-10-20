import { test } from "node:test";
import assert from "node:assert/strict";
import { droppedPercent, notice, obsPill } from "./obs.ts";
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
