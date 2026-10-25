import React from "react";
import { Composition } from "remotion";
import { makeCalculateMetadata, makeMasterVideo } from "@video/core";
import { theme } from "./theme";
import { registry, customRegistry } from "./registry";
import { defaultSec } from "./duration";
import { VideoProps } from "./schema";
import sample from "./videos/sample-what-is-lampstand.json";

const Master = makeMasterVideo({ registry, customRegistry, defaultSec, theme, schema: VideoProps });

export const Root: React.FC = () => (
  <Composition
    id="Video"
    component={Master as any}
    schema={VideoProps as any}
    defaultProps={VideoProps.parse(sample) as any}
    durationInFrames={30}
    fps={30}
    width={1920}
    height={1080}
    calculateMetadata={makeCalculateMetadata(defaultSec, VideoProps) as any}
  />
);
