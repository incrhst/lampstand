// Reusable pieces of the fly-over: the paper backdrop, a window plane, the projector plane, the caption card and key caps.
// Every colour, size, space and timing comes from tokens.ts; every position and word comes from the scene list.
import React from "react";
import { Easing, Img, staticFile, useVideoConfig } from "remotion";
import { fitSize } from "@video/core";
import { motion, site, space, tour, type } from "../tokens";
import type { TourPropsT } from "./schema";
import type { Caption } from "./timeline";

type World = TourPropsT["world"];

export const Backdrop: React.FC<{ b: NonNullable<World["backdrop"]> }> = ({ b }) => (
  <div style={{ position: "absolute", left: b.x, top: b.y, width: b.w, height: b.h, transform: `translateZ(${b.z}px)`, background: `radial-gradient(${tour.dots} 2px, transparent 2.5px) 0 0/${space.dotGrid}px ${space.dotGrid}px, ${site.paper}` }}>
    {Array.from({ length: b.folds.count }, (_, n) => (
      <div key={n} style={{ position: "absolute", left: b.folds.every * n + b.folds.from, top: 0, width: 56, height: "100%", background: tour.fold }} />
    ))}
    {b.labels.map((l) => (
      <div key={l.text} style={{ position: "absolute", left: l.x, top: l.y, fontFamily: type.typed, fontWeight: 700, fontSize: 340, color: site.crease, opacity: 0.55, letterSpacing: type.tracking.backdrop, whiteSpace: "nowrap" }}>{l.text}</div>
    ))}
  </div>
);

export const WindowPlane: React.FC<{ p: World["windows"][number] }> = ({ p }) => {
  const bar = tour.chromeH;
  return (
    <div style={{ position: "absolute", left: -p.w / 2, top: -(p.h + bar) / 2, width: p.w, height: p.h + bar, transform: `translate3d(${p.x}px, ${p.y}px, ${p.z}px) rotateY(${p.yaw}deg) scale(${p.scale})`, borderRadius: tour.windowRadius, overflow: "hidden", background: tour.windowBody, boxShadow: tour.shadow, backfaceVisibility: "hidden" }}>
      <div style={{ height: bar, background: tour.chrome, display: "flex", alignItems: "center", gap: 9, padding: "0 14px", color: tour.chromeInk, fontFamily: type.typed, fontSize: type.size.chrome }}>
        {tour.lights.map((c) => <span key={c} style={{ width: 12, height: 12, borderRadius: 6, background: c }} />)}
        <span style={{ marginLeft: 14 }}>{p.title}</span>
      </div>
      <Img src={staticFile(p.src)} style={{ width: p.w, height: p.h, display: "block" }} />
    </div>
  );
};

/** The congregation's screen: verses change while the camera rests on it. */
export const ScreenPlane: React.FC<{ s: NonNullable<World["screen"]>; t: number; fromSec?: number }> = ({ s, t, fromSec }) => {
  const start = (fromSec ?? 0) + s.delaySec;
  const idx = Math.min(s.lyrics.length - 1, Math.max(0, Math.floor((t - start) / s.everySec)));
  return (
    <div style={{ position: "absolute", left: -s.w / 2, top: -s.h / 2, width: s.w, height: s.h, transform: `translate3d(${s.x}px, ${s.y}px, ${s.z}px) rotateY(${s.yaw}deg)`, background: site.screen, borderRadius: space.radius, border: `${tour.bezelW}px solid ${tour.bezel}`, boxShadow: tour.screenShadow, backfaceVisibility: "hidden", display: "flex", alignItems: "center", justifyContent: "center" }}>
      {s.lyrics.map((l, n) => (
        <div key={n} style={{ position: "absolute", textAlign: "center", fontFamily: type.serif, fontSize: type.size.lyric, lineHeight: type.leading.lyric, color: site.screenInk, opacity: n === idx ? 1 : 0 }}>
          {l.map((line) => <div key={line}>{line}</div>)}
        </div>
      ))}
      {s.credit && <div style={{ position: "absolute", left: 24, bottom: 18, fontFamily: type.typed, fontSize: type.size.credit, color: site.screenGold, opacity: 0.8 }}>{s.credit}</div>}
    </div>
  );
};

