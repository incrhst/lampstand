import React from "react";
import {
  AbsoluteFill,
  Audio,
  Easing,
  Sequence,
  interpolate,
  spring,
  staticFile,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";
import { loadFont as loadSerif } from "@remotion/google-fonts/Fraunces";
import { loadFont as loadSans } from "@remotion/google-fonts/Inter";
import {
  BookOpen,
  Check,
  Library,
  Megaphone,
  MonitorCheck,
  Music,
  Play,
  Presentation,
  Users,
  WifiOff,
} from "lucide-react";
import T from "./timeline.json";

const { fontFamily: SERIF } = loadSerif("normal", { weights: ["400", "600"], subsets: ["latin"] });
const { fontFamily: SANS } = loadSans("normal", { weights: ["400", "500", "600"], subsets: ["latin"] });

const C = {
  bg: "#0D1220",
  panel: "#151C30",
  panel2: "#1B2440",
  gold: "#F2B544",
  cream: "#F6F1E7",
  muted: "#9AA4B8",
  line: "rgba(246,241,231,0.12)",
  green: "#5CC48A",
  blue: "#7FA7D9",
  coral: "#E8806A",
  violet: "#B59CE6",
};

const CLAMP = { extrapolateLeft: "clamp", extrapolateRight: "clamp" } as const;
const EASE = Easing.out(Easing.cubic);
const FADE = 12;

const ramp = (f: number, a: number, b: number, from = 0, to = 1) =>
  interpolate(f, [a, b], [from, to], { ...CLAMP, easing: EASE });

const useSpr = (delay: number, damping = 14, stiffness = 120) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  return spring({ frame: frame - delay, fps, config: { damping, stiffness } });
};

// ---------- shared pieces ----------

const Backdrop: React.FC = () => {
  const frame = useCurrentFrame();
  const drift = Math.sin(frame / 90) * 60;
  return (
    <AbsoluteFill
      style={{
        background: `radial-gradient(1200px 800px at ${1500 + drift}px 200px, rgba(242,181,68,0.10), transparent 60%),
          radial-gradient(900px 700px at 200px ${900 - drift}px, rgba(127,167,217,0.08), transparent 60%), ${C.bg}`,
      }}
    />
  );
};

const Shell: React.FC<{ dur: number; children: React.ReactNode }> = ({ dur, children }) => {
  const frame = useCurrentFrame();
  const opacity = interpolate(frame, [0, FADE, dur - FADE, dur], [0, 1, 1, 0], CLAMP);
  const scale = interpolate(frame, [0, FADE * 2], [1.03, 1], { ...CLAMP, easing: EASE });
  return <AbsoluteFill style={{ opacity, transform: `scale(${scale})` }}>{children}</AbsoluteFill>;
};

const Words: React.FC<{ text: string; delay: number; style: React.CSSProperties; step?: number }> = ({
  text,
  delay,
  style,
  step = 3,
}) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  return (
    <div style={style}>
      {text.split(" ").map((w, i) => {
        const p = spring({ frame: frame - delay - i * step, fps, config: { damping: 200 } });
        return (
          <span
            key={i}
            style={{
              display: "inline-block",
              marginRight: "0.26em",
              opacity: p,
              transform: `translateY(${interpolate(p, [0, 1], [28, 0])}px)`,
            }}
          >
            {w}
          </span>
        );
      })}
    </div>
  );
};

const Copy: React.FC<{ kicker: string; title: string; sub: string }> = ({ kicker, title, sub }) => {
  const frame = useCurrentFrame();
  const k = ramp(frame, 4, 18);
  const s = ramp(frame, 30, 48);
  return (
    <div style={{ position: "absolute", left: 120, top: 0, bottom: 0, width: 690, display: "flex", flexDirection: "column", justifyContent: "center" }}>
      <div
        style={{
          fontFamily: SANS,
          fontWeight: 600,
          fontSize: 22,
          letterSpacing: "0.22em",
          color: C.gold,
          opacity: k,
          transform: `translateX(${(1 - k) * -20}px)`,
          marginBottom: 26,
        }}
      >
        {kicker}
      </div>
      <Words
        text={title}
        delay={10}
        style={{ fontFamily: SERIF, fontWeight: 600, fontSize: 84, lineHeight: 1.04, color: C.cream, letterSpacing: "-0.01em" }}
      />
      <div
        style={{
          fontFamily: SANS,
          fontSize: 31,
          lineHeight: 1.45,
          color: C.muted,
          marginTop: 34,
          maxWidth: 640,
          opacity: s,
          transform: `translateY(${(1 - s) * 16}px)`,
        }}
      >
        {sub}
      </div>
    </div>
  );
};

