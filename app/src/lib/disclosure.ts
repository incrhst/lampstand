// A box that folds to one line (kit/Disclosure.svelte): the Sound box in
// Present (#191) and the "Did you know?" card (#189). Whether it is open is
// remembered on this computer, under a key of its own.

/** The choice remembered under `key`: open, folded, or nothing chosen yet (null). Never throws. */
export function loadOpen(key: string): boolean | null {
  try {
    const v = globalThis.localStorage?.getItem(key);
    return v === "1" ? true : v === "0" ? false : null;
  } catch {
    return null;
  }
}

export function saveOpen(key: string, open: boolean) {
  try {
    globalThis.localStorage?.setItem(key, open ? "1" : "0");
  } catch {
    // Not remembered this time.
  }
}

/** Open or folded: what was chosen on this computer, or the box's own default. */
export function openState(key: string, fallback: boolean): boolean {
  return loadOpen(key) ?? fallback;
}

export const TIP_KEY = "lampstand.tips.open";

/**
 * The Sound box remembers two choices: before the service starts (open
 * unless folded here, so the team can check the sound) and once it is under
 * way (folded unless opened here).
 */
export function soundKey(underWay: boolean): string {
  return underWay ? "lampstand.sound.open.during" : "lampstand.sound.open.before";
}

/** Whether the Sound box is open, before or during the service. */
export function soundOpen(underWay: boolean): boolean {
  return openState(soundKey(underWay), !underWay);
}
