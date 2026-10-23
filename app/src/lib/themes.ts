// Themes as the Themes page edits them: colour and font helpers, the
// choices each setting offers, and the small operations (duplicate, name,
// which theme a service or screen ends up with). Kept free of Svelte and
// Tauri so node can test it.

import type { Background, Brand, Color, DeviceSettings, OutputConfig, Theme, Transition } from "./api";

/** The id of the Starter theme every library has (matches the core's constant). */
export const STARTER_THEME_ID = "01920000-0000-7000-8000-6c616d707374";

/**
 * The fonts that come with Lampstand, so they look the same on every
 * machine. A church can type any other font installed on this computer.
 */
export const bundledFonts = [
  { family: "Noto Sans", note: "Clean, modern" },
  { family: "Noto Serif", note: "Traditional" },
  { family: "Arimo", note: "Like Arial" },
  { family: "Tinos", note: "Like Times New Roman" },
  { family: "Carlito", note: "Like Calibri" },
  { family: "Caladea", note: "Like Cambria" },
  { family: "Cousine", note: "Like Courier New" },
] as const;

export const weights = [
  { id: 300, label: "Light" },
  { id: 400, label: "Regular" },
  { id: 500, label: "Medium" },
  { id: 700, label: "Bold" },
  { id: 900, label: "Black" },
] as const;

export const aligns = [
  { id: "Left", label: "Left" },
  { id: "Center", label: "Centre" },
  { id: "Right", label: "Right" },
] as const;

export const valigns = [
  { id: "Top", label: "Top" },
  { id: "Middle", label: "Middle" },
  { id: "Bottom", label: "Bottom" },
] as const;

/** Where text goes, as fractions of the safe area. */
export const textAreas = [
  { id: "full", label: "Whole screen", rect: { x: 0, y: 0, width: 1, height: 1 } },
  { id: "lower", label: "Lower third", rect: { x: 0, y: 0.7, width: 1, height: 0.3 } },
  { id: "band", label: "Centre band", rect: { x: 0.08, y: 0.25, width: 0.84, height: 0.5 } },
] as const;

/** The preset a text area matches, or "custom". */
export function textAreaId(rect: Theme["text_area"]): string {
  const same = (a: number, b: number) => Math.abs(a - b) < 0.0005;
  const hit = textAreas.find(
    (t) =>
      same(t.rect.x, rect.x) && same(t.rect.y, rect.y) && same(t.rect.width, rect.width) && same(t.rect.height, rect.height),
  );
  return hit?.id ?? "custom";
}

type Kind = Transition["kind"];

/** The transitions, with the directions the ones that have a side take. */
export const transitions = [
  { id: "Cut", label: "Cut (instant)" },
  { id: "Fade", label: "Fade" },
  { id: "FadeThroughBlack", label: "Fade through black" },
  { id: "Zoom", label: "Zoom" },
  { id: "Push", label: "Push", sided: true },
  { id: "Wipe", label: "Wipe", sided: true },
  { id: "Cover", label: "Cover", sided: true },
] as const;

export const directions = [
  { id: "Left", label: "From the left" },
  { id: "Right", label: "From the right" },
  { id: "Up", label: "From the top" },
  { id: "Down", label: "From the bottom" },
] as const;

export type TransitionName = (typeof transitions)[number]["id"];
export type DirectionName = (typeof directions)[number]["id"];

export function transitionName(kind: Kind): TransitionName {
  return typeof kind === "string" ? kind : (Object.keys(kind)[0] as TransitionName);
}

export function transitionDirection(kind: Kind): DirectionName {
  if (typeof kind === "string") return "Left";
  return Object.values(kind)[0] as DirectionName;
}

export function makeTransition(name: TransitionName, direction: DirectionName, duration_ms: number): Transition {
  const sided = transitions.find((t) => t.id === name);
  const kind = (sided && "sided" in sided ? { [name]: direction } : name) as Kind;
  return { kind, duration_ms: name === "Cut" ? 0 : Math.max(0, Math.round(duration_ms)) };
}

/** `#rrggbb` for an `<input type="color">`. */
export function toHex(c: Color): string {
  const h = (n: number) => n.toString(16).padStart(2, "0");
  return `#${h(c.r)}${h(c.g)}${h(c.b)}`;
}

/** The colour of `#rrggbb` (or `#rgb`), fully opaque; null if it is not one. */
export function fromHex(hex: string): Color | null {
  const m = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return null;
  let v = m[1];
  if (v.length === 3) v = [...v].map((c) => c + c).join("");
  return { r: parseInt(v.slice(0, 2), 16), g: parseInt(v.slice(2, 4), 16), b: parseInt(v.slice(4, 6), 16), a: 255 };
}

