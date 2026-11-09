// How Present shows its picture of the screen, chosen in Settings › Screens
// and remembered per computer. Kept out of Present itself so a monthly
// volunteer has nothing there to change by accident. Storage can be missing
// (a private window), so every read and write is wrapped.

export type PreviewSize = "small" | "large" | "off";

const PREVIEW_KEY = "lampstand.present.preview";
const GUIDE_KEY = "lampstand.present.safeAreaGuide";
const NARROW_KEY = "lampstand.present.narrow";

export const previewSizes = [
  { id: "off", label: "Off" },
  { id: "small", label: "Small" },
  { id: "large", label: "Large" },
] as const;

/** The picture of the screen in Present's On air: small unless chosen otherwise. */
export function loadPreviewSize(): PreviewSize {
  try {
    const v = globalThis.localStorage?.getItem(PREVIEW_KEY);
    return v === "large" || v === "off" ? v : "small";
  } catch {
    return "small";
  }
}

export function savePreviewSize(size: PreviewSize) {
  try {
    globalThis.localStorage?.setItem(PREVIEW_KEY, size);
  } catch {
    // Not remembered: it still applies until Lampstand is closed.
  }
}

/** Whether On air's picture shows the safe area guide: on unless turned off. */
export function loadSafeAreaGuide(): boolean {
  try {
    return globalThis.localStorage?.getItem(GUIDE_KEY) !== "0";
  } catch {
    return true;
  }
}

export function saveSafeAreaGuide(on: boolean) {
  try {
    globalThis.localStorage?.setItem(GUIDE_KEY, on ? "1" : "0");
  } catch {
    // Not remembered: it still applies until Lampstand is closed.
  }
}

/** Whether Present's right column is the narrower one: wider unless chosen. */
export function loadNarrow(): boolean {
  try {
    return globalThis.localStorage?.getItem(NARROW_KEY) === "1";
  } catch {
    return false;
  }
}

export function saveNarrow(on: boolean) {
  try {
    globalThis.localStorage?.setItem(NARROW_KEY, on ? "1" : "0");
  } catch {
    // Not remembered: it still applies until Lampstand is closed.
  }
}
