import React from "react";
import { AbsoluteFill, Audio, Easing, Img, Sequence, interpolate, staticFile, useCurrentFrame, useVideoConfig } from "remotion";
import { brand } from "./brand";
import { Cam, cams, captions, cues, lyrics, OUTPUT, PERSPECTIVE, planes, Plane } from "./timeline";

const fonts = `
@font-face{font-family:"Old Standard TT";src:url(${staticFile("fonts/old-standard-tt-400-normal.woff2")}) format("woff2");font-weight:400}
@font-face{font-family:"Old Standard TT";src:url(${staticFile("fonts/old-standard-tt-400-italic.woff2")}) format("woff2");font-weight:400;font-style:italic}
@font-face{font-family:"Old Standard TT";src:url(${staticFile("fonts/old-standard-tt-700-normal.woff2")}) format("woff2");font-weight:700}
@font-face{font-family:"Courier Prime";src:url(${staticFile("fonts/courier-prime-400-normal.woff2")}) format("woff2");font-weight:400}
@font-face{font-family:"Courier Prime";src:url(${staticFile("fonts/courier-prime-700-normal.woff2")}) format("woff2");font-weight:700}
`;

const ease = Easing.inOut(Easing.cubic);

function camAt(t: number): Cam {
  let i = 0;
  while (i < cams.length - 2 && t >= cams[i + 1].t) i++;
  const a = cams[i];
  const b = cams[i + 1];
  // Rest at each beat for a moment, then glide to the next.
  const rest = Math.min(1.6, (b.t - a.t) * 0.3);
  const k = ease(Math.min(1, Math.max(0, (t - a.t - (i === 0 ? 0 : rest)) / (b.t - a.t - (i === 0 ? 0 : rest)))));
  const m = (f: keyof Cam) => (a[f] as number) + ((b[f] as number) - (a[f] as number)) * k;
  // A slow float so a resting camera is never dead still.
  const drift = Math.sin(t * 0.9);
  return { t, x: m("x") + drift * 22, y: m("y") + Math.cos(t * 0.7) * 12, d: m("d"), yaw: m("yaw") + drift * 0.6, pitch: m("pitch"), roll: m("roll") };
}

const Paper: React.FC = () => (
  <div
    style={{
      position: "absolute",
      left: -3000,
      top: -4600,
      width: 17000,
      height: 10600,
      transform: "translateZ(-420px)",
      background: `radial-gradient(rgba(76,42,133,0.10) 2px, transparent 2.5px) 0 0/48px 48px, ${brand.paper}`,
    }}
  >
    {[0, 1, 2, 3, 4, 5, 6].map((n) => (
      <div key={n} style={{ position: "absolute", left: 1700 * n + 500, top: 0, width: 56, height: "100%", background: "linear-gradient(90deg, transparent, rgba(150,100,10,0.18) 48%, rgba(255,240,200,0.25) 52%, transparent)" }} />
    ))}
    {[
      ["PLAN", 4500],
      ["PRESENT", 8000],
      ["THE SCREEN", 9850],
      ["OBS", 11700],
      ["SETUP", 13700],
    ].map(([label, x]) => (
      <div key={label as string} style={{ position: "absolute", left: x as number - 800, top: 5300, fontFamily: brand.typed, fontWeight: 700, fontSize: 340, color: brand.crease, opacity: 0.55, letterSpacing: "0.12em", whiteSpace: "nowrap" }}>
        {label}
      </div>
    ))}
  </div>
);

const WindowPlane: React.FC<{ p: Plane }> = ({ p }) => {
  const bar = 34;
  return (
    <div
      style={{
        position: "absolute",
        left: -p.w / 2,
        top: -(p.h + bar) / 2,
        width: p.w,
        height: p.h + bar,
        transform: `translate3d(${p.x}px, ${p.y}px, ${p.z}px) rotateY(${p.yaw}deg) scale(${p.scale})`,
        borderRadius: 12,
        overflow: "hidden",
        background: "#15171d",
        boxShadow: "0 40px 90px rgba(29,18,0,0.55), 0 6px 14px rgba(29,18,0,0.4)",
        backfaceVisibility: "hidden",
      }}
    >
      <div style={{ height: bar, background: "#2a2d36", display: "flex", alignItems: "center", gap: 9, padding: "0 14px", color: "#9aa0ad", fontFamily: brand.typed, fontSize: 15 }}>
        {["#ff5f57", "#febc2e", "#28c840"].map((c) => (
          <span key={c} style={{ width: 12, height: 12, borderRadius: 6, background: c }} />
        ))}
        <span style={{ marginLeft: 14 }}>{p.title}</span>
      </div>
      <Img src={staticFile(`shots/${p.shot}`)} style={{ width: p.w, height: p.h, display: "block" }} />
    </div>
  );
};

