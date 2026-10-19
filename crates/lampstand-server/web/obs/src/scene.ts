// The `Scene` JSON as serde writes it (crates/lampstand-core/src/scene.rs):
// enums are externally tagged ({"Group": [...]}, "Cover"), colours are
// straight-alpha sRGB bytes and a transform is CSS matrix(a, b, c, d, e, f).

export interface Point {
  x: number;
  y: number;
}
export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}
export interface Color {
  r: number;
  g: number;
  b: number;
  a: number;
}
export interface Transform {
  a: number;
  b: number;
  c: number;
  d: number;
  e: number;
  f: number;
}

export interface Scene {
  width: number;
  height: number;
  nodes: SceneNode[];
}

export interface SceneNode {
  id: number;
  transform: Transform;
  opacity: number;
  visible: boolean;
  kind: NodeKind;
}

export type NodeKind =
  | { Group: SceneNode[] }
  | { Path: PathNode }
  | { Text: TextBlock }
  | { Image: ImageNode }
  | { Video: VideoNode };

export interface PathNode {
  commands: PathCommand[];
  fill: Paint | null;
  stroke: Stroke | null;
  shadow: Shadow | null;
}

export type PathCommand =
  | { MoveTo: Point }
  | { LineTo: Point }
  | { QuadTo: [Point, Point] }
  | { CubicTo: [Point, Point, Point] }
  | "Close";

export interface GradientStop {
  offset: number;
  color: Color;
}

export type Paint =
  | { Solid: Color }
  | { Linear: { start: Point; end: Point; stops: GradientStop[] } }
  | { Radial: { center: Point; radius: number; stops: GradientStop[] } };

export interface Stroke {
  paint: Paint;
  width: number;
  dash: number[] | null;
}

export interface Shadow {
  color: Color;
  offset: Point;
  blur: number;
}

export type Fit = "Fill" | "Contain" | "Cover";

export interface ImageNode {
  blob: string;
  rect: Rect;
  fit: Fit;
  crop: Rect | null;
}

export interface VideoNode {
  blob: string;
  rect: Rect;
  fit: Fit;
  looped: boolean;
  muted: boolean;
  autoplay: boolean;
}

export type HAlign = "Left" | "Center" | "Right" | "Justify";
export type VAlign = "Top" | "Middle" | "Bottom";

export interface TextBlock {
  rect: Rect;
  valign: VAlign;
  paragraphs: Paragraph[];
  shadow: Shadow | null;
  outline: { color: Color; width: number } | null;
}

export interface Paragraph {
  align: HAlign;
  line_spacing: number;
  space_before: number;
  space_after: number;
  indent: number;
  bullet: string | null;
  runs: TextRun[];
}

export interface TextRun {
  text: string;
  font_family: string;
  size: number;
  weight: number;
  italic: boolean;
  underline: boolean;
  strike: boolean;
  color: Color;
}

/** What the WebSocket feed sends: a scene, or a deck slide as a WebP frame. */
export type Message =
  | { seq: number; scene: Scene }
  | { seq: number; width: number; height: number; frame: number };
