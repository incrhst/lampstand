import React from "react";
import { interpolate, useCurrentFrame, useVideoConfig, Easing } from "remotion";
import { FitText, useEnter, useLayout, useSurface, useTheme } from "@video/core";
import { roleColors } from "../theme";
import { roleInfo } from "../roles";
import { Pill } from "./Pill";
import { Icon } from "./Devices";

const CAPTIONS = {
  allow: "The holder chooses Allow, and the job moves.",
  decline: "The holder chooses Not now, and nothing changes.",
  cut: "The holder has been gone for two minutes, so the job can be taken over.",
};

/** One handover, played out: who asks, what the holder sees, where the job ends up. Times are seconds into the scene. */
export const Handover: React.FC<{ scene: any }> = ({ scene }) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const { innerW, innerH, portrait } = useLayout();
  const t = useTheme(); const s = useSurface();
  const sec = frame / fps;
  const cut = scene.outcome === "cut";
  const T = cut ? { ask: 1.2, dead: 1.2, clock: 2.2, take: 6.0, move: 7.2, prompt: 99, answer: 99 } : { ask: 1.4, prompt: 2.8, answer: 5.2, move: 6.4, dead: 99, clock: 99, take: 99 };
  const ease = Easing.inOut(Easing.cubic);
  const at = (a: number, b: number) => interpolate(sec, [a, b], [0, 1], { extrapolateLeft: "clamp", extrapolateRight: "clamp" });
  const moved = scene.outcome === "decline" ? 0 : ease(at(T.move, T.move + 1));
  const info = roleInfo[scene.role as keyof typeof roleInfo];
  const verb = info.job.replace(/^Runs/, "run");

  const gap = portrait ? 150 : 300;
  const cw = portrait ? Math.min(innerW, 640) : 520;
  const ch = portrait ? 280 : 360;
  const dx = portrait ? 0 : cw + gap, dy = portrait ? ch + gap : 0;
  const baseH = portrait ? ch * 2 + gap : ch;
  const promptTop = portrait ? baseH + 16 + 64 : ch + 22;
  const stageW = portrait ? cw : cw * 2 + gap, stageH = promptTop + 150;
  const holderDim = cut ? 1 - 0.65 * at(T.dead, T.dead + 0.8) : 1;
  const eh = useEnter("fade", 0), e1 = useEnter("pop", 4), e2 = useEnter("pop", 12);
  const caption = scene.caption ?? CAPTIONS[scene.outcome as keyof typeof CAPTIONS];
  const capShow = at(cut ? T.take : T.answer, (cut ? T.take : T.answer) + 0.6);

  const Card = ({ m, x, y, dim = 1, children }: any) => (
    <div style={{ position: "absolute", left: x, top: y, width: cw, height: ch, boxSizing: "border-box", background: s.card, color: s.cardFg, border: `4px solid ${s.accent}`, borderRadius: t.radius, opacity: dim, padding: 22, display: "flex", flexDirection: portrait ? "row" : "column", alignItems: "center", justifyContent: "center", gap: 14 }}>
      <Icon kind={m.kind} color={s.accent} size={portrait ? 110 : 120} />
      <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 12 }}>
        <FitText text={m.name} w={portrait ? cw - 200 : cw - 56} h={64} maxSize={48} font={t.fonts.display} weight={700} color={s.cardFg} align="center" />
        {children}
      </div>
    </div>
  );
  // The job's slot is the same place in each card; the pill slides between them.
  const slot = { left: 0, top: 0 };
  const pillX = dx * moved, pillY = dy * moved;

  // Speech bubble from the asker, and the holder's prompt.
  const askShow = at(T.ask, T.ask + 0.5);
  const promptShow = !cut ? at(T.prompt, T.prompt + 0.5) : 0;
  const pick = scene.outcome === "allow" ? at(T.answer, T.answer + 0.3) : scene.outcome === "decline" ? at(T.answer, T.answer + 0.3) : 0;
  const clockFrac = cut ? at(T.clock, T.take - 0.4) : 0;
  const mins = Math.min(2, Math.floor(clockFrac * 2.999));
  const btn = (label: string, on: boolean, tone: string) => (
    <div style={{ fontFamily: t.fonts.label, fontWeight: 700, fontSize: 28, padding: "10px 24px", borderRadius: t.radius, border: `3px solid ${tone}`, background: on ? tone : "transparent", color: on ? "#fff" : tone }}>{label}</div>
  );

  return (
    <div style={{ position: "absolute", left: 96, top: 96, width: innerW, height: innerH - 40, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 36 }}>
      <div style={{ ...eh, alignSelf: "flex-start" }}><FitText text={cut ? "If the holder is cut off" : scene.outcome === "allow" ? "Asking for a job" : "Saying Not now"} w={innerW} h={100} maxSize={76} font={t.fonts.display} color={s.accent} weight={700} /></div>
      <div style={{ position: "relative", width: stageW, height: stageH }}>
        <div style={e1}>
          <Card m={scene.holder} x={0} y={0} dim={holderDim}>
            <FitText text="Holds the job" w={cw - 56} h={34} maxSize={26} font={t.fonts.label} mono color={s.soft} align="center" />
          </Card>
        </div>
        <div style={e2}>
          <Card m={scene.asker} x={dx} y={dy}>
            <FitText text={cut && sec >= T.take ? "Take over" : "Asks for the job"} w={cw - 56} h={34} maxSize={26} font={t.fonts.label} mono color={s.soft} align="center" />
          </Card>
        </div>
        {/* the job */}
        <div style={{ position: "absolute", left: cw / 2 - 90 + slot.left + pillX, top: ch - 56 + pillY + slot.top, width: 180, display: "flex", justifyContent: "center" }}>
          <Pill role={scene.role} size={28} />
        </div>
        {/* link between them: solid, or cut */}
        <div style={{ position: "absolute", left: portrait ? cw / 2 : cw, top: portrait ? ch : ch / 2, width: portrait ? 0 : gap, height: portrait ? gap : 0, borderTop: portrait ? undefined : `5px ${cut ? "dashed" : "solid"} ${s.soft}`, borderLeft: portrait ? `5px ${cut ? "dashed" : "solid"} ${s.soft}` : undefined, opacity: cut ? 1 - 0.7 * at(T.dead, T.dead + 0.6) : 0.8 }} />
        {cut && <div style={{ position: "absolute", left: portrait ? cw / 2 - 32 : cw + gap / 2 - 32, top: portrait ? ch + gap / 2 - 32 : ch / 2 - 32, width: 64, height: 64, borderRadius: "50%", background: "#b3261e", color: "#fff", display: "flex", alignItems: "center", justifyContent: "center", fontFamily: t.fonts.label, fontWeight: 700, fontSize: 40, opacity: at(T.dead, T.dead + 0.5), transform: `scale(${at(T.dead, T.dead + 0.5)})` }}>×</div>}
        {/* the asker's request */}
        <div style={{ position: "absolute", left: portrait ? cw + 24 : dx, top: portrait ? dy + 10 : ch + 22, opacity: askShow && !(cut && sec >= T.take) ? askShow : 0, transform: `translateY(${(1 - askShow) * 12}px)`, ...(portrait ? { display: "none" } : {}) }}>
          <div style={{ width: cw, background: roleColors[scene.role as keyof typeof roleColors], color: "#fff", borderRadius: t.radius, padding: "12px 20px", fontFamily: t.fonts.label, fontWeight: 700, fontSize: 28, textAlign: "center", boxSizing: "border-box" }}>Press “Take {info.name}”</div>
        </div>
        {portrait && <div style={{ position: "absolute", left: 0, top: dy + ch + 16, width: cw, opacity: askShow && !(cut && sec >= T.take) ? askShow : 0 }}>
          <div style={{ background: roleColors[scene.role as keyof typeof roleColors], color: "#fff", borderRadius: t.radius, padding: "12px 20px", fontFamily: t.fonts.label, fontWeight: 700, fontSize: 28, textAlign: "center" }}>Press “Take {info.name}”</div>
        </div>}
        {/* what the holder sees */}
        {!cut && (
          <div style={{ position: "absolute", left: 0, top: promptTop, width: cw, boxSizing: "border-box", opacity: promptShow, transform: `translateY(${(1 - promptShow) * 12}px)`, background: s.bg === "#f2c452" ? "#fff" : s.card, border: `3px solid ${s.fg}`, borderRadius: t.radius, padding: 18, display: "flex", flexDirection: "column", gap: 14, color: s.fg }}>
            <div style={{ fontFamily: t.fonts.body, fontSize: 30, lineHeight: 1.15 }}>{scene.asker.name} wants to {verb}.</div>
            <div style={{ display: "flex", gap: 14 }}>
              {btn("Allow", scene.outcome === "allow" && pick > 0.5, "#1d7a46")}
              {btn("Not now", scene.outcome === "decline" && pick > 0.5, "#b3261e")}
            </div>
          </div>
        )}
        {/* the clock, when the holder is cut off */}
        {cut && (
          <div style={{ position: "absolute", left: 0, top: promptTop, width: cw, boxSizing: "border-box", opacity: at(T.clock, T.clock + 0.5), background: s.bg === "#f2c452" ? "#fff" : s.card, border: `3px solid ${s.fg}`, borderRadius: t.radius, padding: 18, color: s.fg }}>
            <div style={{ fontFamily: t.fonts.body, fontSize: 30, lineHeight: 1.15 }}>{scene.holder.name} has been gone {mins === 0 ? "less than a minute" : mins === 1 ? "1 minute" : "2 minutes"}</div>
            <div style={{ marginTop: 14, height: 14, background: s.line, borderRadius: 7 }}><div style={{ height: 14, width: `${clockFrac * 100}%`, background: s.accent, borderRadius: 7 }} /></div>
          </div>
        )}
      </div>
      <div style={{ opacity: capShow, minHeight: portrait ? 180 : 110 }}>
        <FitText text={caption} w={innerW} h={portrait ? 180 : 110} maxSize={50} font={t.fonts.body} italic color={s.fg} align="center" />
      </div>
    </div>
  );
};
