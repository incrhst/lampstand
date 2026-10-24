import React from "react";
import { Series } from "remotion";
import { Theme, ThemeProvider } from "./theme";
import { sceneFrames } from "./duration";
import { SceneFrame } from "./SceneFrame";

export type Registry = Record<string, React.ComponentType<{ scene: any }>>;

export function makeMasterVideo(opts: { registry: Registry; customRegistry?: Registry; defaultSec: (s: any) => number; theme: Theme; schema: { parse: (v: any) => any } }) {
  const { registry, customRegistry = {}, defaultSec, theme, schema } = opts;
  const MasterVideo: React.FC<{ scenes: any[]; fps: number; aspect: string }> = (raw) => {
    const { scenes, fps } = schema.parse(raw);
    return (
    <ThemeProvider theme={theme}>
      <Series>
        {scenes.map((s: any, i: number) => {
          const C = s.type === "custom" ? customRegistry[s.component] : registry[s.type];
          if (!C) throw new Error(`scene ${i + 1}: no component for "${s.type === "custom" ? s.component : s.type}"`);
          return (
            <Series.Sequence key={i} durationInFrames={sceneFrames(s, fps, defaultSec)}>
              <SceneFrame scene={s}><C scene={s} /></SceneFrame>
            </Series.Sequence>
          );
        })}
      </Series>
    </ThemeProvider>
  ); };
  return MasterVideo;
}