const Pill: React.FC<{ color: string; children: React.ReactNode; style?: React.CSSProperties }> = ({ color, children, style }) => (
  <div
    style={{
      display: "inline-flex",
      alignItems: "center",
      gap: 10,
      padding: "10px 18px",
      borderRadius: 999,
      background: `${color}22`,
      border: `1.5px solid ${color}66`,
      color,
      fontFamily: SANS,
      fontWeight: 600,
      fontSize: 22,
      ...style,
    }}
  >
    {children}
  </div>
);

const popStyle = (p: number): React.CSSProperties => ({
  opacity: Math.min(1, p * 1.5),
  transform: `scale(${interpolate(p, [0, 1], [0.6, 1])})`,
});

// ---------- scene: open ----------

const Open: React.FC = () => {
  const frame = useCurrentFrame();
  const e = T.events.open;
  const dur = T.scenes.open.dur;
  const l1 = ramp(frame, e.line1, e.line1 + 18);
  const l2 = ramp(frame, e.line2, e.line2 + 18);
  const out = interpolate(frame, [dur - 14, dur], [1, 0], CLAMP);
  const ticked = e.ticks.filter((t) => frame >= t).length; // second hand steps with each tick sound
  return (
    <AbsoluteFill style={{ justifyContent: "center", alignItems: "center", opacity: out }}>
      <svg width={120} height={120} viewBox="-60 -60 120 120" style={{ opacity: l1, marginBottom: 40 }}>
        <circle r={54} fill="none" stroke={C.gold} strokeWidth={3} opacity={0.8} />
        <line x1={0} y1={0} x2={0} y2={-34} stroke={C.cream} strokeWidth={4} strokeLinecap="round" transform="rotate(-6)" />
        <line x1={0} y1={0} x2={0} y2={-46} stroke={C.gold} strokeWidth={2.5} strokeLinecap="round" transform={`rotate(${330 + ticked * 6})`} />
        <circle r={4} fill={C.gold} />
      </svg>
      <div style={{ fontFamily: SANS, fontWeight: 500, fontSize: 30, letterSpacing: "0.3em", color: C.gold, opacity: l1, transform: `translateY(${(1 - l1) * 14}px)` }}>
        SUNDAY · 9:58 A.M.
      </div>
      <Words
        text="The worship leader has shared the songs."
        delay={e.line2}
        step={3}
        style={{ fontFamily: SERIF, fontWeight: 600, fontSize: 84, color: C.cream, marginTop: 30, opacity: l2 > 0 ? 1 : 0 }}
      />
      <Words
        text="Is the screen ready?"
        delay={e.line3}
        step={4}
        style={{ fontFamily: SERIF, fontWeight: 600, fontSize: 84, color: C.gold, marginTop: 10 }}
      />
    </AbsoluteFill>
  );
};

// ---------- scene: plan together ----------

const ITEMS = [
  { icon: Music, label: "Great Is Thy Faithfulness", kind: "Song", who: "Worship leader", color: C.gold },
  { icon: BookOpen, label: "John 3:16–18", kind: "Scripture", who: "Pastor", color: C.blue },
  { icon: Presentation, label: "Hope Remains.pptx", kind: "Sermon", who: "Pastor", color: C.blue },
  { icon: Megaphone, label: "Announcements loop", kind: "Welcome", who: "Office", color: C.violet },
];

