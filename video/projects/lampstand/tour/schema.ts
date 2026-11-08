import { z } from "zod";

// The fly-over ("tour") is one continuous camera move over a world of window planes, so it is not a <Series> of
// separate scenes. Its JSON still is a scene list: each scene is one caption and the camera stops it holds.
// The world (where each window sits) is data too. Times are seconds; positions are world pixels.

const Window = z.object({
  id: z.string(),
  src: z.string().startsWith("shots/"), // a screenshot in video/public/shots
  title: z.string(), // the window's title bar
  x: z.number(),
  y: z.number(),
  z: z.number().default(0),
  yaw: z.number().default(0), // degrees about the vertical axis
  scale: z.number().positive().default(1),
  w: z.number().positive().default(1280), // the screenshot's CSS size
  h: z.number().positive().default(800),
});

// The projector the congregation sees. Not a screenshot: the real output is drawn natively (wgpu), so the tour
// draws lyrics on the screen colours instead.
const Screen = z.object({
  id: z.literal("screen").default("screen"),
  x: z.number(),
  y: z.number(),
  z: z.number().default(0),
  yaw: z.number().default(0),
  w: z.number().positive().default(1600),
  h: z.number().positive().default(900),
  lyrics: z.array(z.array(z.string()).min(1).max(3)).min(1).max(6),
  credit: z.string().optional(),
  everySec: z.number().positive().default(2.2), // a new verse this often while the camera is there
  delaySec: z.number().min(0).default(0.6), // after the first scene that stops on the screen starts
});

const Backdrop = z.object({
  x: z.number(), y: z.number(), w: z.number(), h: z.number(), z: z.number().default(-420),
  folds: z.object({ count: z.number().int().min(0), from: z.number(), every: z.number() }).default({ count: 0, from: 0, every: 1 }),
  labels: z.array(z.object({ text: z.string(), x: z.number(), y: z.number() })).default([]), // big typed words on the paper
});

const Stop = z.object({
  on: z.string().optional(), // a window id or "screen"; x/y are then offsets from its centre, otherwise world coordinates
  x: z.number().default(0),
  y: z.number().default(0),
  d: z.number().positive(), // camera distance; smaller is closer
  yaw: z.number().default(0),
  pitch: z.number().default(0),
  roll: z.number().default(0),
  atSec: z.number().min(0).default(0), // arrival time within the scene; the first stop is at 0
});

const Sound = z.enum(["whoosh", "stamp", "key", "bell"]);

const shot = {
  durationSec: z.number().positive().optional(),
  tag: z.string(), // small typed label above the title
  title: z.string(),
  sub: z.string().optional(),
  keys: z.array(z.string()).max(4).optional(), // key caps pressed in turn, each with a click
  stops: z.array(Stop).min(1).max(3),
  captionIn: z.number().min(0).optional(), // caption lands this long after the scene starts (theme default 0.4)
  captionOut: z.number().min(0).optional(), // and leaves this long before it ends (default 0.4)
  sfx: z.array(z.object({ sound: Sound, atSec: z.number().min(0), volume: z.number().min(0).max(1).default(0.5) })).default([]),
  say: z.string().optional(), // narration script (the hero has none; music only)
  narration: z.string().startsWith("narration/").optional(),
};

export const TourScene = z.discriminatedUnion("type", [
  z.object({ type: z.literal("open"), ...shot }), // big caption, lower left
  z.object({ type: z.literal("stop"), ...shot }), // caption, lower left
  z.object({ type: z.literal("close"), ...shot }), // big caption, centred end card
]);
export type TourSceneT = z.infer<typeof TourScene>;

export const TourProps = z
  .object({
    composition: z.literal("Tour"),
    aspect: z.enum(["16:9", "9:16", "1:1"]).default("16:9"),
    fps: z.number().default(30),
    music: z.object({ src: z.string().startsWith("music/"), volume: z.number().min(0).max(1).default(0.38), fadeInSec: z.number().default(1.5), fadeOutSec: z.number().default(4) }).optional(),
    sfxVolume: z.object({ whoosh: z.number().default(0.5), stamp: z.number().default(0.6), key: z.number().default(0.7) }).default({}),
    world: z.object({ windows: z.array(Window).min(1), screen: Screen.optional(), backdrop: Backdrop.optional() }),
    scenes: z.array(TourScene).min(1),
  })
  .superRefine((v, ctx) => {
    const ids = new Set([...v.world.windows.map((w) => w.id), ...(v.world.screen ? ["screen"] : [])]);
    if (ids.size !== v.world.windows.length + (v.world.screen ? 1 : 0)) ctx.addIssue({ code: "custom", path: ["world", "windows"], message: "window ids must be unique" });
    v.scenes.forEach((s, i) => {
      s.stops.forEach((st, j) => {
        if (st.on && !ids.has(st.on)) ctx.addIssue({ code: "custom", path: ["scenes", i, "stops", j, "on"], message: `no window "${st.on}" (have: ${[...ids].join(", ")})` });
        if (j === 0 && st.atSec !== 0) ctx.addIssue({ code: "custom", path: ["scenes", i, "stops", 0, "atSec"], message: "the first stop is at 0" });
        if (j > 0 && st.atSec <= s.stops[j - 1].atSec) ctx.addIssue({ code: "custom", path: ["scenes", i, "stops", j, "atSec"], message: "stops must arrive in order" });
      });
    });
  });
export type TourPropsT = z.infer<typeof TourProps>;
