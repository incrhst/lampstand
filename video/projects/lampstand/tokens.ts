// Lampstand design tokens for video. One place for every colour, font, size, space and motion value;
// theme.ts (explainers) and tour/ (the site's fly-over) both read from here, and no scene hardcodes one.
//
// Sources:
//   site  - site/style.css and DESIGN.md: goldenrod copy paper, toner ink, one stamp violet.
//   app   - app/src/lib/kit/theme.css (the app is dark only). `npm run lamp:tokens` checks these against the file.
//   roles - docs/diagrams colour each role; the app lightens each one just enough for 3:1 on its dark panels.

export const site = {
  paper: "#f2c452",
  crease: "#d9a93a",
  ink: "#1d1a15",
  inkSoft: "#4a3d22",
  stamp: "#4c2a85",
  stampDeep: "#36195f",
  table: "#2b211a",
  screen: "#0b0e16",
  screenInk: "#f5f1e8",
  screenGold: "#e9b949",
  card: "#fbf3d9",
} as const;

/** The app's own tokens, named as in theme.css without the leading `--`. */
export const app = {
  canvas: "#121419",
  panel: "#1b1f27",
  raised: "#232833",
  line: "#2a303b",
  "line-strong": "#3a4250",
  text: "#e6e8ec",
  muted: "#9aa3b2",
  faint: "#808a9b",
  gold: "#f0c75e",
  "gold-hover": "#f5d47e",
  "on-gold": "#1b1f27",
  live: "#e5484d",
  "live-strong": "#c62f37",
  ready: "#46a758",
  "ready-text": "#5cc16f",
  warn: "#f5a524",
  "role-projection": "#4c7fcc",
  "role-operator": "#348758",
  "role-stream": "#8865cd",
  "role-audio": "#bf6a1a",
} as const;

/** Role colours. `doc` on light paper (docs, diagrams, explainers); `app` on the app's dark panels. */
export const roles = {
  doc: { projection: "#1f5fbf", operator: "#1d7a46", stream: "#6a3fc0", audio: "#b85a00" },
  app: { projection: app["role-projection"], operator: app["role-operator"], stream: app["role-stream"], audio: app["role-audio"] },
} as const;

export const type = {
  serif: '"Old Standard TT", "Iowan Old Style", Georgia, serif',
  typed: '"Courier Prime", "Courier New", monospace',
  /** Font files in video/public/fonts (SIL OFL), the site's own. */
  faces: [
    ["Old Standard TT", "old-standard-tt-400-normal.woff2", 400, "normal"],
    ["Old Standard TT", "old-standard-tt-400-italic.woff2", 400, "italic"],
    ["Old Standard TT", "old-standard-tt-700-normal.woff2", 700, "normal"],
    ["Courier Prime", "courier-prime-400-normal.woff2", 400, "normal"],
    ["Courier Prime", "courier-prime-700-normal.woff2", 700, "normal"],
  ] as const,
  /** Pixel sizes at 1080p (every aspect's short side is 1080). */
  size: { tag: 24, chrome: 15, credit: 20, sub: 28, key: 30, title: 54, titleBig: 66, lyric: 82 },
  tracking: { tag: "0.14em", label: "0.16em", backdrop: "0.12em" },
  leading: { title: 1.08, sub: 1.35, lyric: 1.25 },
} as const;

export const space = {
  /** Safe margin of an explainer scene. */
  pad: 96,
  /** Lower-left caption card's distance from the frame edge in the tour. */
  edge: 80,
  card: { padding: "26px 34px 28px", border: 3, offset: 10, maxW: 900, maxWBig: 1100, tagGap: 10, subGap: 14, keysGap: 18 },
  key: { gap: 10, minW: 54, padding: "6px 16px", radius: 8, travel: 4 },
  radius: 6,
  radiusLg: 10,
  dotGrid: 48,
} as const;

export const motion = {
  /** Explainer entrances: calm and a little typewriter-stiff, no bounce. */
  spring: { damping: 22, stiffness: 120, mass: 1 },
  staggerFrames: 7,
  /** The app's own ease (theme.css --ease), for UI-like moves. */
  appEase: [0.23, 1, 0.32, 1] as const,
  /** Tour camera: rests at each stop for up to `rest` s (or 30% of the gap), then glides with an in-out cubic. */
  camera: { rest: 1.6, restShare: 0.3, drift: { x: 22, y: 12, yaw: 0.6, xRate: 0.9, yRate: 0.7 } },
  /** Tour caption cards: rise in, fade out, tilt slightly like a pasted label. */
  caption: { inSec: 0.6, outSec: 0.4, rise: 30, tilt: -0.6, leadSec: 0.4, tailSec: 0.4 },
  /** A key cap is pressed for `holdSec`, the first `firstSec` after its caption lands, then every `everySec`. */
  keys: { firstSec: 1.2, everySec: 0.9, holdSec: 0.5 },
  /** Whole-film fades, in frames. */
  fadeInFrames: 18,
  fadeOutFrames: 24,
} as const;

/** The tour's window planes and projector, drawn around the real screenshots. */
export const tour = {
  table: site.table,
  windowBody: "#15171d",
  chrome: "#2a2d36",
  chromeInk: "#9aa0ad",
  lights: ["#ff5f57", "#febc2e", "#28c840"],
  chromeH: 34,
  windowRadius: 12,
  bezel: "#101114",
  bezelW: 14,
  shadow: "0 40px 90px rgba(29,18,0,0.55), 0 6px 14px rgba(29,18,0,0.4)",
  screenShadow: "0 50px 100px rgba(29,18,0,0.6), 0 0 120px rgba(233,185,73,0.25)",
  vignette: "radial-gradient(ellipse at 50% 50%, transparent 55%, rgba(29,18,0,0.45) 100%)",
  dots: "rgba(76,42,133,0.10)",
  fold: "linear-gradient(90deg, transparent, rgba(150,100,10,0.18) 48%, rgba(255,240,200,0.25) 52%, transparent)",
  keyShadow: "0 6px 0 #000",
  keyShadowPressed: "0 2px 0 #000",
  perspective: 1400,
} as const;
