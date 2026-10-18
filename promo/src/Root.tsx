import { Composition } from "remotion";
import { Promo, TOTAL_FRAMES } from "./Promo";
import T from "./timeline.json";

export const RemotionRoot: React.FC = () => (
  <Composition
    id="Promo"
    component={Promo}
    durationInFrames={TOTAL_FRAMES}
    fps={T.fps}
    width={T.width}
    height={T.height}
    defaultProps={{ withAudio: true }}
  />
);