function luminance(c: Color): number {
  const channel = (v: number) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b);
}

/** The contrast ratio of two colours, 1 to 21 (WCAG). */
export function contrast(a: Color, b: Color): number {
  const [x, y] = [luminance(a), luminance(b)];
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
}

/** Black or white, whichever reads better on `background`. */
export function readableOn(background: Color): Color {
  return luminance(background) > 0.4 ? { r: 0, g: 0, b: 0, a: 255 } : { r: 255, g: 255, b: 255, a: 255 };
}

export type BackgroundKind = "None" | "Color" | "Image" | "Video";

export function backgroundKind(b: Background): BackgroundKind {
  return typeof b === "string" ? b : (Object.keys(b)[0] as BackgroundKind);
}

/** The colour text is read against: the background colour, or black for a picture or video. */
export function backgroundColor(b: Background): Color {
  return typeof b === "object" && "Color" in b ? b.Color : { r: 0, g: 0, b: 0, a: 255 };
}

/** Text that is hard to read on a colour background, as a sentence; null when fine. */
export function readability(theme: Theme): string | null {
  if (backgroundKind(theme.background) !== "Color") return null;
  const ratio = contrast(theme.text.color, backgroundColor(theme.background));
  if (ratio >= 4.5) return null;
  return ratio < 3 ? "The text is very hard to read on this background." : "The text may be hard to read from the back of the room.";
}

/** "Mine copy", then "Mine copy 2": a name no theme has. */
export function copyName(name: string, themes: readonly Theme[]): string {
  const taken = new Set(themes.map((t) => t.name.toLowerCase()));
  const base = `${name} copy`;
  if (!taken.has(base.toLowerCase())) return base;
  for (let n = 2; ; n++) if (!taken.has(`${base} ${n}`.toLowerCase())) return `${base} ${n}`;
}

/** A copy under a new id and name, to be saved with `PutTheme`. */
export function duplicate(theme: Theme, themes: readonly Theme[], id: string): Theme {
  // JSON, not `structuredClone`, which cannot clone a Svelte state proxy.
  const copy = JSON.parse(JSON.stringify(theme)) as Theme;
  return { ...copy, id, name: copyName(theme.name, themes) };
}

/** Why a theme cannot be deleted, as a sentence; null when it can. */
export function cannotDelete(theme: Theme, themes: readonly Theme[]): string | null {
  if (theme.id === STARTER_THEME_ID) return "The Starter theme is what every screen falls back to. Duplicate it to make your own.";
  if (themes.length <= 1) return "This is the only theme.";
  return null;
}

/** Starter first, then the rest by name. */
export function sortThemes(themes: readonly Theme[]): Theme[] {
  return [...themes].sort((a, b) => {
    if (a.id === STARTER_THEME_ID) return -1;
    if (b.id === STARTER_THEME_ID) return 1;
    return a.name.localeCompare(b.name);
  });
}

/** The id of the theme a screen with no choice of its own ends up with. */
export function defaultThemeId(settings: Pick<DeviceSettings, "default_theme"> | null, themes: readonly Theme[]): string {
  const id = settings?.default_theme;
  if (id && themes.some((t) => t.id === id)) return id;
  return themes.find((t) => t.id === STARTER_THEME_ID)?.id ?? themes[0]?.id ?? STARTER_THEME_ID;
}

/**
 * The theme a screen shows lyrics in: its own, else the service's, else
 * the default. The same order the core uses, for saying which one in the
 * UI.
 */
export function themeFor(
  themes: readonly Theme[],
  settings: Pick<DeviceSettings, "default_theme"> | null,
  output: Pick<OutputConfig, "theme"> | null,
  service: { theme: string | null } | null,
): Theme | null {
  for (const id of [output?.theme, service?.theme]) {
    const hit = id && themes.find((t) => t.id === id);
    if (hit) return hit;
  }
  return themes.find((t) => t.id === defaultThemeId(settings, themes)) ?? null;
}

/**
 * The theme a brand makes, for the sketch beside the Church look form. The
 * core makes the real one (`lampstand_library::theme_from_brand`) the same
 * way: the Starter theme with the brand's colours and font.
 */
export function themeFromBrand(brand: Brand, starter: Theme): Theme {
  const copy = JSON.parse(JSON.stringify(starter)) as Theme;
  copy.name = brand.name.trim() || "Our church";
  copy.background = { Color: brand.primary };
  copy.text.color = brand.text ?? readableOn(brand.primary);
  if (brand.font_family.trim()) copy.text.font_family = brand.font_family.trim();
  return copy;
}