const Plan: React.FC = () => {
  const frame = useCurrentFrame();
  const e = T.events.plan;
  const card = useSpr(6, 200);
  const sync = useSpr(e.sync, 12, 140);
  const pulse = 0.5 + 0.5 * Math.sin(frame / 6);
  return (
    <Shell dur={T.scenes.plan.dur}>
      <Copy
        kicker="01 · PLAN TOGETHER"
        title="Plan together. Arrive ready."
        sub="The whole team builds the service from home during the week. On Sunday the projection laptop already has it, no internet needed."
      />
      <div
        style={{
          position: "absolute",
          left: 920,
          top: 170,
          width: 860,
          padding: 36,
          borderRadius: 28,
          background: C.panel,
          border: `1.5px solid ${C.line}`,
          boxShadow: "0 40px 80px rgba(0,0,0,0.35)",
          opacity: card,
          transform: `translateY(${(1 - card) * 40}px)`,
        }}
      >
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 26 }}>
          <div>
            <div style={{ fontFamily: SANS, fontSize: 20, color: C.muted, letterSpacing: "0.14em" }}>SERVICE ORDER</div>
            <div style={{ fontFamily: SERIF, fontWeight: 600, fontSize: 40, color: C.cream, marginTop: 4 }}>Sunday 10:00</div>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
            <div style={{ width: 12, height: 12, borderRadius: 6, background: C.green, opacity: 0.4 + pulse * 0.6 }} />
            <span style={{ fontFamily: SANS, fontSize: 20, color: C.green, fontWeight: 600, letterSpacing: "0.1em" }}>EDITING LIVE</span>
            <div style={{ display: "flex", marginLeft: 8 }}>
              {[C.gold, C.blue, C.violet].map((c, i) => (
                <div
                  key={c}
                  style={{
                    width: 44,
                    height: 44,
                    borderRadius: 22,
                    background: c,
                    border: `3px solid ${C.panel}`,
                    marginLeft: i ? -12 : 0,
                    ...popStyle(ramp(frame, e.items[0] - 20 + i * 10, e.items[0] - 6 + i * 10)),
                  }}
                />
              ))}
            </div>
          </div>
        </div>
        {ITEMS.map((it, i) => {
          const p = spring({ frame: frame - e.items[i], fps: 30, config: { damping: 15, stiffness: 140 } });
          const Icon = it.icon;
          return (
            <div
              key={it.label}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 22,
                height: 92,
                padding: "0 24px",
                marginTop: 14,
                borderRadius: 18,
                background: C.panel2,
                borderLeft: `6px solid ${it.color}`,
                opacity: Math.min(1, p * 1.4),
                transform: `translateX(${(1 - p) * 60}px)`,
              }}
            >
              <Icon size={34} color={it.color} strokeWidth={2} />
              <div style={{ flex: 1 }}>
                <div style={{ fontFamily: SANS, fontSize: 18, color: C.muted, letterSpacing: "0.1em" }}>{it.kind.toUpperCase()}</div>
                <div style={{ fontFamily: SANS, fontWeight: 600, fontSize: 30, color: C.cream }}>{it.label}</div>
              </div>
              <Pill color={it.color} style={{ fontSize: 19, padding: "8px 16px" }}>
                {it.who}
              </Pill>
            </div>
          );
        })}
      </div>
      <div style={{ position: "absolute", left: 920, top: 838, ...popStyle(sync) }}>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 20,
            padding: "20px 30px",
            borderRadius: 20,
            background: "rgba(92,196,138,0.12)",
            border: `2px solid ${C.green}88`,
          }}
        >
          <WifiOff size={30} color={C.muted} />
          <span style={{ fontFamily: SANS, fontSize: 26, color: C.muted }}>Projection laptop, offline</span>
          <Check size={34} color={C.green} strokeWidth={3} />
          <span style={{ fontFamily: SANS, fontSize: 28, fontWeight: 600, color: C.green }}>Ready for Sunday</span>
        </div>
      </div>
    </Shell>
  );
};

// ---------- scene: every screen ----------

const TVW = 840;
const TVH = 472;
const CROP = 46;

