import React from "react";
import { FitText, useEnter, useLayout, useProgress, useSurface, useTheme } from "@video/core";
import { Pill } from "./Pill";

export const Icon: React.FC<{ kind: string; color: string; size: number }> = ({ kind, color, size }) => {
  const p = { fill: "none", stroke: color, strokeWidth: 5, strokeLinecap: "round" as const, strokeLinejoin: "round" as const };
  return (
    <svg width={size} height={size} viewBox="0 0 80 80">
      {kind === "laptop" && <><rect x="14" y="16" width="52" height="34" rx="3" {...p} /><path d="M6 60h68" {...p} /></>}
      {kind === "projector" && <><rect x="8" y="30" width="64" height="26" rx="4" {...p} /><circle cx="52" cy="43" r="8" {...p} /><path d="M18 62v6M62 62v6" {...p} /></>}
      {kind === "desk" && <><path d="M10 56V26h60v30M10 36h60M24 46v6M40 46v6M56 46v6" {...p} /></>}
      {kind === "phone" && <><rect x="26" y="8" width="28" height="64" rx="5" {...p} /><path d="M37 62h6" {...p} /></>}
    </svg>
  );
};

export const Devices: React.FC<{ scene: any }> = ({ scene }) => {
  const { innerW, innerH, portrait } = useLayout();
  const t = useTheme(); const s = useSurface();
  const ms = scene.machines as any[];
  const n = ms.length;
  const eh = useEnter(scene.enter, 0);
  const line = useProgress(ms.length * t.motion.staggerFrames + 10);
  const gap = n > 1 ? 70 : 0;
  const headH = scene.heading ? 130 : 0;
  const areaH = innerH - 40 - headH - 20;
  const cw = portrait ? Math.min(innerW, 700) : Math.min(500, (innerW - gap * (n - 1)) / n);
  const ch = portrait ? Math.min(320, (areaH - gap * (n - 1)) / n) : Math.min(areaH - 70, 560);
  return (
    <div style={{ position: "absolute", left: 96, top: 96, width: innerW, height: innerH - 40, display: "flex", flexDirection: "column", justifyContent: "center", gap: 20 }}>
      {scene.heading && <div style={eh}><FitText text={scene.heading} w={innerW} h={110} maxSize={84} font={t.fonts.display} color={s.accent} weight={700} /></div>}
      <div style={{ display: "flex", flexDirection: portrait ? "column" : "row", justifyContent: "center", alignItems: "center", gap, position: "relative" }}>
        {ms.map((m, i) => <Machine key={i} m={m} i={i} w={cw} h={ch} enter={scene.enter} compact={portrait} />)}
        {n > 1 && scene.link && (
          <div style={{ position: "absolute", left: 0, right: 0, bottom: portrait ? undefined : -46, top: portrait ? -1000 : undefined, display: portrait ? "none" : "flex", justifyContent: "center", opacity: line }}>
            <FitText text={`- - -  ${scene.link}  - - -`} w={innerW} h={40} maxSize={30} font={t.fonts.label} mono color={s.soft} align="center" />
          </div>
        )}
      </div>
    </div>
  );
};

const Machine: React.FC<{ m: any; i: number; w: number; h: number; enter: any; compact: boolean }> = ({ m, i, w, h, enter, compact }) => {
  const t = useTheme(); const s = useSurface();
  const st = useEnter(enter, i * t.motion.staggerFrames * 2);
  const inner = w - 56;
  return (
    <div style={{ ...st, width: w, height: h, boxSizing: "border-box", background: s.card, color: s.cardFg, border: `4px solid ${s.accent}`, borderRadius: t.radius, padding: 24, display: "flex", flexDirection: compact ? "row" : "column", alignItems: "center", justifyContent: "center", gap: 18 }}>
      <Icon kind={m.kind} color={s.accent} size={compact ? 120 : 170} />
      <div style={{ display: "flex", flexDirection: "column", alignItems: compact ? "flex-start" : "center", gap: 10, flex: compact ? 1 : undefined }}>
        <FitText text={m.name} w={compact ? w - 220 : inner} h={compact ? 70 : 90} maxSize={54} font={t.fonts.display} weight={700} color={s.cardFg} align={compact ? "left" : "center"} />
        {m.caption && <FitText text={m.caption} w={compact ? w - 220 : inner} h={compact ? 60 : 80} maxSize={32} font={t.fonts.body} italic color={s.soft} align={compact ? "left" : "center"} />}
        <div style={{ display: "flex", flexWrap: "wrap", gap: 10, justifyContent: compact ? "flex-start" : "center" }}>
          {m.roles.length ? m.roles.map((r: any) => <Pill key={r} role={r} size={compact ? 24 : 28} />) : <FitText text="Helps from the browser" w={inner} h={40} maxSize={28} font={t.fonts.label} mono color={s.soft} align="center" />}
        </div>
      </div>
    </div>
  );
};
