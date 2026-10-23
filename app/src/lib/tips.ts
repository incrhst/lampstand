// Quick tips: short notes on features, shown as a "Did you know?" card only
// when the thing they describe is in front of the operator, and listed in
// full under Setup › Tips. Kept free of Svelte and Tauri so node can test it.

import type { LoadedService } from "./api";

/** What a tip can see to decide whether it applies. */
export interface TipFacts {
  loaded: LoadedService | null;
  /** The item on air, if any. */
  liveItem: string | null;
  /** How many slides the item on air has. */
  liveSlides?: number;
  /** The church has set a logo (Setup › Church look). */
  hasLogo?: boolean;
}

export type TipArea = "Present" | "Plan";

export interface Tip {
  id: string;
  area: TipArea;
  title: string;
  body: string;
  /** A short looping clip under app/public/tips (recorded with video/capture/tips.mjs). */
  clip?: string;
  /** The feature is in front of the operator now. */
  relevant: (f: TipFacts) => boolean;
  /** They have already used it, so the tip has nothing left to teach. */
  used: (f: TipFacts) => boolean;
}

const sections = (f: TipFacts) => f.loaded?.sections ?? [];
const anyLoops = (f: TipFacts) => sections(f).some((s) => s.seconds !== null);

const onAir = (f: TipFacts) => f.liveItem !== null;
const never = () => false;

export const TIPS: Tip[] = [
  {
    id: "loop-group",
    clip: "tips/loop-group.mp4",
    area: "Present",
    title: "Loop a group",
    body: "Press Loop on a group to make its slides advance by themselves, for pre- and post-service. Pick how long each slide stays up.",
    relevant: (f) => sections(f).some((s) => s.items.length > 0),
    used: anyLoops,
  },
  {
    id: "loop-progress",
    clip: "tips/loop-progress.mp4",
    area: "Present",
    title: "See the time left on a slide",
    body: "While a group loops, a thin bar along the bottom of the live screen fills as the slide's time runs out.",
    relevant: (f) => sections(f).some((s) => s.seconds !== null && f.liveItem !== null && s.items.includes(f.liveItem)),
    used: () => false,
  },
  {
    id: "fold-group",
    clip: "tips/fold-group.mp4",
    area: "Present",
    title: "Fold a group away",
    body: "Click a group's name in the service order to fold it up and keep a long order short. The slide on air always stays visible.",
    relevant: (f) => sections(f).filter((s) => s.items.length > 0).length >= 2,
    used: never,
  },
  {
    id: "quick-add",
    clip: "tips/quick-add.mp4",
    area: "Present",
    title: "Add a song without leaving the screen",
    body: "Press / to search for a song or verse and add it right after what is on air. Nothing changes on the screen until you start it.",
    relevant: onAir,
    used: never,
  },
  {
    id: "jump-keys",
    clip: "tips/jump-keys.mp4",
    area: "Present",
    title: "Jump straight to a verse or chorus",
    body: "On a song, press C for the chorus, B for the bridge, or a number for that verse. On scripture, a number jumps to that verse.",
    relevant: (f) => onAir(f) && (f.liveSlides ?? 0) >= 3,
    used: never,
  },
  {
    id: "blank-screen",
    clip: "tips/blank-screen.mp4",
    area: "Present",
    title: "Blank the screen in one key",
    body: "Press the full stop to blank the screen at once, and again to bring the slide back. The Blank button does the same.",
    relevant: onAir,
    used: never,
  },
  {
    id: "logo-bug",
    area: "Present",
    title: "Put your logo in a corner",
    body: "Switch on Logo bug in the More menu to show your logo small in the lower corner of every screen and the stream. Black and the Logo screen cover it. Set the corner and size in Setup › Church look.",
    relevant: (f) => onAir(f) && f.hasLogo === true,
    used: never,
  },
  {
    id: "safe-area",
    clip: "tips/safe-area.mp4",
    area: "Present",
    title: "Check the safe area",
    body: "Switch on the safe-area guide in the More menu to see what a projector might crop. It shows only on your preview, never on the screen.",
    relevant: onAir,
    used: never,
  },
];

/** The tip to offer now: relevant, not used, not dismissed; the first in catalogue order. */
export function nextTip(f: TipFacts, dismissed: ReadonlySet<string>, tips: Tip[] = TIPS): Tip | null {
  return tips.find((t) => !dismissed.has(t.id) && t.relevant(f) && !t.used(f)) ?? null;
}

const KEY = "lampstand.tips.dismissed";
const USED_KEY = "lampstand.tips.used";

/** Tips dismissed on this device. Browser storage can be unavailable, so this never throws. */
export const loadDismissed = () => loadSet(KEY);

/** Features this device has used, so their tips are not offered. */
export const loadUsed = () => loadSet(USED_KEY);
export const saveUsed = (ids: ReadonlySet<string>) => saveSet(USED_KEY, ids);
export const saveDismissed = (ids: ReadonlySet<string>) => saveSet(KEY, ids);

function loadSet(key: string): Set<string> {
  try {
    const raw = JSON.parse(globalThis.localStorage?.getItem(key) ?? "[]");
    return new Set(Array.isArray(raw) ? raw.filter((x) => typeof x === "string") : []);
  } catch {
    return new Set();
  }
}

function saveSet(key: string, ids: ReadonlySet<string>) {
  try {
    globalThis.localStorage?.setItem(key, JSON.stringify([...ids]));
  } catch {
    // Not remembered: the tip may show again next time.
  }
}
