import React from "react";
import { useLayout, useTheme } from "../theme";
import { useSurface } from "../SceneFrame";
import { useEnter, useProgress } from "../motion/presets";
import { FitText } from "../layout/FitText";

export const Title: React.FC<{ scene: any }> = ({ scene }) => {
  const { innerW, innerH, portrait } = useLayout();
  const t = useTheme(); const s = useSurface();
  const e0 = useEnter(scene.enter, 0), e1 = useEnter(scene.enter, t.motion.staggerFrames), e2 = useEnter("fade", t.motion.staggerFrames * 2);
  const rule = useProgress(t.motion.staggerFrames);
  return (
    <div style={{ position: "absolute", left: 96, top: 96, width: innerW, height: innerH - 40, display: "flex", flexDirection: "column", justifyContent: "center", alignItems: "center", gap: 28 }}>
      {scene.kicker && <div style={e2}><FitText text={scene.kicker} w={innerW} h={44} maxSize={32} font={t.fonts.label} color={s.accent} weight={700} align="center" mono letterSpacing="0.16em" /></div>}
      <div style={e0}><FitText text={scene.heading} w={innerW} h={portrait ? 560 : 340} maxSize={scene.sub ? 190 : 220} minSize={50} font={t.fonts.display} color={s.fg} weight={700} align="center" lineHeight={1} /></div>
      <div style={{ width: 220 * rule, height: 6, background: s.accent }} />
      {scene.sub && <div style={e1}><FitText text={scene.sub} w={innerW * 0.86} h={portrait ? 300 : 190} maxSize={64} font={t.fonts.body} color={s.soft} align="center" lineHeight={1.25} italic /></div>}
    </div>
  );
};

export const Bullets: React.FC<{ scene: any }> = ({ scene }) => {
  const { innerW, innerH, portrait } = useLayout();
  const t = useTheme(); const s = useSurface();
  const n = scene.items.length;
  const headH = scene.heading ? 150 : 0;
  const rowH = Math.min(150, (innerH - 60 - headH) / n);
  const eh = useEnter(scene.enter, 0);
  return (
    <div style={{ position: "absolute", left: 96, top: 96, width: innerW, height: innerH - 40, display: "flex", flexDirection: "column", justifyContent: "center", gap: 14 }}>
      {scene.heading && <div style={eh}><FitText text={scene.heading} w={innerW} h={130} maxSize={96} font={t.fonts.display} color={s.accent} weight={700} /></div>}
      {scene.items.map((item: string, i: number) => <BulletRow key={i} i={i} text={item} w={innerW} h={rowH} enter={scene.enter} headDelay={scene.heading ? 1 : 0} />)}
    </div>
  );
};
const BulletRow: React.FC<{ i: number; text: string; w: number; h: number; enter: any; headDelay: number }> = ({ i, text, w, h, enter, headDelay }) => {
  const t = useTheme(); const s = useSurface();
  const st = useEnter(enter, (i + headDelay) * t.motion.staggerFrames + 6);
  const badge = Math.min(76, h * 0.7);
  return (
    <div style={{ ...st, display: "flex", alignItems: "center", gap: 28, height: h }}>
      <div style={{ width: badge, height: badge, flex: "none", border: `4px solid ${s.accent}`, color: s.accent, borderRadius: "50%", display: "flex", alignItems: "center", justifyContent: "center", fontFamily: t.fonts.label, fontWeight: 700, fontSize: badge * 0.5 }}>{i + 1}</div>
      <FitText text={text} w={w - badge - 28} h={h} maxSize={72} minSize={28} font={t.fonts.body} color={s.fg} lineHeight={1.12} />
    </div>
  );
};

export const Stat: React.FC<{ scene: any }> = ({ scene }) => {
  const { innerW, innerH, portrait } = useLayout();
  const t = useTheme(); const s = useSurface();
  const p = useProgress(4);
  const shown = Math.round(scene.value * Math.min(1, p) * 100) / 100;
  const text = `${scene.prefix ?? ""}${Number.isInteger(scene.value) ? Math.round(shown) : shown.toFixed(1)}${scene.suffix ?? ""}`;
  const el = useEnter("fade", t.motion.staggerFrames * 3);
  return (
    <div style={{ position: "absolute", left: 96, top: 96, width: innerW, height: innerH - 40, display: "flex", flexDirection: "column", justifyContent: "center", alignItems: "center", gap: 24 }}>
      <div style={{ fontFamily: t.fonts.display, fontWeight: 700, fontSize: Math.min(420, innerW / (0.62 * Math.max(2, text.length))), color: s.accent, lineHeight: 1, fontVariantNumeric: "tabular-nums" }}>{text}</div>
      <div style={el}><FitText text={scene.label} w={innerW * 0.9} h={portrait ? 330 : 190} maxSize={68} font={t.fonts.body} color={s.fg} align="center" italic /></div>
    </div>
  );
};

export const Cta: React.FC<{ scene: any }> = ({ scene }) => {
  const { innerW, innerH, portrait } = useLayout();
  const t = useTheme(); const s = useSurface();
  const e0 = useEnter(scene.enter, 0), e1 = useEnter("pop", t.motion.staggerFrames * 2), e2 = useEnter("fade", t.motion.staggerFrames * 3);
  return (
    <div style={{ position: "absolute", left: 96, top: 96, width: innerW, height: innerH - 40, display: "flex", flexDirection: "column", justifyContent: "center", alignItems: "center", gap: 36 }}>
      <div style={e0}><FitText text={scene.heading} w={innerW} h={portrait ? 460 : 280} maxSize={150} minSize={44} font={t.fonts.display} color={s.fg} weight={700} align="center" lineHeight={1} /></div>
      <div style={{ ...e1, background: s.accent, color: s.bg, padding: "22px 48px", borderRadius: t.radius, maxWidth: innerW }}>
        <FitText text={scene.action} w={Math.min(innerW - 100, scene.action.length * 40 + 20)} h={90} maxSize={60} font={t.fonts.label} mono weight={700} color={s.bg} align="center" letterSpacing="0.06em" />
      </div>
      {scene.url && <div style={e2}><FitText text={scene.url} w={innerW} h={60} maxSize={44} font={t.fonts.label} mono color={s.soft} align="center" /></div>}
    </div>
  );
};
