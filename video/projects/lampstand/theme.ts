import { staticFile } from "remotion";
import type { Theme } from "@video/core";
import { app, motion, roles, site, space, type } from "./tokens";

// The explainer theme, built from tokens.ts (site palette, app palette, role colours, type, spacing, motion).
// Role colours on paper are the ones in docs/diagrams (Projection blue, Operator green, Stream purple, Audio orange).
export const roleColors = roles.doc;

export const fontFaces = type.faces
  .map(([family, file, weight, style]) => `@font-face{font-family:"${family}";src:url(${staticFile(`fonts/${file}`)}) format("woff2");font-weight:${weight};font-style:${style}}`)
  .join("");

export const theme: Theme = {
  name: "lampstand",
  wordmark: "Lampstand",
  colors: { ...site, ...roleColors, appCanvas: app.canvas, appPanel: app.panel, appGold: app.gold, live: app.live },
  surfaces: {
    paper: { bg: site.paper, fg: site.ink, soft: site.inkSoft, accent: site.stamp, card: site.card, cardFg: site.ink, line: "rgba(76,42,133,0.12)" },
    table: { bg: site.table, fg: "#f2e6c8", soft: "#d9c28a", accent: site.paper, card: "#3a2d23", cardFg: "#f2e6c8", line: "rgba(242,196,82,0.10)" },
    screen: { bg: site.screen, fg: site.screenInk, soft: "#b9b4a6", accent: site.screenGold, card: "#171c2a", cardFg: site.screenInk, line: "rgba(233,185,73,0.08)" },
  },
  fonts: { display: type.serif, body: type.serif, label: type.typed },
  fontFaces,
  radius: space.radius,
  motion: { spring: motion.spring, staggerFrames: motion.staggerFrames },
  defaultTone: { title: "paper", bullets: "paper", stat: "paper", cta: "paper", quote: "table", roles: "paper", devices: "paper", diagram: "paper", screenshot: "screen", handover: "paper" },
};
