import React, { createContext, useContext } from "react";
import { useVideoConfig } from "remotion";

export type Tone = "paper" | "table" | "screen";
export type Surface = { bg: string; fg: string; soft: string; accent: string; card: string; cardFg: string; line: string };

export type Theme = {
  name: string;
  wordmark: string;
  colors: Record<string, string>;
  surfaces: Record<Tone, Surface>;
  fonts: { display: string; body: string; label: string };
  fontFaces: string; // @font-face CSS, built with staticFile() by the project
  radius: number;
  motion: { spring: { damping: number; stiffness: number; mass: number }; staggerFrames: number };
  defaultTone: Record<string, Tone>; // scene type -> tone when the scene sets none
};

const Ctx = createContext<Theme | null>(null);
export const ThemeProvider: React.FC<{ theme: Theme; children: React.ReactNode }> = ({ theme, children }) => (
  <Ctx.Provider value={theme}>
    <style>{theme.fontFaces}</style>
    {children}
  </Ctx.Provider>
);
export function useTheme(): Theme {
  const t = useContext(Ctx);
  if (!t) throw new Error("useTheme outside ThemeProvider");
  return t;
}

/** Every video's shortest side is 1080px, so one pixel scale works for all three aspects. */
export function useLayout() {
  const { width, height } = useVideoConfig();
  const pad = 96;
  return { w: width, h: height, portrait: height > width, square: width === height, pad, innerW: width - pad * 2, innerH: height - pad * 2 };
}
