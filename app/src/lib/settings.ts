// This device's settings (clicker mode, default Bible, default theme, idle
// screen, church brand). The core replaces the whole object on a save, so
// a change is read, changed and written back.

import { request, type DeviceSettings } from "./api";

export async function loadSettings(): Promise<DeviceSettings> {
  return request({ area: "Settings", request: "Get" }, "Settings");
}

/** Saves `change` over the current settings and returns what is now saved. */
export async function updateSettings(change: Partial<DeviceSettings>): Promise<DeviceSettings> {
  const current = await loadSettings();
  return request({ area: "Settings", request: { Put: { ...current, ...change } } }, "Settings");
}
