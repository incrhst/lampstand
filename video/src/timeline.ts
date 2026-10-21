// The whole film as data: where each window plane sits in the world, where
// the camera is at each beat, and what the captions say. Times are seconds.

export const FPS = 30;
export const PERSPECTIVE = 1400;

export type Plane = {
  id: string;
  shot: string; // file in public/shots
  x: number;
  y: number;
  z: number;
  yaw: number; // degrees about the vertical axis
  scale: number;
  w: number; // the screenshot's CSS size
  h: number;
  title: string;
};

const WIN = { w: 1280, h: 800 };
export const planes: Plane[] = [
  { id: "start", shot: "1-getstarted.png", x: 0, y: 0, z: 0, yaw: 0, scale: 1, ...WIN, title: "Lampstand" },
  { id: "plan", shot: "2-plan.png", x: 1500, y: 40, z: -60, yaw: -7, scale: 1, ...WIN, title: "Lampstand · Plan" },
  { id: "quick", shot: "3-quickadd.png", x: 2150, y: 360, z: 170, yaw: -7, scale: 0.72, ...WIN, title: "Quick add" },
  { id: "decks", shot: "4-decks.png", x: 3450, y: -40, z: -30, yaw: -7, scale: 1, ...WIN, title: "Lampstand · Plan" },
  { id: "present", shot: "5-present.png", x: 5000, y: 20, z: 0, yaw: -5, scale: 1, ...WIN, title: "Lampstand · Present" },
  { id: "notes", shot: "5b-present-deck.png", x: 5620, y: 380, z: 170, yaw: -5, scale: 0.72, ...WIN, title: "Speaker notes" },
  { id: "obs", shot: "7-obs.png", x: 8650, y: 10, z: -20, yaw: 6, scale: 1, ...WIN, title: "Lampstand · Setup" },
  { id: "obslive", shot: "8-present-obs.png", x: 9300, y: 400, z: 170, yaw: 6, scale: 0.72, ...WIN, title: "Lampstand · Present" },
  { id: "screens", shot: "6-setup.png", x: 10800, y: 0, z: 0, yaw: 4, scale: 1, ...WIN, title: "Lampstand · Setup" },
];

export const plane = (id: string) => planes.find((p) => p.id === id)!;

// The projector screen the congregation sees. It is not a screenshot of the app:
// the real one is drawn by the renderer, so it is shown here as lyrics on the
// site's own screen colours.
export const OUTPUT = { x: 6850, y: -20, z: 0, yaw: -3, w: 1600, h: 900 };

export type Cam = { t: number; x: number; y: number; d: number; yaw: number; pitch: number; roll: number };

const at = (id: string, ox = 0, oy = 0) => ({ x: plane(id).x + ox, y: plane(id).y + oy });

// Each beat arrives at its time and rests until the next one starts to move.
export const cams: Cam[] = [
  { t: 0, x: 2600, y: 200, d: 3600, yaw: -26, pitch: 34, roll: -4 },
  { t: 3.6, ...at("start", 40, 20), d: 1790, yaw: 16, pitch: 12, roll: -1.5 },
  { t: 7.2, ...at("start", -120, -300), d: 1060, yaw: 10, pitch: 6, roll: 0 },
  { t: 11.2, ...at("plan", 0, 20), d: 1610, yaw: 12, pitch: 11, roll: -1 },
  { t: 16.4, ...at("quick", 0, 10), d: 1260, yaw: 8, pitch: 10, roll: 1 },
  { t: 21.4, ...at("decks", 0, 0), d: 1540, yaw: 12, pitch: 11, roll: -1 },
  { t: 26.4, ...at("present", -20, 10), d: 1540, yaw: 8, pitch: 12, roll: 0.5 },
  { t: 30.4, ...at("present", -380, 0), d: 1060, yaw: 6, pitch: 8, roll: 0 },
  { t: 34.6, ...at("notes", 120, -10), d: 1260, yaw: 8, pitch: 10, roll: -1 },
  { t: 40.4, x: OUTPUT.x, y: OUTPUT.y + 20, d: 1900, yaw: 4, pitch: 8, roll: 0 },
  { t: 47.2, ...at("obs", 0, 10), d: 1540, yaw: -12, pitch: 10, roll: 1 },
  { t: 52.4, ...at("obslive", -10, 0), d: 1330, yaw: -10, pitch: 10, roll: -1 },
  { t: 58.4, ...at("screens", 0, 0), d: 1470, yaw: -8, pitch: 11, roll: 0.5 },
  { t: 64.4, ...at("screens", 600, 200), d: 2800, yaw: -18, pitch: 30, roll: 2 },
];
export const DURATION_S = 72;
export const TOTAL = DURATION_S * FPS;

export type Caption = { from: number; to: number; tag: string; title: string; sub?: string; keys?: string[] };
export const captions: Caption[] = [
  { from: 0.4, to: 3.4, tag: "Lampstand", title: "Free projection for your church service.", sub: "Songs, scripture and slides. Works offline." },
  { from: 4.2, to: 10.4, tag: "1 · Start", title: "Try the Welcome service first.", sub: "A Get started strip walks you through it." },
  { from: 11.6, to: 16, tag: "2 · Plan", title: "Build the order of service.", sub: "Songs, scripture, slides and video, in order." },
  { from: 16.8, to: 21, tag: "Quick add", title: "Type John 3:16 and press Enter.", sub: "Songs and verses drop straight into the order.", keys: ["/"] },
  { from: 21.8, to: 26, tag: "Slides", title: "Bring your PowerPoint or Google Slides.", sub: "Animation steps and speaker notes come too." },
  { from: 26.8, to: 34, tag: "3 · Present", title: "Choose the next item. Start it.", sub: "Space moves on. Left goes back. Period blanks the screen.", keys: ["⇧", "↓", "Enter"] },
  { from: 35, to: 40, tag: "Notes", title: "Your notes stay on your screen.", sub: "The congregation never sees them." },
  { from: 40.8, to: 47, tag: "The screen", title: "The congregation sees only the words.", sub: "Words stay inside the safe area of each screen." },
  { from: 47.6, to: 58, tag: "OBS", title: "Send lyrics to your live stream.", sub: "A browser source in OBS, no plugin. Switch scenes with the service." },
  { from: 58.8, to: 64, tag: "Setup", title: "Fit each screen once.", sub: "Calibrate so words never fall off the edge." },
  { from: 65, to: 71.6, tag: "Lampstand", title: "Free and open source.", sub: "GPL-3.0 · github.com/incrhst/lampstand" },
];

export const lyrics = [
  ["Amazing grace, how sweet the sound", "That saved a wretch like me"],
  ["'Twas grace that taught my heart to fear", "And grace my fears relieved"],
  ["Through many dangers, toils and snares", "I have already come"],
];
