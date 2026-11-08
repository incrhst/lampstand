import { test } from "node:test";
import assert from "node:assert/strict";
import { sharingNote, waitingText } from "./church.ts";

const w = (what: string, from = "Mary's laptop") => ({ what, from });

test("this device only when nothing is paired", () => {
  assert.match(sharingNote(0, { shared: false }), /^This device only/);
});

test("shared once paired and agreed", () => {
  assert.equal(sharingNote(1, { shared: true }), "Shared with your other devices.");
});

test("honest before this device has met a device with settings", () => {
  assert.match(sharingNote(2, { shared: false }), /once this device has met/);
});

test("nothing awaited gives null", () => {
  assert.equal(waitingText([]), null);
  assert.equal(waitingText([w("logo")], ["idle video"]), null);
});

test("waiting names the file and where it comes from", () => {
  assert.equal(waitingText([w("idle video")]), "Waiting for the idle video from Mary's laptop");
  assert.equal(waitingText([w("logo"), w("idle picture")]), "Waiting for the logo and idle picture from Mary's laptop");
  assert.equal(waitingText([w("logo"), w("idle video")], ["idle video"]), "Waiting for the idle video from Mary's laptop");
});
