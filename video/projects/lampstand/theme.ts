import { staticFile } from "remotion";
import type { Theme } from "@video/core";

// The site's own palette and type (site/style.css, DESIGN.md): goldenrod copy paper, toner ink, one stamp violet.
// Role colours are the ones in docs/diagrams (Projection blue, Operator green, Stream purple, Audio orange).
export const roleColors = {
  projection: "#1f5fbf",
  operator: "#1d7a46",
  stream: "#6a3fc0",
  audio: "#b85a00",
} as const;

const font = (family: string, file: string, weight: number, style = "normal") =>
  `@font-face{font-family:"${family}";src:url(${staticFile(`fonts/${file}`)}) format("woff2");font-weight:${weight};font-style:${style}}`;

export const theme: Theme = {
  name: "lampstand",
  wordmark: "Lampstand",
  colors: { paper: "#f2c452", crease: "#d9a93a", ink: "#1d1a15", inkSoft: "#4a3d22", stamp: "#4c2a85", stampDeep: "#36195f", table: "#2b211a", screen: "#0b0e16", screenInk: "#f5f1e8", screenGold: "#e9b949", card: "#fbf3d9", ...roleColors },
  surfaces: {
    paper: { bg: "#f2c452", fg: "#1d1a15", soft: "#4a3d22", accent: "#4c2a85", card: "#fbf3d9", cardFg: "#1d1a15", line: "rgba(76,42,133,0.12)" },
    table: { bg: "#2b211a", fg: "#f2e6c8", soft: "#d9c28a", accent: "#f2c452", card: "#3a2d23", cardFg: "#f2e6c8", line: "rgba(242,196,82,0.10)" },
    screen: { bg: "#0b0e16", fg: "#f5f1e8", soft: "#b9b4a6", accent: "#e9b949", card: "#171c2a", cardFg: "#f5f1e8", line: "rgba(233,185,73,0.08)" },
  },
  fonts: { display: '"Old Standard TT", "Iowan Old Style", Georgia, serif', body: '"Old Standard TT", "Iowan Old Style", Georgia, serif', label: '"Courier Prime", "Courier New", monospace' },
  fontFaces: [
    font("Old Standard TT", "old-standard-tt-400-normal.woff2", 400),
    font("Old Standard TT", "old-standard-tt-400-italic.woff2", 400, "italic"),
    font("Old Standard TT", "old-standard-tt-700-normal.woff2", 700),
    font("Courier Prime", "courier-prime-400-normal.woff2", 400),
    font("Courier Prime", "courier-prime-700-normal.woff2", 700),
  ].join(""),
  radius: 6,
  // Calm and a little typewriter-stiff: no bounce, short stagger.
  motion: { spring: { damping: 22, stiffness: 120, mass: 1 }, staggerFrames: 7 },
  defaultTone: { title: "paper", bullets: "paper", stat: "paper", cta: "paper", quote: "table", roles: "paper", devices: "paper", diagram: "paper", screenshot: "screen", handover: "paper" },
};
