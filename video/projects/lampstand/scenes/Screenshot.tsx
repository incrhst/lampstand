import React from "react";
import { Img, staticFile } from "remotion";
import { FitText, useEnter, useLayout, useProgress, useSurface, useTheme } from "@video/core";

export const Screenshot: React.FC<{ scene: any }> = ({ scene }) => {
  const { innerW, innerH, portrait } = useLayout();
  const t = useTheme(); const s = useSurface();
  const eh = useEnter(scene.enter, 0), ei = useEnter("fade", 4);
  const cs = scene.callouts as { x: number; y: number; label: string }[];
  const headH = scene.heading ? 110 : 0;
  const legendH = cs.length ? (portrait ? 70 * cs.length : 0) : 0;
  const legendW = !portrait && cs.length ? 520 : 0;
  const boxW = innerW - legendW - (legendW ? 36 : 0);
  const boxH = innerH - 40 - headH - legendH - 20;
  const w = Math.min(boxW, boxH * scene.ratio), h = w / scene.ratio;
  const label = (c: any, i: number) => <Legend key={i} i={i} text={c.label} w={portrait ? innerW : legendW} />;
  return (
    <div style={{ position: "absolute", left: 96, top: 96, width: innerW, height: innerH - 40, display: "flex", flexDirection: "column", justifyContent: "center", gap: 18 }}>
      {scene.heading && <div style={eh}><FitText text={scene.heading} w={innerW} h={100} maxSize={72} font={t.fonts.display} color={s.accent} weight={700} /></div>}
      <div style={{ display: "flex", flexDirection: portrait ? "column" : "row", gap: 36, alignItems: portrait ? "center" : "center" }}>
        <div style={{ ...ei, position: "relative", width: w, height: h, flex: "none", borderRadius: t.radius, boxShadow: "0 14px 50px rgba(0,0,0,0.5)", border: `2px solid ${s.line}` }}>
          <Img src={staticFile(scene.src)} style={{ width: w, height: h, borderRadius: t.radius, display: "block" }} />
          {cs.map((c, i) => <Marker key={i} i={i} x={c.x} y={c.y} w={w} />)}
        </div>
        {cs.length > 0 && <div style={{ display: "flex", flexDirection: "column", gap: 18, width: portrait ? innerW : legendW }}>{cs.map(label)}</div>}
      </div>
    </div>
  );
};

const START = 24;
const Marker: React.FC<{ i: number; x: number; y: number; w: number }> = ({ i, x, y, w }) => {
  const t = useTheme(); const s = useSurface();
  const p = useProgress(START + i * 28);
  const size = Math.max(44, w * 0.04);
  return (
    <div style={{ position: "absolute", left: `${x}%`, top: `${y}%`, width: size, height: size, marginLeft: -size / 2, marginTop: -size / 2, borderRadius: "50%", background: s.accent, color: s.bg, border: `4px solid ${s.bg}`, boxSizing: "border-box", display: "flex", alignItems: "center", justifyContent: "center", fontFamily: t.fonts.label, fontWeight: 700, fontSize: size * 0.55, transform: `scale(${p})`, opacity: Math.min(1, p) }}>{i + 1}</div>
  );
};
const Legend: React.FC<{ i: number; text: string; w: number }> = ({ i, text, w }) => {
  const t = useTheme(); const s = useSurface();
  const st = useEnter("rise", START + i * 28);
  return (
    <div style={{ ...st, display: "flex", alignItems: "center", gap: 20, width: w }}>
      <div style={{ width: 52, height: 52, flex: "none", borderRadius: "50%", background: s.accent, color: s.bg, display: "flex", alignItems: "center", justifyContent: "center", fontFamily: t.fonts.label, fontWeight: 700, fontSize: 28 }}>{i + 1}</div>
      <FitText text={text} w={w - 72} h={90} maxSize={44} font={t.fonts.body} color={s.fg} />
    </div>
  );
};
