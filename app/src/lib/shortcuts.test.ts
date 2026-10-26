import assert from "node:assert/strict";
import { test } from "node:test";
import { isTyping, modLabel, screenForKey, type KeyLike } from "./shortcuts.ts";

test("a switch or radio button is not typing, so Present's keys still work", () => {
  assert.equal(isTyping({ tagName: "INPUT", type: "checkbox" }), false);
  assert.equal(isTyping({ tagName: "INPUT", type: "radio" }), false);
  assert.equal(isTyping({ tagName: "BUTTON" }), false);
  assert.equal(isTyping({ tagName: "DIV" }), false);
  assert.equal(isTyping(null), false);
});

test("text fields, text areas, menus and editable text are typing", () => {
  assert.equal(isTyping({ tagName: "INPUT", type: "text" }), true);
  assert.equal(isTyping({ tagName: "INPUT", type: "search" }), true);
  assert.equal(isTyping({ tagName: "INPUT" }), true);
  assert.equal(isTyping({ tagName: "TEXTAREA" }), true);
  assert.equal(isTyping({ tagName: "SELECT" }), true);
  assert.equal(isTyping({ tagName: "DIV", isContentEditable: true }), true);
});

const press = (key: string, mods: Partial<KeyLike> = {}): KeyLike => ({
  key,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  ...mods,
});
const mod = { metaKey: true };

test("Mod+1 is Plan and Mod+2 is Present, from wherever you are", () => {
  for (const from of ["plan", "operate", "setup"] as const) {
    assert.equal(screenForKey(press("1", mod), from), "plan");
    assert.equal(screenForKey(press("2", mod), from), "operate");
  }
});

test("Mod+\\ goes between Plan and Present, and out of Setup to Plan", () => {
  assert.equal(screenForKey(press("\\", mod), "plan"), "operate");
  assert.equal(screenForKey(press("\\", mod), "operate"), "plan");
  assert.equal(screenForKey(press("\\", mod), "setup"), "plan");
});

test("Ctrl works as the modifier too", () => {
  assert.equal(screenForKey(press("2", { ctrlKey: true }), "plan"), "operate");
});

test("plain keys are left to Present, so 1 to 9 still pick a verse", () => {
  assert.equal(screenForKey(press("1"), "operate"), null);
  assert.equal(screenForKey(press("2"), "operate"), null);
  assert.equal(screenForKey(press("\\"), "operate"), null);
});

test("other combinations are not ours", () => {
  assert.equal(screenForKey(press("1", { metaKey: true, shiftKey: true }), "plan"), null);
  assert.equal(screenForKey(press("1", { metaKey: true, altKey: true }), "plan"), null);
  assert.equal(screenForKey(press("1", { metaKey: true, ctrlKey: true }), "plan"), null);
  assert.equal(screenForKey(press("3", mod), "plan"), null);
});

test("the hint says ⌘ on a Mac and Ctrl+ elsewhere", () => {
  assert.equal(modLabel("MacIntel"), "⌘");
  assert.equal(modLabel("Win32"), "Ctrl+");
  assert.equal(modLabel("Linux x86_64"), "Ctrl+");
});
