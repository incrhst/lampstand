import React, { useMemo } from "react";
import { AbsoluteFill, Audio, interpolate, Sequence, staticFile, useCurrentFrame, useVideoConfig } from "remotion";
import { makeCalculateMetadata } from "@video/core";
import { fontFaces } from "../theme";
import { motion, tour } from "../tokens";
import { TourProps, type TourPropsT } from "./schema";
import { compile, tourDefaultSec } from "./timeline";
import { Backdrop, CaptionCard, ScreenPlane, WindowPlane } from "./parts";

/** The site's fly-over: one camera gliding across the world in the scene list, captions on top, music and cues under it. */
export const Tour: React.FC<TourPropsT> = (raw) => {
  const p = useMemo(() => TourProps.parse(raw), [raw]);
  const tl = useMemo(() => compile(p), [p]);
  const frame = useCurrentFrame();
  const { fps, durationInFrames } = useVideoConfig();
  const t = frame / fps;
  const c = tl.camAt(t);
  const fadeIn = interpolate(frame, [0, motion.fadeInFrames], [0, 1], { extrapolateRight: "clamp" });
  const fadeOut = interpolate(frame, [durationInFrames - motion.fadeOutFrames, durationInFrames - 1], [1, 0], { extrapolateLeft: "clamp" });
  const P = tour.perspective;
  const m = p.music;
  return (
    <AbsoluteFill style={{ background: tour.table, opacity: fadeIn * fadeOut, overflow: "hidden" }}>
      <style>{fontFaces}</style>
      {m && <Audio src={staticFile(m.src)} volume={(f) => m.volume * Math.min(1, f / (m.fadeInSec * fps)) * Math.min(1, (durationInFrames - f) / (m.fadeOutSec * fps))} />}
      {tl.cues.map((q, i) => (
        <Sequence key={i} from={Math.round(q.at * fps)} layout="none">
          <Audio src={staticFile(`sfx/${q.sound}.wav`)} volume={q.volume} />
        </Sequence>
      ))}
      {p.scenes.map((s, i) => s.narration && (
        <Sequence key={`n${i}`} from={tl.starts[i] + 9} layout="none"><Audio src={staticFile(s.narration)} /></Sequence>
      ))}
      <AbsoluteFill style={{ perspective: P, perspectiveOrigin: "50% 50%" }}>
        <div style={{ position: "absolute", left: "50%", top: "50%", transformStyle: "preserve-3d", transform: `translateY(-70px) translateZ(${P - c.d}px) rotateX(${-c.pitch}deg) rotateY(${c.yaw}deg) rotateZ(${c.roll}deg) translate3d(${-c.x}px, ${-c.y}px, 0px)` }}>
          {p.world.backdrop && <Backdrop b={p.world.backdrop} />}
          {p.world.windows.map((w) => <WindowPlane key={w.id} p={w} />)}
          {p.world.screen && <ScreenPlane s={p.world.screen} t={t} fromSec={tl.screenFromSec} />}
        </div>
      </AbsoluteFill>
      <AbsoluteFill style={{ background: tour.vignette, pointerEvents: "none" }} />
      {tl.captions.map((cp, i) => <CaptionCard key={i} c={cp} t={t} />)}
    </AbsoluteFill>
  );
};

export const tourCalculateMetadata = makeCalculateMetadata(tourDefaultSec as (s: any) => number, TourProps);
