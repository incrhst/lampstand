// The default Bible version, kept in this device's settings (Setup ›
// Scripture). Choosing a Bible in the library or quick add saves it too, so
// the last one chosen is the one the next reference is read from.

import { request } from "./api";

/** The saved default translation's ID, or null when none is set. */
export async function loadDefaultTranslation(): Promise<string | null> {
  const settings = await request({ area: "Settings", request: "Get" }, "Settings");
  return settings.default_translation ?? null;
}

export async function saveDefaultTranslation(id: string | null): Promise<void> {
  const settings = await request({ area: "Settings", request: "Get" }, "Settings");
  await request({ area: "Settings", request: { Put: { ...settings, default_translation: id } } }, "Settings");
}