const Screens: React.FC = () => {
  const frame = useCurrentFrame();
  const e = T.events.screens;
  const tv = useSpr(4, 200);
  const before = 1 - ramp(frame, e.calib - 6, e.calib + 6);
  const grid = ramp(frame, e.calib, e.calib + 12) * (1 - ramp(frame, e.done - 8, e.done + 4));
  const after = ramp(frame, e.done, e.done + 14);
  const target = CROP + 14;
  const edge = (i: number) => ramp(frame, e.edges[i], e.edges[i] + e.edgeDur, 0, target);
  const [mt, mr, mb, ml] = [0, 1, 2, 3].map(edge);
  const shake = frame > e.cropLabel - 4 && frame < e.cropLabel + 8 ? Math.sin(frame * 3) * 4 : 0;
  const labelBefore = ramp(frame, e.cropLabel, e.cropLabel + 12) * before;
  const labelAfter = useSpr(e.done + 6, 13, 140);
  const nums = Array.from({ length: 9 }, (_, i) => i + 1);

  return (
    <Shell dur={T.scenes.screens.dur}>
      <Copy
        kicker="02 · FITS EVERY SCREEN"
        title="Every word on every screen."
        sub="Calibrate each screen in under two minutes, standing in the room. Lyrics and scripture never fall off the edge again."
      />
      <div style={{ position: "absolute", left: 920, top: 190, opacity: tv, transform: `translateY(${(1 - tv) * 40}px)` }}>
        <div
          style={{
            width: TVW,
            height: TVH,
            borderRadius: 16,
            border: "16px solid #05070C",
            boxShadow: "0 40px 90px rgba(0,0,0,0.45)",
            position: "relative",
            overflow: "hidden",
            background: "linear-gradient(160deg, #22305A, #121a33)",
            transform: `translateX(${shake}px)`,
          }}
        >
          {/* before: lyrics laid out to the full frame, so the TV crops them */}
          <div style={{ position: "absolute", inset: 0, display: "flex", alignItems: "center", justifyContent: "center", opacity: before }}>
            <div style={{ fontFamily: SERIF, fontWeight: 600, fontSize: 70, color: C.cream, whiteSpace: "nowrap", textAlign: "center", lineHeight: 1.2 }}>
              Amazing grace, how sweet
              <br />
              the sound that saved a wretch
            </div>
          </div>
          {/* calibration grid */}
          <div style={{ position: "absolute", inset: 0, opacity: grid }}>
            <svg width="100%" height="100%" style={{ position: "absolute", inset: 0 }}>
              {Array.from({ length: 17 }, (_, i) => (
                <line key={`v${i}`} x1={i * 52} y1={0} x2={i * 52} y2={TVH} stroke="rgba(246,241,231,0.14)" />
              ))}
              {Array.from({ length: 10 }, (_, i) => (
                <line key={`h${i}`} x1={0} y1={i * 52} x2={TVW} y2={i * 52} stroke="rgba(246,241,231,0.14)" />
              ))}
            </svg>
            {nums.map((n) => (
              <React.Fragment key={n}>
                <span style={{ position: "absolute", top: 4 + (n - 1) * 5.4, left: (n / 10) * (TVW - 32) - 10, fontFamily: SANS, fontWeight: 600, fontSize: 22, color: C.gold }}>{n}</span>
                <span style={{ position: "absolute", left: 6 + (n - 1) * 5.4, top: (n / 10) * (TVH - 32) - 10, fontFamily: SANS, fontWeight: 600, fontSize: 22, color: C.blue }}>{n}</span>
              </React.Fragment>
            ))}
            <div style={{ position: "absolute", top: mt, right: mr, bottom: mb, left: ml, border: `3px solid ${C.gold}` }}>
              {[
                { top: -9, left: "50%" },
                { right: -9, top: "50%" },
                { bottom: -9, left: "50%" },
                { left: -9, top: "50%" },
              ].map((pos, i) => (
                <div key={i} style={{ position: "absolute", width: 16, height: 16, borderRadius: 8, background: C.gold, transform: "translate(-50%,0)", ...pos }} />
              ))}
            </div>
          </div>
          {/* after: text laid out inside the safe area */}
          <div
            style={{
              position: "absolute",
              inset: target,
              border: `2.5px dashed ${C.gold}aa`,
              borderRadius: 6,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              opacity: after,
            }}
          >
            <div style={{ fontFamily: SERIF, fontWeight: 600, fontSize: 36, color: C.cream, textAlign: "center", lineHeight: 1.35, whiteSpace: "nowrap", transform: `scale(${0.92 + after * 0.08})` }}>
              Amazing grace, how sweet the sound
              <br />
              that saved a wretch like me
            </div>
          </div>
          {/* the part of the panel this TV hides (overscan) */}
          <div style={{ position: "absolute", inset: 0, boxShadow: `inset 0 0 0 ${CROP}px #000`, pointerEvents: "none" }} />
        </div>
        <div style={{ width: 160, height: 46, margin: "0 auto", background: "#05070C", clipPath: "polygon(30% 0, 70% 0, 100% 100%, 0 100%)" }} />
        <div style={{ position: "relative", height: 70, marginTop: 24 }}>
          <div style={{ position: "absolute", left: 0, opacity: labelBefore }}>
            <Pill color={C.coral}>The TV crops the edges. Words go missing.</Pill>
          </div>
          <div style={{ position: "absolute", left: 0, ...popStyle(labelAfter) }}>
            <Pill color={C.green}>
              <Check size={26} strokeWidth={3} /> Calibrated. Everything fits.
            </Pill>
          </div>
        </div>
      </div>
    </Shell>
  );
};

