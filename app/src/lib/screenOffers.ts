// What the "No screen is on" prompt can switch on in one tap: screens the
// operator has already set up but left off, and displays that are connected
// but not yet a screen. The list of displays never includes the one the
// operator window is on (the core leaves it out), so nothing offered here
// can cover the controls.

import type { DisplayMatch, OutputConfig } from "./api";
import { takesMainPlace } from "./primary.ts";

export type ScreenOffer =
  /** An audience screen that is set up and off. */
  | { kind: "enable"; label: string; output: OutputConfig }
  /** A connected display with no screen yet. */
  | { kind: "add"; label: string; display: DisplayMatch };

const isAudience = (o: OutputConfig) => typeof o.role === "object" && "Audience" in o.role;

/** The core's rule for finding a display again: same name, or same size. */
const fits = (want: DisplayMatch, d: DisplayMatch) =>
  (want.name !== "" && want.name === d.name) || (want.width === d.width && want.height === d.height);

/** At most `limit` offers, set-up screens first. */
export function screenOffers(outputs: OutputConfig[], displays: DisplayMatch[], limit = 3): ScreenOffer[] {
  const offers: ScreenOffer[] = [];
  for (const o of outputs) {
    if (o.enabled || !isAudience(o)) continue;
    const t = o.target;
    if ("Window" in t || ("Display" in t && displays.some((d) => fits(t.Display, d)))) {
      offers.push({ kind: "enable", label: `Turn on ${o.name}`, output: o });
    }
  }
  displays.forEach((d, i) => {
    const taken = outputs.some((o) => "Display" in o.target && fits(o.target.Display, d));
    if (!taken) offers.push({ kind: "add", label: `Use ${d.name || `display ${i + 1}`}`, display: d });
  });
  return offers.slice(0, limit);
}

/** The output for a display turned on from the prompt: on, and the main audience screen unless a display already is. */
export function newScreen(display: DisplayMatch, outputs: OutputConfig[], id: string): OutputConfig {
  const target = { Display: display };
  const primary = takesMainPlace(outputs, target);
  return {
    id,
    name: display.name || "Display",
    role: { Audience: { primary } },
    target,
    safe_area: { top: 0, right: 0, bottom: 0, left: 0 },
    background: "Bleed",
    theme: null,
    enabled: true,
  };
}
