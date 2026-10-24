import { z } from "zod";

export const TONES = ["paper", "table", "screen"] as const;

export const base = {
  durationSec: z.number().positive().optional(),
  enter: z.enum(["rise", "fade", "wipe", "pop"]).default("rise"),
  exit: z.enum(["fade", "drop", "none"]).default("fade"),
  tone: z.enum(TONES).optional(),
};

export const Title = z.object({
  type: z.literal("title"),
  kicker: z.string().optional(),
  heading: z.string(),
  sub: z.string().optional(),
  ...base,
});
export const Bullets = z.object({
  type: z.literal("bullets"),
  heading: z.string().optional(),
  items: z.array(z.string()).min(1).max(5),
  ...base,
});
export const Stat = z.object({
  type: z.literal("stat"),
  value: z.number(),
  prefix: z.string().optional(),
  suffix: z.string().optional(),
  label: z.string(),
  ...base,
});
export const Cta = z.object({
  type: z.literal("cta"),
  heading: z.string(),
  action: z.string(),
  url: z.string().optional(),
  ...base,
});
export const Custom = z.object({
  type: z.literal("custom"),
  component: z.string(),
  props: z.record(z.unknown()).default({}),
  ...base,
});

export const coreScenes = [Title, Bullets, Stat, Cta] as const;

export function makeVideoProps<S extends z.ZodTypeAny>(scene: S) {
  return z.object({
    aspect: z.enum(["16:9", "9:16", "1:1"]).default("16:9"),
    fps: z.number().default(30),
    scenes: z.array(scene).min(1),
  });
}
