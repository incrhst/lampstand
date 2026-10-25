// Turns a tour scene list into one timeline: absolute camera stops, caption windows and sound cues.
// Pure data, so the composition, the contact sheet and the validator all agree on timing.
// Everything is counted in whole frames first, then divided by fps, so times match the hand-written film exactly.
import { Easing } from "remotion";
import { sceneFrames, words, WPS } from "@video/core";
import { motion } from "../tokens";
import type { TourPropsT, TourSceneT } from "./schema";

export type Cam = { t: number; x: number; y: number; d: number; yaw: number; pitch: number; roll: number };
export type Caption = { from: number; to: number; kind: TourSceneT["type"]; tag: string; title: string; sub?: string; keys?: string[] };
export type Cue = { at: number; sound: "whoosh" | "stamp" | "key" | "bell"; volume: number };

/** Default scene length: read the caption, press the keys, and give each later camera stop its own beat. */
export function tourDefaultSec(s: TourSceneT): number {
  const read = 1.6 + words(`${s.tag} ${s.title} ${s.sub ?? ""}`) / WPS + (s.keys?.length ?? 0) * motion.keys.everySec;
  const last = s.stops[s.stops.length - 1].atSec;
  return Math.max(4, read, last + 3);
}

export const sceneFramesOf = (s: TourSceneT, fps: number) => sceneFrames(s, fps, tourDefaultSec as (s: any) => number);

export function compile(p: TourPropsT) {
  const { fps } = p;
  const f = (sec: number) => Math.round(sec * fps);
  const where = (id?: string) => {
    if (!id) return { x: 0, y: 0 };
    if (id === "screen") return { x: p.world.screen!.x, y: p.world.screen!.y };
    const w = p.world.windows.find((w) => w.id === id)!;
    return { x: w.x, y: w.y };
  };

  const cams: Cam[] = [];
  const captions: Caption[] = [];
  const extra: Cue[] = [];
  const starts: number[] = [];
  let start = 0;
  let screenFrom: number | undefined;
  for (const s of p.scenes) {
    const n = sceneFramesOf(s, fps);
    starts.push(start);
    for (const st of s.stops) {
      const o = where(st.on);
      cams.push({ t: (start + f(st.atSec)) / fps, x: o.x + st.x, y: o.y + st.y, d: st.d, yaw: st.yaw, pitch: st.pitch, roll: st.roll });
      if (st.on === "screen" && screenFrom === undefined) screenFrom = start + f(st.atSec);
    }
    captions.push({
      from: (start + f(s.captionIn ?? motion.caption.leadSec)) / fps,
      to: (start + n - f(s.captionOut ?? motion.caption.tailSec)) / fps,
      kind: s.type, tag: s.tag, title: s.title, sub: s.sub, keys: s.keys,
    });
    for (const x of s.sfx) extra.push({ at: (start + f(x.atSec)) / fps, sound: x.sound, volume: x.volume });
    start += n;
  }
  const total = start;
  // The camera needs somewhere to go: a single stop is held for the whole film.
  if (cams.length === 1) cams.push({ ...cams[0], t: total / fps });

  const { rest: REST, restShare } = motion.camera;
  const restOf = (i: number) => (i === 0 ? 0 : Math.min(REST, (cams[i + 1].t - cams[i].t) * restShare));
  const v = p.sfxVolume;
  const cues: Cue[] = [
    // A whoosh in the middle of each glide, a stamp as each caption lands, a click for each key press.
    ...cams.slice(0, -1).map((a, i): Cue => {
      const b = cams[i + 1];
      const rest = restOf(i);
      const mid = a.t + rest + (b.t - a.t - rest) / 2;
      return { at: Math.max(0, mid - 0.9), sound: "whoosh", volume: v.whoosh };
    }),
    ...captions.map((c): Cue => ({ at: c.from, sound: "stamp", volume: v.stamp })),
    ...captions.flatMap((c) => (c.keys ?? []).map((_, n): Cue => ({ at: c.from + motion.keys.firstSec + n * motion.keys.everySec, sound: "key", volume: v.key }))),
    ...extra,
  ];

  const ease = Easing.inOut(Easing.cubic);
  const { drift } = motion.camera;
  /** Rest at each stop for a moment, then glide to the next, with a slow float so a resting camera is never dead still. */
  function camAt(t: number): Cam {
    let i = 0;
    while (i < cams.length - 2 && t >= cams[i + 1].t) i++;
    const a = cams[i];
    const b = cams[i + 1];
    const rest = Math.min(REST, (b.t - a.t) * restShare);
    const r = i === 0 ? 0 : rest;
    const k = ease(Math.min(1, Math.max(0, (t - a.t - r) / (b.t - a.t - r))));
    const m = (key: keyof Cam) => (a[key] as number) + ((b[key] as number) - (a[key] as number)) * k;
    const d = Math.sin(t * drift.xRate);
    return { t, x: m("x") + d * drift.x, y: m("y") + Math.cos(t * drift.yRate) * drift.y, d: m("d"), yaw: m("yaw") + d * drift.yaw, pitch: m("pitch"), roll: m("roll") };
  }

  return { fps, total, starts, cams, captions, cues, camAt, screenFromSec: screenFrom === undefined ? undefined : screenFrom / fps };
}
