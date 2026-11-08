import { test } from "node:test";
import assert from "node:assert/strict";
import { register } from "node:module";
import { TIP_KEY, loadOpen, openState, saveOpen, soundKey, soundOpen } from "./disclosure.ts";
import type { Tip } from "./tips.ts";

/** This computer's storage, as the app has it, standing in for the browser's. */
const store = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", {
  configurable: true,
  value: {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
    removeItem: (k: string) => void store.delete(k),
  },
});

// The kit components are rendered as the server would, to read the markup.
register("./svelteHooks.mjs", import.meta.url);
const { render } = await import("svelte/server");
const Disclosure = (await import("./kit/Disclosure.svelte")).default;
const TipCard = (await import("./kit/TipCard.svelte")).default;

const html = (component: unknown, props: Record<string, unknown>) =>
  render(component as Parameters<typeof render>[0], { props } as never).body;

test("a choice is remembered on this computer, and nothing chosen reads as the default", () => {
  store.clear();
  assert.equal(loadOpen("k"), null);
  assert.equal(openState("k", true), true);
  saveOpen("k", false);
  assert.equal(loadOpen("k"), false);
  assert.equal(openState("k", true), false, "the choice wins over the default");
  saveOpen("k", true);
  assert.equal(openState("k", false), true);
  store.set("k", "nonsense");
  assert.equal(loadOpen("k"), null);
});

test("storage that cannot be read or written changes nothing", () => {
  const was = globalThis.localStorage;
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
    },
  });
  try {
    assert.equal(openState("k", true), true);
    assert.doesNotThrow(() => saveOpen("k", false));
  } finally {
    Object.defineProperty(globalThis, "localStorage", { configurable: true, value: was });
  }
});

test("the Sound box is open before the service and folded once it is under way, unless chosen here", () => {
  store.clear();
  assert.equal(soundOpen(false), true);
  assert.equal(soundOpen(true), false);
  saveOpen(soundKey(true), true);
  assert.equal(soundOpen(true), true, "opened during a service, it stays open next time");
  assert.equal(soundOpen(false), true);
  saveOpen(soundKey(false), false);
  assert.equal(soundOpen(false), false);
  assert.notEqual(soundKey(true), soundKey(false));
});

test("the disclosure says whether it is open, what it opens and what it is", () => {
  const open = html(Disclosure, { open: true, label: "Sound", controls: "box" });
  assert.match(open, /aria-expanded="true"/);
  assert.match(open, /aria-controls="box"/);
  assert.match(open, /aria-label="Sound"/);
  assert.match(open, /title="Fold Sound"/);
  const folded = html(Disclosure, { open: false, label: "Sound", controls: "box" });
  assert.match(folded, /aria-expanded="false"/);
  assert.match(folded, /title="Open Sound"/);
  assert.match(folded, /class="disclosure[^"]*folded/);
});

const tip: Tip = {
  id: "t",
  title: "Press Space for Next",
  body: "Space moves the service on, from anywhere in Present.",
} as Tip;
const noop = () => {};

test("the tip card is open until folded, and its buttons work as before", () => {
  store.clear();
  const open = html(TipCard, { tip, ondismiss: noop, onall: noop });
  assert.match(open, /aria-expanded="true"/);
  assert.match(open, /Space moves the service on/);
  assert.match(open, /Got it/);
  assert.match(open, /All quick tips/);
});

test("a folded tip card is one line, and stays folded after a restart", () => {
  store.clear();
  saveOpen(TIP_KEY, false);
  const folded = html(TipCard, { tip, ondismiss: noop, onall: noop });
  assert.match(folded, /aria-expanded="false"/);
  assert.match(folded, /Did you know\?/);
  assert.match(folded, /Press Space for Next/, "the title is on the line");
  assert.doesNotMatch(folded, /Space moves the service on/);
  assert.doesNotMatch(folded, /Got it/);
  // Opening it again (a click or Enter on the line) is remembered the same way.
  saveOpen(TIP_KEY, true);
  assert.match(html(TipCard, { tip, ondismiss: noop, onall: noop }), /Got it/);
});