// ---------- scene: slides ----------

const BULLETS = ["Hope is a person", "Hope is a practice", "Hope is shared"];

const Slides: React.FC = () => {
  const frame = useCurrentFrame();
  const e = T.events.slides;
  const slide = useSpr(6, 200);
  const chipA = useSpr(e.chips[0], 12, 150);
  const chipB = useSpr(e.chips[1], 12, 150);
  const vid = ramp(frame, e.video, e.video + 70);
  const notes = useSpr(e.notes, 200);
  const badge = useSpr(e.badge, 12, 140);
  return (
    <Shell dur={T.scenes.slides.dur}>
      <Copy
        kicker="03 · SLIDES THAT JUST PLAY"
        title="Their slides, exactly as they made them."
        sub="Drop in PowerPoint or pick a Google Slides deck. Animations, video and speaker notes play intact, with no PowerPoint on the laptop."
      />
      <div style={{ position: "absolute", left: 920, top: 120, display: "flex", gap: 16 }}>
        <div style={popStyle(chipA)}>
          <Pill color={C.coral}>
            <Presentation size={24} /> PowerPoint
          </Pill>
        </div>
        <div style={popStyle(chipB)}>
          <Pill color={C.gold}>
            <Presentation size={24} /> Google Slides
          </Pill>
        </div>
      </div>
      <div
        style={{
          position: "absolute",
          left: 920,
          top: 200,
          width: 840,
          height: 472,
          borderRadius: 14,
          background: "#FBFAF7",
          boxShadow: "0 40px 90px rgba(0,0,0,0.45)",
          overflow: "hidden",
          opacity: slide,
          transform: `translateY(${(1 - slide) * 40}px)`,
        }}
      >
        <div style={{ position: "absolute", left: 0, top: 0, bottom: 0, width: 14, background: C.gold }} />
        <div style={{ position: "absolute", left: 60, top: 48 }}>
          <div style={{ fontFamily: SANS, fontWeight: 600, fontSize: 20, color: "#8A6A22", letterSpacing: "0.16em" }}>ROMANS 15:13</div>
          <div style={{ fontFamily: SERIF, fontWeight: 600, fontSize: 64, color: C.bg, marginTop: 6 }}>Hope Remains</div>
          <div style={{ marginTop: 26 }}>
            {BULLETS.map((b, i) => {
              const p = spring({ frame: frame - e.builds[i], fps: 30, config: { damping: 16, stiffness: 130 } });
              return (
                <div
                  key={b}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 16,
                    fontFamily: SANS,
                    fontWeight: 500,
                    fontSize: 32,
                    color: "#26304A",
                    marginTop: 18,
                    opacity: Math.min(1, p * 1.3),
                    transform: `translateX(${(1 - p) * -80}px)`,
                  }}
                >
                  <div style={{ width: 12, height: 12, borderRadius: 6, background: C.gold }} />
                  {b}
                </div>
              );
            })}
          </div>
        </div>
        <div
          style={{
            position: "absolute",
            right: 44,
            top: 150,
            width: 300,
            height: 200,
            borderRadius: 12,
            overflow: "hidden",
            background: `linear-gradient(${120 + vid * 120}deg, #2B3D6B, #6E5A9E ${40 + vid * 30}%, #E8A85C)`,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          <div style={{ width: 70, height: 70, borderRadius: 35, background: "rgba(255,255,255,0.88)", display: "flex", alignItems: "center", justifyContent: "center", opacity: 1 - vid * 0.9 }}>
            <Play size={32} color={C.bg} fill={C.bg} />
          </div>
          <div style={{ position: "absolute", left: 0, bottom: 0, height: 6, width: `${vid * 100}%`, background: C.gold }} />
        </div>
      </div>
      <div
        style={{
          position: "absolute",
          left: 920,
          top: 700,
          width: 840,
          padding: "22px 28px",
          borderRadius: 18,
          background: C.panel,
          border: `1.5px solid ${C.line}`,
          opacity: notes,
          transform: `translateY(${(1 - notes) * 24}px)`,
        }}
      >
        <div style={{ fontFamily: SANS, fontSize: 18, color: C.blue, letterSpacing: "0.14em", fontWeight: 600 }}>SPEAKER NOTES → STAGE DISPLAY</div>
        <div style={{ fontFamily: SANS, fontSize: 27, color: C.cream, marginTop: 8 }}>Pause here. Ask: where did you see hope this week?</div>
      </div>
      <div style={{ position: "absolute", left: 920, top: 860, ...popStyle(badge) }}>
        <Pill color={C.green}>
          <Check size={26} strokeWidth={3} /> No PowerPoint install needed
        </Pill>
      </div>
    </Shell>
  );
};

