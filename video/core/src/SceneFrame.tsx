import React from "react";
import { AbsoluteFill, useCurrentFrame, useVideoConfig } from "remotion";
import { Surface, Tone, useLayout, useTheme } from "./theme";
import { exitStyle } from "./motion/presets";

const SurfaceCtx = React.createContext<Surface | null>(null);
export const useSurface = () => {
  const s = React.useContext(SurfaceCtx);
  if (!s) throw new Error("useSurface outside SceneFrame");
  return s;
};

/** Background, wordmark, and the scene's exit. Every scene renders inside one. */
export const SceneFrame: React.FC<{ scene: any; children: React.ReactNode }> = ({ scene, children }) => {
  const theme = useTheme();
  const frame = useCurrentFrame();
  const { durationInFrames } = useVideoConfig();
  const { pad, portrait } = useLayout();
  const tone: Tone = scene.tone ?? theme.defaultTone[scene.type] ?? "paper";
  const s = theme.surfaces[tone];
  return (
    <SurfaceCtx.Provider value={s}>
      <AbsoluteFill style={{ background: s.bg, color: s.fg, fontFamily: theme.fonts.body }}>
        <div style={{ position: "absolute", inset: 0, backgroundImage: `radial-gradient(${s.line} 2px, transparent 2.5px)`, backgroundSize: "48px 48px", opacity: 0.5 }} />
        <AbsoluteFill style={exitStyle(scene.exit ?? "fade", frame, durationInFrames)}>{children}</AbsoluteFill>
        <div style={{ position: "absolute", left: pad, bottom: portrait ? 56 : 36, fontFamily: theme.fonts.label, fontWeight: 700, fontSize: 24, letterSpacing: "0.14em", color: s.accent, opacity: 0.8, textTransform: "uppercase" }}>
          {theme.wordmark}
        </div>
      </AbsoluteFill>
    </SurfaceCtx.Provider>
  );
};
