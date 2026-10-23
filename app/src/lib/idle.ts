// What a screen shows when nothing is on air (Setup › Screens): a picture,
// a looping video or a slide of a deck, set per screen or for every screen.
// Free of Svelte and Tauri so node can test it.

import type { Deck, IdleScreen } from "./api";

export type IdleKind = "Inherit" | "Blank" | "Image" | "Video" | "DeckSlide";

/** Which kind a setting is. `null` (nothing set on a screen) uses the default for all screens. */
export function idleKind(idle: IdleScreen | null | undefined): IdleKind {
  if (idle == null) return "Inherit";
  return typeof idle === "string" ? idle : (Object.keys(idle)[0] as IdleKind);
}

/** One line on what a setting shows: "Picture: Welcome.png", "Slide 3 of Announcements". */
export function describeIdle(idle: IdleScreen | null, decks: readonly Pick<Deck, "id" | "title">[], inherit = "Same as the other screens"): string {
  if (idle === null) return inherit;
  if (idle === "Blank") return "The theme's plain background";
  if ("Image" in idle) return `Picture: ${idle.Image.name}`;
  if ("Video" in idle) return `Looping video: ${idle.Video.name}`;
  const deck = decks.find((d) => d.id === idle.DeckSlide.deck);
  return deck ? `Slide ${idle.DeckSlide.slide + 1} of ${deck.title}` : "A slide from a deck that is no longer in the library";
}

/** The idle setting that applies to a screen: its own, else the default for all screens. */
export function effectiveIdle(own: IdleScreen | null, fallback: IdleScreen | null): IdleScreen | null {
  const chosen = own ?? fallback;
  return chosen === "Blank" ? null : chosen;
}

/** The slide to show after the operator steps by `by`, kept inside the deck. */
export function stepSlide(slide: number, by: number, slides: number): number {
  return Math.min(Math.max(0, slides - 1), Math.max(0, slide + by));
}