const OutputPlane: React.FC<{ frame: number }> = ({ frame }) => {
  const { fps } = useVideoConfig();
  const t = frame / fps;
  // Verses change while the camera rests here.
  const idx = Math.min(lyrics.length - 1, Math.max(0, Math.floor((t - 41) / 2.2)));
  return (
    <div
      style={{
        position: "absolute",
        left: -OUTPUT.w / 2,
        top: -OUTPUT.h / 2,
        width: OUTPUT.w,
        height: OUTPUT.h,
        transform: `translate3d(${OUTPUT.x}px, ${OUTPUT.y}px, ${OUTPUT.z}px) rotateY(${OUTPUT.yaw}deg)`,
        background: brand.screen,
        borderRadius: 6,
        border: "14px solid #101114",
        boxShadow: "0 50px 100px rgba(29,18,0,0.6), 0 0 120px rgba(233,185,73,0.25)",
        backfaceVisibility: "hidden",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      {lyrics.map((l, n) => {
        const op = n === idx ? 1 : 0;
        return (
          <div key={n} style={{ position: "absolute", textAlign: "center", fontFamily: brand.serif, fontSize: 82, lineHeight: 1.25, color: brand.screenInk, opacity: op, transition: "none" }}>
            {l.map((line) => (
              <div key={line}>{line}</div>
            ))}
          </div>
        );
      })}
      <div style={{ position: "absolute", left: 24, bottom: 18, fontFamily: brand.typed, fontSize: 20, color: brand.screenGold, opacity: 0.8 }}>Amazing Grace · John Newton</div>
    </div>
  );
};

const CaptionCard: React.FC<{ c: (typeof captions)[number]; t: number }> = ({ c, t }) => {
  const inK = Easing.out(Easing.cubic)(Math.min(1, Math.max(0, (t - c.from) / 0.6)));
  const outK = Math.min(1, Math.max(0, (c.to - t) / 0.4));
  if (t < c.from || t > c.to) return null;
  const big = c.from < 4 || c.from > 64;
  return (
    <div
      style={{
        position: "absolute",
        ...(c.from > 64 ? { left: "50%", top: "50%", marginLeft: -520, marginTop: -170 } : { left: 80, bottom: 80 }),
        maxWidth: big ? 1100 : 900,
        padding: "26px 34px 28px",
        background: brand.paper,
        color: brand.ink,
        border: `3px solid ${brand.ink}`,
        boxShadow: `10px 10px 0 ${brand.stamp}`,
        opacity: inK * outK,
        transform: `translateY(${(1 - inK) * 30}px) rotate(-0.6deg)`,
      }}
    >
      <div style={{ fontFamily: brand.typed, fontWeight: 700, fontSize: 24, letterSpacing: "0.14em", textTransform: "uppercase", color: brand.stamp, marginBottom: 10 }}>{c.tag}</div>
      <div style={{ fontFamily: brand.serif, fontWeight: 700, fontSize: big ? 66 : 54, lineHeight: 1.08 }}>{c.title}</div>
      {c.sub && <div style={{ fontFamily: brand.typed, fontSize: 28, lineHeight: 1.35, marginTop: 14, color: brand.inkSoft }}>{c.sub}</div>}
      {c.keys && (
        <div style={{ display: "flex", gap: 10, marginTop: 18 }}>
          {c.keys.map((k, n) => {
            const press = t > c.from + 1.2 + n * 0.9 && t < c.from + 1.7 + n * 0.9;
            return (
              <span key={k} style={{ fontFamily: brand.typed, fontWeight: 700, fontSize: 30, minWidth: 54, textAlign: "center", padding: "6px 16px", background: press ? brand.stamp : brand.ink, color: brand.paper, borderRadius: 8, boxShadow: press ? "0 2px 0 #000" : "0 6px 0 #000", transform: press ? "translateY(4px)" : "none" }}>
                {k}
              </span>
            );
          })}
        </div>
      )}
    </div>
  );
};

export const Demo: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps, durationInFrames } = useVideoConfig();
  const t = frame / fps;
  const c = camAt(t);
  const fadeIn = interpolate(frame, [0, 18], [0, 1], { extrapolateRight: "clamp" });
  const fadeOut = interpolate(frame, [durationInFrames - 24, durationInFrames - 1], [1, 0], { extrapolateLeft: "clamp" });
  return (
    <AbsoluteFill style={{ background: brand.table, opacity: fadeIn * fadeOut, overflow: "hidden" }}>
      <style>{fonts}</style>
      {/* Background music (public/music): sits under the effects, fades in and out. */}
      <Audio
        src={staticFile("music/lampstand-intro.m4a")}
        volume={(f) => 0.38 * Math.min(1, f / (1.5 * fps)) * Math.min(1, (durationInFrames - f) / (4 * fps))}
      />
      {cues.map((c, i) => (
        <Sequence key={i} from={Math.round(c.at * fps)} layout="none">
          <Audio src={staticFile(`sfx/${c.sound}.wav`)} volume={c.volume} />
        </Sequence>
      ))}
      <AbsoluteFill style={{ perspective: PERSPECTIVE, perspectiveOrigin: "50% 50%" }}>
        <div
          style={{
            position: "absolute",
            left: "50%",
            top: "50%",
            transformStyle: "preserve-3d",
            transform: `translateY(-70px) translateZ(${PERSPECTIVE - c.d}px) rotateX(${-c.pitch}deg) rotateY(${c.yaw}deg) rotateZ(${c.roll}deg) translate3d(${-c.x}px, ${-c.y}px, 0px)`,
          }}
        >
          <Paper />
          {planes.map((p) => (
            <WindowPlane key={p.id} p={p} />
          ))}
          <OutputPlane frame={frame} />
        </div>
      </AbsoluteFill>
      <AbsoluteFill style={{ background: "radial-gradient(ellipse at 50% 50%, transparent 55%, rgba(29,18,0,0.45) 100%)", pointerEvents: "none" }} />
      {captions.map((cp) => (
        <CaptionCard key={cp.from} c={cp} t={t} />
      ))}
    </AbsoluteFill>
  );
};
