import { test } from "node:test";
import assert from "node:assert/strict";
import { chooseTranslation, versionLabel } from "./bibles.ts";
import type { Translation } from "./api/index";

const t = (id: string, name: string, abbreviation = ""): Translation =>
  ({ id, name, abbreviation, language: "en", licence: "PublicDomain", text: "" }) as unknown as Translation;

const all = [t("kjv", "King James Version", "KJV"), t("web", "World English Bible", "WEB")];

test("the default version is used when nothing else was picked", () => {
  assert.equal(chooseTranslation(all, "web"), "web");
});

test("what was just picked beats the default while the device still has it", () => {
  assert.equal(chooseTranslation(all, "web", "kjv"), "kjv");
  assert.equal(chooseTranslation(all, "web", "gone"), "web");
});

test("a default the device no longer has falls back to the first version", () => {
  assert.equal(chooseTranslation(all, "esv"), "kjv");
  assert.equal(chooseTranslation(all, null), "kjv");
  assert.equal(chooseTranslation([], "web"), "");
});

test("a version is labelled by abbreviation, else name", () => {
  assert.equal(versionLabel(all[0]), "KJV");
  assert.equal(versionLabel(t("x", "Plain Bible")), "Plain Bible");
});