// ---------- scene: migrate ----------

const SOURCES = ["OpenSong", "OpenLP", "EasyWorship", "ProPresenter", "FreeShow"];
const SONGS = 1284;

const Migrate: React.FC = () => {
  const frame = useCurrentFrame();
  const e = T.events.migrate;
  const box = useSpr(8, 200);
  const count = Math.round(interpolate(frame, [e.countFrom, e.countTo], [0, SONGS], { ...CLAMP, easing: Easing.out(Easing.quad) }));
  const boxGlow = ramp(frame, e.countTo - 4, e.countTo + 6) * (1 - ramp(frame, e.countTo + 10, e.countTo + 40));
  return (
    <Shell dur={T.scenes.migrate.dur}>
      <Copy
        kicker="04 · SWITCH IN AN HOUR"
        title="Bring everything with you."
        sub="Songs, sets, themes and media come across from the tool you use today, in one pass. Most churches are done in under an hour."
      />
      {SOURCES.map((s, i) => {
        const appear = spring({ frame: frame - e.sources[i], fps: 30, config: { damping: 12, stiffness: 150 } });
        const flyStart = e.fly + i * e.flyStep;
        const fly = interpolate(frame, [flyStart, flyStart + 18], [0, 1], { ...CLAMP, easing: Easing.inOut(Easing.cubic) });
        const y0 = 230 + i * 128;
        const x = interpolate(fly, [0, 1], [920, 1440]);
        const y = interpolate(fly, [0, 1], [y0, 470]) - Math.sin(fly * Math.PI) * 80;
        return (
          <div
            key={s}
            style={{
              position: "absolute",
              left: x,
              top: y,
              opacity: Math.min(1, appear * 1.5) * (1 - ramp(frame, flyStart + 12, flyStart + 18)),
              transform: `scale(${interpolate(appear, [0, 1], [0.6, 1]) * (1 - fly * 0.5)})`,
              transformOrigin: "left center",
            }}
          >
            <div
              style={{
                width: 290,
                padding: "22px 26px",
                borderRadius: 18,
                background: C.panel,
                border: `1.5px solid ${C.line}`,
                fontFamily: SANS,
                fontWeight: 600,
                fontSize: 30,
                color: C.cream,
                display: "flex",
                alignItems: "center",
                gap: 16,
              }}
            >
              <div style={{ width: 14, height: 14, borderRadius: 7, background: [C.gold, C.blue, C.coral, C.violet, C.green][i] }} />
              {s}
            </div>
          </div>
        );
      })}
      <div
        style={{
          position: "absolute",
          left: 1330,
          top: 260,
          width: 460,
          padding: "40px 40px 34px",
          borderRadius: 28,
          background: C.panel2,
          border: `2px solid ${C.gold}${boxGlow > 0.05 ? "" : "55"}`,
          boxShadow: `0 0 ${boxGlow * 90}px rgba(242,181,68,${boxGlow * 0.5}), 0 40px 80px rgba(0,0,0,0.35)`,
          opacity: box,
          transform: `translateY(${(1 - box) * 40}px) scale(${1 + boxGlow * 0.03})`,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
          <Library size={34} color={C.gold} />
          <span style={{ fontFamily: SANS, fontWeight: 600, fontSize: 24, color: C.muted, letterSpacing: "0.12em" }}>YOUR LIBRARY</span>
        </div>
        <div style={{ fontFamily: SERIF, fontWeight: 600, fontSize: 120, color: C.cream, marginTop: 14, fontVariantNumeric: "tabular-nums" }}>
          {count.toLocaleString("en-US")}
        </div>
        <div style={{ fontFamily: SANS, fontSize: 30, color: C.muted, marginTop: -6 }}>songs imported</div>
        <div style={{ marginTop: 26, display: "flex", flexDirection: "column", gap: 12 }}>
          {["Sets and service orders", "Themes", "Media and backgrounds"].map((t, i) => {
            const p = spring({ frame: frame - e.checks[i], fps: 30, config: { damping: 12, stiffness: 150 } });
            return (
              <div key={t} style={{ display: "flex", alignItems: "center", gap: 14, fontFamily: SANS, fontSize: 27, color: C.cream, ...popStyle(p), transformOrigin: "left center" }}>
                <Check size={28} color={C.green} strokeWidth={3} /> {t}
              </div>
            );
          })}
        </div>
      </div>
    </Shell>
  );
};

// ---------- scene: recap ----------

const TILES = [
  { icon: Users, title: "Plan together", sub: "Live with the team, offline on Sunday", color: C.gold },
  { icon: MonitorCheck, title: "Fits every screen", sub: "Calibrated per screen in two minutes", color: C.blue },
  { icon: Presentation, title: "Slides that just play", sub: "PowerPoint and Google Slides, intact", color: C.coral },
  { icon: Library, title: "Switch in an hour", sub: "Your whole library in one pass", color: C.green },
];

const Recap: React.FC = () => {
  const frame = useCurrentFrame();
  const e = T.events.recap;
  return (
    <Shell dur={T.scenes.recap.dur}>
      <AbsoluteFill style={{ alignItems: "center", paddingTop: 140 }}>
        <Words
          text="Sunday, sorted."
          delay={4}
          step={4}
          style={{ fontFamily: SERIF, fontWeight: 600, fontSize: 92, color: C.cream }}
        />
        <div style={{ display: "grid", gridTemplateColumns: "640px 640px", gap: 32, marginTop: 70 }}>
          {TILES.map((t, i) => {
            const p = spring({ frame: frame - e.tiles[i], fps: 30, config: { damping: 13, stiffness: 140 } });
            const Icon = t.icon;
            return (
              <div
                key={t.title}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 28,
                  padding: "34px 36px",
                  borderRadius: 24,
                  background: C.panel,
                  border: `1.5px solid ${C.line}`,
                  ...popStyle(p),
                }}
              >
                <div style={{ width: 86, height: 86, borderRadius: 22, background: `${t.color}22`, display: "flex", alignItems: "center", justifyContent: "center", flexShrink: 0 }}>
                  <Icon size={46} color={t.color} />
                </div>
                <div>
                  <div style={{ fontFamily: SANS, fontWeight: 600, fontSize: 36, color: C.cream }}>{t.title}</div>
                  <div style={{ fontFamily: SANS, fontSize: 25, color: C.muted, marginTop: 6 }}>{t.sub}</div>
                </div>
              </div>
            );
          })}
        </div>
      </AbsoluteFill>
    </Shell>
  );
};

