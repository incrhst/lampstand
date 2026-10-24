// What a screen shows when nothing is on air (Setup › Screens): a picture,
// a looping video or a slide of a deck, set per screen or for every screen.
// Until one is chosen, a screen shows the default: the church's logo and name,
// else the Lampstand lamp.
// Free of Svelte and Tauri so node can test it.

import type { Brand, Deck, IdleScreen } from "./api";

export type IdleKind = "Inherit" | "Blank" | "Default" | "Image" | "Video" | "DeckSlide";

/** Which kind a setting is. `null` (nothing set on a screen) uses the default for all screens. */
export function idleKind(idle: IdleScreen | null | undefined): IdleKind {
  if (idle == null) return "Inherit";
  return typeof idle === "string" ? idle : (Object.keys(idle)[0] as IdleKind);
}

/** One line on what a setting shows: "Picture: Welcome.png", "Slide 3 of Announcements". */
export function describeIdle(
  idle: IdleScreen | null,
  decks: readonly Pick<Deck, "id" | "title">[],
  inherit = "Same as the other screens",
  brand: Pick<Brand, "name" | "logo"> | null = null,
): string {
  if (idle === null) return inherit;
  if (idle === "Blank") return "The theme's plain background";
  if (idle === "Default") return describeDefaultIdle(brand);
  if ("Image" in idle) return `Picture: ${idle.Image.name}`;
  if ("Video" in idle) return `Looping video: ${idle.Video.name}`;
  const deck = decks.find((d) => d.id === idle.DeckSlide.deck);
  return deck ? `Slide ${idle.DeckSlide.slide + 1} of ${deck.title}` : "A slide from a deck that is no longer in the library";
}

/** What the default idle screen shows: the church's logo and name once it has a logo, else Lampstand's lamp. */
export function describeDefaultIdle(brand: Pick<Brand, "name" | "logo"> | null): string {
  if (brand?.logo) {
    const name = brand.name.trim();
    return name ? `Your logo and name (${name}) on your colour` : "Your logo on your colour";
  }
  return "The Lampstand lamp. Add your logo under Setup › Church look to show yours instead.";
}

/** The idle setting that applies to a screen: its own, else the default for all screens, else the default idle screen. `null` is the theme's plain background. */
export function effectiveIdle(own: IdleScreen | null, fallback: IdleScreen | null): IdleScreen | null {
  const chosen = own ?? fallback ?? "Default";
  return chosen === "Blank" ? null : chosen;
}

/** The slide to show after the operator steps by `by`, kept inside the deck. */
export function stepSlide(slide: number, by: number, slides: number): number {
  return Math.min(Math.max(0, slides - 1), Math.max(0, slide + by));
}
