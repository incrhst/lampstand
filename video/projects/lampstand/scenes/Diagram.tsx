import React from "react";
import { Img, staticFile } from "remotion";
import { FitText, useEnter, useLayout, useSurface, useTheme } from "@video/core";

export const Diagram: React.FC<{ scene: any }> = ({ scene }) => {
  const { innerW, innerH } = useLayout();
  const t = useTheme(); const s = useSurface();
  const eh = useEnter(scene.enter, 0), ei = useEnter("fade", 8), ec = useEnter("fade", 18);
  const headH = scene.heading ? 120 : 0, capH = scene.caption ? 100 : 0;
  const boxH = innerH - 40 - headH - capH - 40;
  return (
    <div style={{ position: "absolute", left: 96, top: 96, width: innerW, height: innerH - 40, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 20 }}>
      {scene.heading && <div style={{ ...eh, alignSelf: "flex-start" }}><FitText text={scene.heading} w={innerW} h={100} maxSize={76} font={t.fonts.display} color={s.accent} weight={700} /></div>}
      <div style={{ ...ei, background: "#ffffff", borderRadius: t.radius, padding: 16, boxShadow: "0 12px 40px rgba(0,0,0,0.25)" }}>
        <Img src={staticFile(scene.src)} style={{ display: "block", maxWidth: innerW - 32, maxHeight: boxH - 32 }} />
      </div>
      {scene.caption && <div style={ec}><FitText text={scene.caption} w={innerW} h={capH - 10} maxSize={44} font={t.fonts.body} italic color={s.soft} align="center" /></div>}
    </div>
  );
};
