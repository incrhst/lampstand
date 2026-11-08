// A picture of what an idle setting shows, for Setup and the live preview in
// Present. A video or a picture is its poster, a deck is the chosen slide.

import { request, type Brand, type IdleScreen } from "./api";

export interface IdlePicture {
  /** A `data:` URL. */
  picture: string;
  /** For a deck slide, how many slides the deck has. */
  slides?: number;
}

/**
 * Null for the plain background, or when the picture cannot be drawn (not on this device yet, no GPU).
 * The default idle screen is drawn from `brand` (Lampstand's own with none) in the theme `theme`.
 */
export async function loadIdlePicture(idle: IdleScreen | null, width: number, brand: Brand | null = null, theme: string | null = null): Promise<IdlePicture | null> {
  if (idle === null || idle === "Blank") return null;
  try {
    if (idle === "Default") {
      return { picture: await request({ area: "Library", request: { IdlePreview: { brand, theme, width } } }, "Thumbnail") };
    }
    if ("Image" in idle || "Video" in idle) {
      const { blob, name } = "Image" in idle ? idle.Image : idle.Video;
      return { picture: await request({ area: "Library", request: { MediaThumbnail: { blob, name, width } } }, "Thumbnail") };
    }
    const { thumbnail, slides } = await request(
      { area: "Library", request: { DeckSlidePicture: { deck: idle.DeckSlide.deck, slide: idle.DeckSlide.slide, width } } },
      "DeckSlide",
    );
    return { picture: thumbnail, slides };
  } catch {
    return null;
  }
}

/** A picture of the church's logo, for the logo bug. */
export async function loadLogoPicture(blob: string | null, width: number): Promise<string | null> {
  if (!blob) return null;
  try {
    return await request({ area: "Library", request: { MediaThumbnail: { blob, name: "logo.png", width } } }, "Thumbnail");
  } catch {
    return null;
  }
}