// ---------- scene: end card ----------

const LampMark: React.FC<{ size: number; glow: number }> = ({ size, glow }) => {
  const frame = useCurrentFrame();
  const flicker = 1 + Math.sin(frame / 4) * 0.03 + Math.sin(frame / 7) * 0.02;
  return (
    <svg width={size} height={size} viewBox="0 0 100 100">
      <defs>
        <radialGradient id="g" cx="50%" cy="32%" r="45%">
          <stop offset="0%" stopColor={C.gold} stopOpacity={0.55 * glow} />
          <stop offset="100%" stopColor={C.gold} stopOpacity={0} />
        </radialGradient>
      </defs>
      <circle cx={50} cy={32} r={44} fill="url(#g)" />
      <g transform={`translate(50 34) scale(${flicker}) translate(-50 -34)`}>
        <path d="M50 12 C58 24 60 30 56 38 C54 42 46 42 44 38 C40 30 42 24 50 12 Z" fill={C.gold} />
        <path d="M50 24 C53 30 54 33 52 37 C51 39 49 39 48 37 C46 33 47 30 50 24 Z" fill={C.cream} />
      </g>
      <path d="M36 46 H64 L60 54 H40 Z" fill={C.cream} />
      <rect x={47} y={54} width={6} height={26} fill={C.cream} />
      <path d="M32 88 C36 80 64 80 68 88 Z" fill={C.cream} />
    </svg>
  );
};

