// Front view plus one edge view (third-angle projection: the edge view shares the
// front view's x axis) for each supported body. Millimetres throughout. Which parts
// there are follows the "Parts and controls" figures in Sony's Help Guides (NW-A50
// 4-740-523-13, NW-ZX500 5-006-039-11); where they sit and how big they are was
// measured on product photographs scaled to Sony's published body sizes.

export type CalloutKey = "output" | "usb" | "card" | "battery" | "system";

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
  r: number;
}

/** The controls a guide can point at. */
export type SidePart = "power" | "volume" | "prev" | "play" | "next" | "hold";

export interface SideFeature {
  y0: number;
  y1: number;
  /** A button is round, or a stadium when it is taller than the side allows. */
  kind: "button" | "rocker" | "hold" | "usb-c" | "card-cover" | "card-tray";
  part?: SidePart;
  /** The tactile dot Sony puts on play and volume +. */
  dot?: boolean;
}

export interface EdgeFeature {
  x: number;
  kind: "jack-44" | "jack-35" | "wm-port" | "strap";
}

export interface Anchor {
  view: "front" | "edge";
  x: number;
  y: number;
  side: "left" | "right";
}

/** Corner radii of a front outline: some bodies round their two ends differently. */
export interface Ends {
  top: number;
  bottom: number;
}

export interface BodySpec {
  w: number;
  h: number;
  d: number;
  radius: Ends;
  /** The corner radius of the body's cross-section, as the edge view draws it. */
  section: number;
  /** How much of the frame shows around the front glass. */
  lip: number;
  glass: Ends;
  /** A round side key's width across the side, as a share of the side's depth. */
  key: number;
  display: Rect;
  lamp: { x: number; y: number };
  /** Ring around each headphone jack, where it is not the body's own colour. */
  jackRing?: string;
  right: SideFeature[];
  left: SideFeature[];
  edge: "top" | "bottom";
  edgeFeatures: EdgeFeature[];
  anchors: Record<CalloutKey, Anchor>;
}

const a50: BodySpec = {
  w: 55.7,
  h: 97.3,
  d: 10.8,
  radius: { top: 4.6, bottom: 4.6 },
  section: 4.1,
  lip: 2.1,
  glass: { top: 2.5, bottom: 2.5 },
  key: 0.55,
  // 3.1-inch 800 × 480 panel: 40.5 × 67.5 mm.
  display: { x: 7.6, y: 10.2, w: 40.5, h: 67.5, r: 0.8 },
  lamp: { x: 50.4, y: 5.2 },
  right: [
    { y0: 4.3, y1: 10, kind: "button", part: "power" },
    { y0: 14.8, y1: 33.7, kind: "rocker", part: "volume", dot: true },
    { y0: 39.8, y1: 44.8, kind: "button", part: "prev" },
    { y0: 48.4, y1: 53.3, kind: "button", part: "play", dot: true },
    { y0: 57.1, y1: 61.9, kind: "button", part: "next" },
    { y0: 69.7, y1: 78.4, kind: "hold", part: "hold" },
  ],
  left: [{ y0: 57, y1: 73, kind: "card-cover" }],
  edge: "bottom",
  edgeFeatures: [
    { x: 9.6, kind: "jack-35" },
    { x: 23, kind: "wm-port" },
    { x: 47.6, kind: "strap" },
  ],
  anchors: {
    output: { view: "edge", x: 9.6, y: 5.4, side: "left" },
    card: { view: "front", x: -0.4, y: 65, side: "left" },
    system: { view: "front", x: 27.85, y: 32, side: "right" },
    battery: { view: "front", x: 27.85, y: 88, side: "right" },
    usb: { view: "edge", x: 23, y: 5.4, side: "right" },
  },
};

const zx500: BodySpec = {
  w: 57.9,
  h: 122.6,
  d: 14.8,
  // Square at the top under the jacks, broadly rounded at the bottom; the glass follows.
  radius: { top: 0.5, bottom: 7.8 },
  section: 4.7,
  lip: 3.3,
  glass: { top: 1, bottom: 4.5 },
  key: 0.76,
  // 3.6-inch 1280 × 720 panel: 44.8 × 79.7 mm.
  display: { x: 6.55, y: 11.4, w: 44.8, h: 79.7, r: 0.8 },
  lamp: { x: 5.6, y: 116.6 },
  // Gold-plated jacks, on both finishes.
  jackRing: "#c8a96a",
  right: [
    { y0: 6.6, y1: 14.6, kind: "button", part: "power" },
    { y0: 21.9, y1: 31.8, kind: "button", part: "volume", dot: true },
    { y0: 33.2, y1: 43.8, kind: "button", part: "volume" },
    { y0: 49.7, y1: 59, kind: "button", part: "prev" },
    { y0: 62.3, y1: 71.6, kind: "button", part: "play", dot: true },
    { y0: 74.3, y1: 83.6, kind: "button", part: "next" },
    { y0: 90.2, y1: 96.8, kind: "hold", part: "hold" },
  ],
  left: [
    { y0: 27.5, y1: 37, kind: "usb-c" },
    { y0: 79.5, y1: 100, kind: "card-tray" },
  ],
  edge: "top",
  edgeFeatures: [
    { x: 11, kind: "jack-44" },
    { x: 47, kind: "jack-35" },
  ],
  anchors: {
    output: { view: "edge", x: 11, y: 7.4, side: "left" },
    usb: { view: "front", x: -0.4, y: 32.2, side: "left" },
    card: { view: "front", x: -0.4, y: 89.7, side: "left" },
    system: { view: "front", x: 28.95, y: 40, side: "right" },
    battery: { view: "front", x: 28.95, y: 106, side: "right" },
  },
};

export const BODIES: Record<string, BodySpec> = { a50, zx500 };

/** Gap between the front view and the edge view, in millimetres. */
export const VIEW_GAP = 7;
/** How far side controls stand off the body outline. */
export const SIDE_RELIEF = 1.1;
