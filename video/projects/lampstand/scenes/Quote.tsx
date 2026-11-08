import React from "react";
import { FitText, useEnter, useLayout, useSurface, useTheme } from "@video/core";

export const Quote: React.FC<{ scene: any }> = ({ scene }) => {
  const { innerW, innerH, portrait } = useLayout();
  const t = useTheme(); const s = useSurface();
  const e0 = useEnter("fade", 0), e1 = useEnter(scene.enter, 5), e2 = useEnter("fade", 24);
  return (
    <div style={{ position: "absolute", left: 96, top: 96, width: innerW, height: innerH - 40, display: "flex", flexDirection: "column", justifyContent: "center", gap: 20 }}>
      <div style={{ ...e0, fontFamily: t.fonts.display, fontSize: 260, lineHeight: 0.6, height: 130, color: s.accent }}>“</div>
      <div style={e1}><FitText text={scene.text} w={innerW} h={portrait ? 800 : 440} maxSize={120} minSize={36} font={t.fonts.display} color={s.fg} weight={700} lineHeight={1.1} /></div>
      {scene.attribution && <div style={e2}><FitText text={`— ${scene.attribution}`} w={innerW} h={70} maxSize={42} font={t.fonts.label} mono color={s.accent} letterSpacing="0.08em" /></div>}
    </div>
  );
};
