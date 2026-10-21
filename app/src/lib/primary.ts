// Which audience screen is the main one. Scripture is split to fit the main
// screen, so a rehearsal window must never keep that place once a real
// display is added: the display takes it, and the window steps down.

import type { OutputConfig, OutputTarget } from "./api";

const isAudience = (o: OutputConfig) => typeof o.role === "object" && "Audience" in o.role;
const isPrimary = (o: OutputConfig) => typeof o.role === "object" && "Audience" in o.role && o.role.Audience.primary;

/** The audience window that holds the main place and would give it up to a display, if any. */
export function windowToDemote(outputs: OutputConfig[], target: OutputTarget): OutputConfig | null {
  if (!("Display" in target)) return null;
  return outputs.find((o) => isAudience(o) && isPrimary(o) && "Window" in o.target) ?? null;
}

/** Whether a new audience output with this target is the main screen. */
export function takesMainPlace(outputs: OutputConfig[], target: OutputTarget): boolean {
  return !outputs.some(isPrimary) || windowToDemote(outputs, target) !== null;
}

/** `window` with the main place given up. */
export const steppedDown = (window: OutputConfig): OutputConfig => ({ ...window, role: { Audience: { primary: false } } });
