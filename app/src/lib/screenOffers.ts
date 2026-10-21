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

/**
 * Which of `displays` (by index) already have a screen. Each display output
 * claims the display that fits it best, one output to a display, as the
 * core assigns them: a name match beats a size match, then the nearer
 * position wins. So two displays of one size are told apart, and a second
 * output for the same display claims nothing more.
 */
export function displaysWithScreens(outputs: OutputConfig[], displays: DisplayMatch[]): Set<number> {
  const wants: DisplayMatch[] = [];
  for (const o of outputs) if ("Display" in o.target) wants.push(o.target.Display);
  const candidates: { want: number; display: number; score: number[] }[] = [];
  wants.forEach((w, wi) =>
    displays.forEach((d, di) => {
      if (!fits(w, d)) return;
      const name = w.name !== "" && w.name === d.name ? 1 : 0;
      const size = w.width === d.width && w.height === d.height ? 1 : 0;
      const position = w.x === d.x && w.y === d.y ? 1 : 0;
      candidates.push({ want: wi, display: di, score: [name, size, position, -(Math.abs(w.x - d.x) + Math.abs(w.y - d.y))] });
    }),
  );
  // Best fit first; ties go to the earlier output, then the earlier display.
  candidates.sort((a, b) => {
    for (let i = 0; i < 4; i++) if (a.score[i] !== b.score[i]) return b.score[i] - a.score[i];
    return a.want - b.want || a.display - b.display;
  });
  const claimed = new Set<number>();
  const used = new Set<number>();
  for (const c of candidates) {
    if (used.has(c.want) || claimed.has(c.display)) continue;
    used.add(c.want);
    claimed.add(c.display);
  }
  return claimed;
}

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
  const taken = displaysWithScreens(outputs, displays);
  displays.forEach((d, i) => {
    if (!taken.has(i)) offers.push({ kind: "add", label: `Use ${d.name || `display ${i + 1}`}`, display: d });
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
