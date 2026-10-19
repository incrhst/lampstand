// Fitting a screen of one shape inside a projector's picture of another:
// a 16:9 projector on a 4:5 screen lights the wall either side, so the
// safe area becomes the part of the picture that lands on the screen.

import type { OutputTarget, SafeArea } from "./api";

export const screenShapes = [
  { id: "fill", label: "Whole picture", ratio: null },
  { id: "4:3", label: "4:3", ratio: 4 / 3 },
  { id: "1:1", label: "1:1", ratio: 1 },
  { id: "4:5", label: "4:5", ratio: 4 / 5 },
] as const;

export type ScreenShape = (typeof screenShapes)[number]["id"];

/**
 * Margins (fractions of the picture's width and height) that leave a
 * centred area of the screen's shape, as large as fits. A screen wider
 * than the picture gets top and bottom margins; a narrower one, sides.
 */
export function marginsForScreen(pictureWidth: number, pictureHeight: number, screenRatio: number | null): SafeArea {
  const none = { top: 0, right: 0, bottom: 0, left: 0 };
  if (screenRatio === null || pictureWidth <= 0 || pictureHeight <= 0) return none;
  const picture = pictureWidth / pictureHeight;
  if (screenRatio < picture) {
    const side = (1 - screenRatio / picture) / 2;
    return { ...none, left: side, right: side };
  }
  const band = (1 - picture / screenRatio) / 2;
  return { ...none, top: band, bottom: band };
}

/** The pixel size an output draws at. */
export function targetSize(target: OutputTarget): { width: number; height: number } {
  if ("Window" in target) return target.Window;
  if ("Display" in target) return target.Display;
  return target.BrowserSource;
}
