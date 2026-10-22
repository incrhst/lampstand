// Keys that move between the screens, kept free of Svelte so node can test
// them. They all use the platform's command key (Ctrl elsewhere), so they
// work from anywhere, even while typing, and never collide with the plain
// keys Present uses to run a service.

export type ScreenId = "plan" | "operate" | "setup";

export interface KeyLike {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

/**
 * The screen a key press asks for, or null when it is not one of ours.
 * Mod+1 is Plan, Mod+2 is Present, and Mod+\ goes to the other one of the
 * two (from Setup, to Plan).
 */
export function screenForKey(e: KeyLike, current: ScreenId): ScreenId | null {
  if (e.altKey || e.shiftKey || e.metaKey === e.ctrlKey) return null;
  if (e.key === "1") return "plan";
  if (e.key === "2") return "operate";
  if (e.key === "\\") return current === "plan" ? "operate" : "plan";
  return null;
}

/** "⌘" on a Mac and "Ctrl+" elsewhere, for hints. */
export function modLabel(platform: string): string {
  return /mac|iphone|ipad/i.test(platform) ? "⌘" : "Ctrl+";
}
