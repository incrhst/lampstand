import { test } from "node:test";
import assert from "node:assert/strict";
import { VIEW_KEY, loadView, saveView, slideMark } from "./presentView.ts";

function withStorage(fn: (store: Map<string, string>) => void) {
  const store = new Map<string, string>();
  const g = globalThis as { localStorage?: unknown };
  const before = g.localStorage;
  g.localStorage = {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
  };
  try {
    fn(store);
  } finally {
    g.localStorage = before;
  }
}

test("the wall is the view until another is chosen", () => {
  withStorage(() => assert.equal(loadView(), "wall"));
});

test("classic slides is remembered on this computer", () => {
  withStorage((store) => {
    saveView("classic");
    assert.equal(store.get(VIEW_KEY), "classic");
    assert.equal(loadView(), "classic");
    saveView("wall");
    assert.equal(loadView(), "wall");
  });
});

test("a value this version does not know means the wall", () => {
  withStorage((store) => {
    store.set(VIEW_KEY, "grid");
    assert.equal(loadView(), "wall");
  });
});

test("storage that throws still gives the wall", () => {
  const g = globalThis as { localStorage?: unknown };
  const before = g.localStorage;
  g.localStorage = {
    getItem: () => {
      throw new Error("blocked");
    },
    setItem: () => {
      throw new Error("blocked");
    },
  };
  try {
    assert.equal(loadView(), "wall");
    saveView("classic");
  } finally {
    g.localStorage = before;
  }
});

test("a card is marked live, next, or neither", () => {
  const live = { item: "a", slide: 1, step: 0 };
  const next = { item: "a", slide: 2, step: 0 };
  assert.equal(slideMark("a", 1, live, next), "live");
  assert.equal(slideMark("a", 2, live, next), "next");
  assert.equal(slideMark("a", 0, live, next), null);
  assert.equal(slideMark("b", 1, live, next), null);
  assert.equal(slideMark("a", 1, null, null), null);
});
