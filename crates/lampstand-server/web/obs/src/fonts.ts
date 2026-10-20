// Font families as the layout engine resolves them (crates/lampstand-layout
// fonts.rs): Office families map to the metric-compatible bundled ones, and
// everything else falls back to Noto Sans. Unlike layout, which uses an
// installed Calibri or Arial if the machine has one, this always draws the
// bundled stand-in: the metrics match, so lines break the same way. The @font-face rules come from the
// server (`/obs/assets/fonts.css`) and name the same files the app bundles.

const SUBSTITUTES: Record<string, string> = {
  calibri: "Carlito",
  cambria: "Caladea",
  arial: "Arimo",
  helvetica: "Arimo",
  "times new roman": "Tinos",
  "courier new": "Cousine",
};

export const DEFAULT_FAMILY = "Noto Sans";

export function cssFamily(family: string): string {
  const name = family.trim();
  const mapped = SUBSTITUTES[name.toLowerCase()] ?? name;
  const quoted = `"${mapped.replace(/["\\]/g, "")}"`;
  return mapped === DEFAULT_FAMILY ? quoted : `${quoted}, "${DEFAULT_FAMILY}", sans-serif`;
}
