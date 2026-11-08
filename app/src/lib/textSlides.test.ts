import { test } from "node:test";
import assert from "node:assert/strict";
import { splitText, textTitle } from "./textSlides.ts";

test("a blank line or a line of hyphens starts a new slide", () => {
  assert.deepEqual(splitText("A\nB\n\n\nC\n---\nD\r\n-----\r\n- E"), [["A", "B"], ["C"], ["D"], ["- E"]]);
  assert.deepEqual(splitText("  \n---\n"), []);
});

test("the order list names a text item by its title, else its first line", () => {
  assert.equal(textTitle(" Welcome ", "x"), "Welcome");
  assert.equal(textTitle("", "\nBe still\nand know"), "Be still");
  assert.equal(textTitle("", ""), "Text slide");
  assert.equal(textTitle("", "x".repeat(60)), `${"x".repeat(40)}…`);
});
