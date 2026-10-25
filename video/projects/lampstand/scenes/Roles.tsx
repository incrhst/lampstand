import React from "react";
import { FitText, useEnter, useLayout, useSurface, useTheme } from "@video/core";
import { roleColors } from "../theme";
import { roleInfo, RoleKey } from "../roles";
import { Pill } from "./Pill";

const Card: React.FC<{ role: RoleKey; i: number; w: number; h: number; enter: any; dim: boolean; big: boolean }> = ({ role, i, w, h, enter, dim, big }) => {
  const t = useTheme(); const s = useSurface();
  const st = useEnter(enter, i * t.motion.staggerFrames + 8);
  const info = roleInfo[role];
  const inner = w - 56;
  return (
    <div style={{ ...st, opacity: ((st.opacity as number) ?? 1) * (dim ? 0.35 : 1), width: w, height: h, boxSizing: "border-box", background: s.card, color: s.cardFg, borderTop: `14px solid ${roleColors[role]}`, borderRadius: t.radius, padding: "26px 28px", display: "flex", flexDirection: "column", gap: 14 }}>
      <Pill role={role} size={big ? 36 : 28} style={{ alignSelf: "flex-start" }} />
      <FitText text={info.job} w={inner} h={big ? 150 : Math.min(190, h * 0.38)} maxSize={big ? 100 : 76} font={t.fonts.display} weight={700} color={s.cardFg} lineHeight={1.05} vAlign="top" />
      <FitText text={info.owns} w={inner} h={big ? 150 : Math.min(240, h * 0.42)} maxSize={big ? 52 : 40} font={t.fonts.body} color={s.cardFg} vAlign="top" />
      {big && <FitText text={info.needs} w={inner} h={110} maxSize={40} font={t.fonts.body} italic color={roleColors[role]} vAlign="top" />}
    </div>
  );
};

export const Roles: React.FC<{ scene: any }> = ({ scene }) => {
  const { innerW, innerH, portrait } = useLayout();
  const t = useTheme(); const s = useSurface();
  const show = scene.show as RoleKey[];
  const eh = useEnter(scene.enter, 0);
  const headH = scene.heading ? 130 : 0;
  const areaH = innerH - 40 - headH - 20;
  let body: React.ReactNode;
  if (scene.focus) {
    const others = show.filter((r) => r !== scene.focus);
    const smallW = portrait ? (innerW - 20 * (others.length - 1)) / Math.max(1, others.length) : innerW / 3 - 14;
    body = portrait ? (
      <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
        <Card role={scene.focus} i={0} w={innerW} h={Math.min(areaH * 0.62, 640)} enter={scene.enter} dim={false} big />
        <div style={{ display: "flex", gap: 20 }}>{others.map((r, i) => <MiniTag key={r} role={r} w={smallW} />)}</div>
      </div>
    ) : (
      <div style={{ display: "flex", gap: 24 }}>
        <Card role={scene.focus} i={0} w={innerW * 0.62} h={Math.min(areaH, 460)} enter={scene.enter} dim={false} big />
        <div style={{ display: "flex", flexDirection: "column", gap: 16, width: innerW * 0.38 - 24 }}>{others.map((r) => <MiniTag key={r} role={r} w={innerW * 0.38 - 24} />)}</div>
      </div>
    );
  } else {
    const cols = portrait ? 1 : show.length > 2 ? Math.min(show.length, 4) : show.length;
    const rows = Math.ceil(show.length / cols);
    const gap = 20;
    const cw = (innerW - gap * (cols - 1)) / cols, ch = (areaH - gap * (rows - 1)) / rows;
    body = <div style={{ display: "flex", flexWrap: "wrap", gap }}>{show.map((r, i) => <Card key={r} role={r} i={i} w={cw} h={Math.min(ch, 520)} enter={scene.enter} dim={false} big={false} />)}</div>;
  }
  return (
    <div style={{ position: "absolute", left: 96, top: 96, width: innerW, height: innerH - 40, display: "flex", flexDirection: "column", justifyContent: "center", gap: 20 }}>
      {scene.heading && <div style={eh}><FitText text={scene.heading} w={innerW} h={110} maxSize={84} font={t.fonts.display} color={s.accent} weight={700} /></div>}
      {body}
    </div>
  );
};

const MiniTag: React.FC<{ role: RoleKey; w: number }> = ({ role, w }) => {
  const t = useTheme(); const s = useSurface();
  return (
    <div style={{ width: w, boxSizing: "border-box", opacity: 0.55, background: s.card, color: s.cardFg, borderLeft: `12px solid ${roleColors[role]}`, borderRadius: t.radius, padding: "16px 20px", display: "flex", flexDirection: "column", gap: 4 }}>
      <div style={{ fontFamily: t.fonts.label, fontWeight: 700, fontSize: 26, color: roleColors[role] }}>{roleInfo[role].name}</div>
      <FitText text={roleInfo[role].job} w={w - 52} h={44} maxSize={34} font={t.fonts.body} color={s.cardFg} vAlign="top" />
    </div>
  );
};