const End: React.FC = () => {
  const frame = useCurrentFrame();
  const e = T.events.end;
  const mark = useSpr(e.mark, 12, 120);
  const word = useSpr(e.word, 200);
  const tag = ramp(frame, e.tag, e.tag + 16);
  const l2 = ramp(frame, e.line2, e.line2 + 16);
  const out = interpolate(frame, [T.scenes.end.dur - 20, T.scenes.end.dur], [1, 0], CLAMP);
  return (
    <AbsoluteFill style={{ justifyContent: "center", alignItems: "center", opacity: Math.min(ramp(frame, 0, 10), out) }}>
      <div style={popStyle(mark)}>
        <LampMark size={210} glow={mark} />
      </div>
      <div style={{ fontFamily: SERIF, fontWeight: 600, fontSize: 132, color: C.cream, letterSpacing: "-0.01em", marginTop: 6, opacity: word, transform: `translateY(${(1 - word) * 24}px)` }}>
        Lampstand
      </div>
      <div style={{ fontFamily: SANS, fontWeight: 500, fontSize: 36, color: C.gold, marginTop: 18, opacity: tag }}>Free and open source church projection.</div>
      <div style={{ fontFamily: SANS, fontSize: 30, color: C.muted, marginTop: 14, opacity: l2 }}>Windows · macOS · Linux · Runs the whole service offline</div>
    </AbsoluteFill>
  );
};

// ---------- composition ----------

export const Promo: React.FC<{ withAudio: boolean }> = ({ withAudio }) => {
  const S = T.scenes;
  return (
    <AbsoluteFill style={{ background: C.bg }}>
      <Backdrop />
      <Sequence from={S.open.from} durationInFrames={S.open.dur}><Open /></Sequence>
      <Sequence from={S.plan.from} durationInFrames={S.plan.dur}><Plan /></Sequence>
      <Sequence from={S.screens.from} durationInFrames={S.screens.dur}><Screens /></Sequence>
      <Sequence from={S.slides.from} durationInFrames={S.slides.dur}><Slides /></Sequence>
      <Sequence from={S.migrate.from} durationInFrames={S.migrate.dur}><Migrate /></Sequence>
      <Sequence from={S.recap.from} durationInFrames={S.recap.dur}><Recap /></Sequence>
      <Sequence from={S.end.from} durationInFrames={S.end.dur}><End /></Sequence>
      {withAudio ? <Audio src={staticFile("sfx.wav")} /> : null}
    </AbsoluteFill>
  );
};

export const TOTAL_FRAMES = T.scenes.end.from + T.scenes.end.dur;