/** Key caps, each pressed in turn after the caption lands. */
const Keys: React.FC<{ keys: string[]; t: number; from: number }> = ({ keys, t, from }) => (
  <div style={{ display: "flex", gap: space.key.gap, marginTop: space.card.keysGap }}>
    {keys.map((k, n) => {
      const at = from + motion.keys.firstSec + n * motion.keys.everySec;
      const press = t > at && t < at + motion.keys.holdSec;
      return (
        <span key={k} style={{ fontFamily: type.typed, fontWeight: 700, fontSize: type.size.key, minWidth: space.key.minW, textAlign: "center", padding: space.key.padding, background: press ? site.stamp : site.ink, color: site.paper, borderRadius: space.key.radius, boxShadow: press ? tour.keyShadowPressed : tour.keyShadow, transform: press ? `translateY(${space.key.travel}px)` : "none" }}>{k}</span>
      );
    })}
  </div>
);

const PAD_X = 68; // the card's left + right padding (space.card.padding)

/** A pasted paper label: typed tag, serif title, typed sub, optional key caps. Rises in, fades out. */
export const CaptionCard: React.FC<{ c: Caption; t: number }> = ({ c, t }) => {
  const { width } = useVideoConfig();
  if (t < c.from || t > c.to) return null;
  const inK = Easing.out(Easing.cubic)(Math.min(1, Math.max(0, (t - c.from) / motion.caption.inSec)));
  const outK = Math.min(1, Math.max(0, (c.to - t) / motion.caption.outSec));
  const big = c.kind !== "stop";
  const end = c.kind === "close";
  // The end card sits a little left of and above centre, as in the first cut; it never leaves the frame.
  const endLeft = Math.max(space.edge / 2, width / 2 - 520);
  const maxW = end ? Math.min(space.card.maxWBig, width - endLeft - space.edge / 2) : Math.min(big ? space.card.maxWBig : space.card.maxW, width - space.edge * 2);
  const inner = maxW - PAD_X;
  const titleMax = big ? type.size.titleBig : type.size.title;
  // Text fits itself: full size unless it would run past three lines.
  const titleSize = fitSize(c.title, inner, titleMax * type.leading.title * 3, titleMax, 32, 0.54, type.leading.title);
  const subSize = c.sub ? fitSize(c.sub, inner, type.size.sub * type.leading.sub * 3, type.size.sub, 18, 0.6, type.leading.sub) : 0;
  return (
    <div style={{ position: "absolute", ...(end ? { left: endLeft, top: "50%", marginTop: -170 } : { left: space.edge, bottom: space.edge }), maxWidth: maxW, padding: space.card.padding, background: site.paper, color: site.ink, border: `${space.card.border}px solid ${site.ink}`, boxShadow: `${space.card.offset}px ${space.card.offset}px 0 ${site.stamp}`, opacity: inK * outK, transform: `translateY(${(1 - inK) * motion.caption.rise}px) rotate(${motion.caption.tilt}deg)` }}>
      <div style={{ fontFamily: type.typed, fontWeight: 700, fontSize: type.size.tag, letterSpacing: type.tracking.tag, textTransform: "uppercase", color: site.stamp, marginBottom: space.card.tagGap }}>{c.tag}</div>
      <div style={{ fontFamily: type.serif, fontWeight: 700, fontSize: titleSize, lineHeight: type.leading.title }}>{c.title}</div>
      {c.sub && <div style={{ fontFamily: type.typed, fontSize: subSize, lineHeight: type.leading.sub, marginTop: space.card.subGap, color: site.inkSoft }}>{c.sub}</div>}
      {c.keys && <Keys keys={c.keys} t={t} from={c.from} />}
    </div>
  );
};
