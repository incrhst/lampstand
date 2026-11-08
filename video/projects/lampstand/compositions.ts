// Every composition this project registers, with its scene-list schema and default scene length.
// core/scripts/validate.ts and render.mjs pick one by the scene list's "composition" field (default "Video").
import { VideoProps } from "./schema";
import { defaultSec } from "./duration";
import { TourProps } from "./tour/schema";
import { tourDefaultSec } from "./tour/timeline";

export const compositions = {
  Video: { schema: VideoProps, defaultSec },
  Tour: { schema: TourProps, defaultSec: tourDefaultSec as (s: any) => number },
};
