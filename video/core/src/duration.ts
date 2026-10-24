export const WPS = 3; // reading speed, words per second
export const words = (s = "") => (s.trim() ? s.trim().split(/\s+/).length : 0);

export function coreDefaultSec(s: any): number | undefined {
  switch (s.type) {
    case "title": return 2.5 + words(`${s.kicker ?? ""} ${s.heading} ${s.sub ?? ""}`) / WPS;
    case "bullets": return 1.5 + words(`${s.heading ?? ""} ${s.items.join(" ")}`) / WPS + s.items.length * 0.4;
    case "stat": return 3.5 + words(s.label) / WPS;
    case "cta": return 3 + words(`${s.heading} ${s.action}`) / WPS;
    case "custom": return 4;
  }
}

export const SIZES = { "16:9": [1920, 1080], "9:16": [1080, 1920], "1:1": [1080, 1080] } as const;

export const sceneFrames = (s: any, fps: number, defaultSec: (s: any) => number) =>
  Math.max(1, Math.round((s.durationSec ?? defaultSec(s)) * fps));

export function makeCalculateMetadata(defaultSec: (s: any) => number, schema: { parse: (v: any) => any }) {
  return ({ props: raw }: { props: any }) => {
    const props = schema.parse(raw); // inputProps arrive unparsed; apply schema defaults
    const [width, height] = SIZES[props.aspect as keyof typeof SIZES];
    const durationInFrames = props.scenes.reduce((n: number, s: any) => n + sceneFrames(s, props.fps, defaultSec), 0);
    return { durationInFrames, width, height, fps: props.fps };
  };
}
