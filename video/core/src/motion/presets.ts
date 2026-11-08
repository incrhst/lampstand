import { spring, useCurrentFrame, useVideoConfig } from "remotion";
import { useTheme } from "../theme";

export type EnterPreset = "rise" | "fade" | "wipe" | "pop";
export type ExitPreset = "fade" | "drop" | "none";
export const EXIT_FRAMES = 12;

export function useEnter(preset: EnterPreset, delay = 0): React.CSSProperties {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const theme = useTheme();
  const p = spring({ frame: frame - delay, fps, config: theme.motion.spring });
  switch (preset) {
    case "rise": return { opacity: Math.min(1, p * 1.4), transform: `translateY(${(1 - p) * 40}px)` };
    case "fade": return { opacity: p };
    case "pop": return { opacity: Math.min(1, p * 1.4), transform: `scale(${0.85 + 0.15 * p})` };
    case "wipe": return { clipPath: `inset(0 ${Math.max(0, (1 - p) * 100)}% 0 0)` };
  }
}

/** Progress 0..1 of the entrance, for count-ups and drawn lines. */
export function useProgress(delay = 0): number {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const theme = useTheme();
  return Math.max(0, spring({ frame: frame - delay, fps, config: theme.motion.spring }));
}

export function exitStyle(preset: ExitPreset, frame: number, total: number): React.CSSProperties {
  if (preset === "none") return {};
  const k = Math.min(1, Math.max(0, (frame - (total - EXIT_FRAMES)) / EXIT_FRAMES));
  return preset === "fade" ? { opacity: 1 - k } : { opacity: 1 - k, transform: `translateY(${k * 50}px)` };
}
