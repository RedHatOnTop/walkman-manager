import { useId } from "react";
import { outline } from "./outline";
import { BODIES, SIDE_RELIEF, VIEW_GAP, type Anchor, type BodySpec, type CalloutKey, type EdgeFeature, type SideFeature } from "./specs";

export type Tone = "plain" | "live" | "warn" | "muted";

export interface Callout {
  key: CalloutKey;
  label: string;
  value: string;
  tone?: Tone;
}

interface Props {
  art: string;
  /** Pixels per millimetre. */
  scale?: number;
  callouts?: Callout[];
  /** Screen lit: the player is booted and answering. */
  awake?: boolean;
  playing?: boolean;
  charging?: boolean;
  dimension?: boolean;
  className?: string;
}

const COL = 156;
const GAP = 44;
const PAD_Y = 28;
const DIM_SPACE = 13;
const LABEL_H = 38;

interface Layout {
  width: number;
  height: number;
  originX: number;
  frontY: number;
  edgeY: number;
}

function layout(spec: BodySpec, s: number, annotated: boolean, dimension: boolean): Layout {
  const col = annotated ? COL + GAP : 14;
  const bodyW = (spec.w + 2 * SIDE_RELIEF) * s;
  const viewsH = (spec.h + VIEW_GAP + spec.d) * s;
  const frontY = spec.edge === "top" ? spec.d + VIEW_GAP : 0;
  const edgeY = spec.edge === "top" ? 0 : spec.h + VIEW_GAP;
  return {
    width: Math.round(2 * col + bodyW),
    height: Math.round(PAD_Y * 2 + viewsH + (dimension ? DIM_SPACE * s : 0)),
    originX: col + SIDE_RELIEF * s,
    frontY,
    edgeY,
  };
}

function anchorPx(a: Anchor, l: Layout, s: number) {
  const viewY = a.view === "front" ? l.frontY : l.edgeY;
  return { x: l.originX + a.x * s, y: PAD_Y + (viewY + a.y) * s };
}

/** Spreads labels on one side so none overlap, keeping each near its anchor. */
function placeLabels(desired: number[], min: number, max: number): number[] {
  const order = desired.map((y, i) => ({ y, i })).sort((a, b) => a.y - b.y);
  const placed: number[] = [];
  for (const [n, o] of order.entries()) {
    const prev = n > 0 ? placed[n - 1]! : -Infinity;
    placed.push(Math.max(o.y, prev + LABEL_H + 8, min));
  }
  for (let n = placed.length - 1; n >= 0; n--) {
    const next = n < placed.length - 1 ? placed[n + 1]! - LABEL_H - 8 : max;
    placed[n] = Math.min(placed[n]!, next);
  }
  const out = new Array<number>(desired.length);
  order.forEach((o, n) => (out[o.i] = placed[n]!));
  return out;
}

function SideControls({ features, side, w }: { features: SideFeature[]; side: "left" | "right"; w: number }) {
  return (
    <>
      {features.map((f, i) => {
        const h = f.y1 - f.y0;
        if (side === "right") {
          const x = w - 0.5;
          const rw = SIDE_RELIEF + 0.5;
          return (
            <g key={i}>
              <rect className="dw-part" x={x} y={f.y0} width={rw} height={h} rx={0.55} />
              {f.kind === "rocker" && <line className="dw-hair" x1={x + 0.5} x2={x + rw} y1={f.y0 + h / 2} y2={f.y0 + h / 2} />}
              {f.kind === "hold" && <line className="dw-hair" x1={x + rw * 0.55} x2={x + rw * 0.55} y1={f.y0 + 1.2} y2={f.y1 - 1.2} />}
            </g>
          );
        }
        if (f.kind === "usb-c") {
          return <rect key={i} className="dw-port" x={-0.65} y={f.y0} width={1.3} height={h} rx={0.65} />;
        }
        return (
          <g key={i}>
            <rect className="dw-part" x={-0.55} y={f.y0} width={1.1} height={h} rx={0.4} />
            {f.kind === "card-tray" && <circle className="dw-hair" cx={0} cy={f.y1 - 2.2} r={0.45} />}
          </g>
        );
      })}
    </>
  );
}

function EdgeView({ spec, y, playing }: { spec: BodySpec; y: number; playing: boolean }) {
  const cy = y + spec.d / 2;
  const r = Math.min(spec.section, spec.d / 2);
  const feature = (f: EdgeFeature, i: number) => {
    switch (f.kind) {
      case "jack-44":
        return (
          <g key={i} className={playing ? "dw-jack dw-jack--live" : "dw-jack"}>
            <circle className="dw-part" cx={f.x} cy={cy} r={4.1} />
            <circle className="dw-port" cx={f.x} cy={cy} r={2.2} />
          </g>
        );
      case "jack-35":
        return (
          <g key={i} className={playing && spec.edgeFeatures.every((e) => e.kind !== "jack-44") ? "dw-jack dw-jack--live" : "dw-jack"}>
            <circle className="dw-part" cx={f.x} cy={cy} r={3.1} />
            <circle className="dw-port" cx={f.x} cy={cy} r={1.75} />
          </g>
        );
      case "wm-port":
        return <rect key={i} className="dw-port" x={f.x - 6.4} y={cy - 1.35} width={12.8} height={2.7} rx={1.1} />;
      case "strap":
        return <rect key={i} className="dw-port" x={f.x - 1.7} y={cy - 0.85} width={3.4} height={1.7} rx={0.85} />;
    }
  };
  return (
    <g>
      <rect className="dw-body" x={0} y={y} width={spec.w} height={spec.d} rx={r} />
      <line className="dw-hair" x1={r} x2={spec.w - r} y1={spec.edge === "top" ? y + spec.d - 1.4 : y + 1.4} y2={spec.edge === "top" ? y + spec.d - 1.4 : y + 1.4} />
      {spec.edgeFeatures.map(feature)}
    </g>
  );
}

export function DeviceDrawing({ art, scale = 3.1, callouts = [], awake = false, playing = false, charging = false, dimension = false, className }: Props) {
  const uid = useId().replace(/:/g, "");
  const spec = BODIES[art];
  if (!spec) return null;
  const s = scale;
  const annotated = callouts.length > 0;
  const l = layout(spec, s, annotated, dimension);
  const d = spec.display;
  const fy = l.frontY;

  const items = callouts.map((c) => ({ c, a: spec.anchors[c.key], p: anchorPx(spec.anchors[c.key], l, s) }));
  const sides = { left: items.filter((i) => i.a.side === "left"), right: items.filter((i) => i.a.side === "right") };
  const placed = new Map<string, number>();
  for (const group of [sides.left, sides.right]) {
    const ys = placeLabels(
      group.map((g) => g.p.y - LABEL_H / 2),
      PAD_Y / 2,
      l.height - LABEL_H - PAD_Y / 2,
    );
    group.forEach((g, n) => placed.set(g.c.key, ys[n]!));
  }

  const bodyLeft = l.originX - SIDE_RELIEF * s;
  const bodyRight = l.originX + (spec.w + SIDE_RELIEF) * s;
  const dimY = PAD_Y + (spec.h + VIEW_GAP + spec.d + 6.5) * s;

  return (
    <div className={`dw ${className ?? ""}`} style={{ width: l.width, height: l.height }}>
      <svg className="dw-svg" width={l.width} height={l.height} viewBox={`0 0 ${l.width} ${l.height}`} aria-hidden="true">
        <defs>
          <linearGradient id={`scr-${uid}`} x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stopColor="var(--glass-sheen)" />
            <stop offset="0.42" stopColor="transparent" />
          </linearGradient>
        </defs>

        <g transform={`translate(${l.originX} ${PAD_Y}) scale(${s})`}>
          <line className="dw-center" x1={spec.w / 2} x2={spec.w / 2} y1={-3} y2={spec.h + VIEW_GAP + spec.d + 3} />
          <EdgeView spec={spec} y={l.edgeY} playing={playing} />

          <g transform={`translate(0 ${fy})`}>
            <SideControls features={spec.right} side="right" w={spec.w} />
            <SideControls features={spec.left} side="left" w={spec.w} />
            <path className="dw-body" d={outline(0, 0, spec.w, spec.h, spec.radius.top, spec.radius.bottom)} />
            <path className="dw-bezel" d={outline(spec.lip, spec.lip, spec.w - 2 * spec.lip, spec.h - 2 * spec.lip, spec.glass.top, spec.glass.bottom)} />
            <rect className={awake ? "dw-screen dw-screen--awake" : "dw-screen"} x={d.x} y={d.y} width={d.w} height={d.h} rx={d.r} />
            <rect x={d.x} y={d.y} width={d.w} height={d.h} rx={d.r} fill={`url(#scr-${uid})`} />
            <circle className={charging ? "dw-lamp dw-lamp--on" : "dw-lamp"} cx={spec.lamp.x} cy={spec.lamp.y} r={0.75} />
          </g>
        </g>

        {dimension && (
          <g className="dw-dim">
            <line x1={l.originX} x2={l.originX} y1={dimY - 9} y2={dimY + 4} />
            <line x1={l.originX + spec.w * s} x2={l.originX + spec.w * s} y1={dimY - 9} y2={dimY + 4} />
            <line x1={l.originX} x2={l.originX + spec.w * s} y1={dimY} y2={dimY} />
            <path d={`M${l.originX + 6} ${dimY - 3} L${l.originX} ${dimY} L${l.originX + 6} ${dimY + 3}`} />
            <path d={`M${l.originX + spec.w * s - 6} ${dimY - 3} L${l.originX + spec.w * s} ${dimY} L${l.originX + spec.w * s - 6} ${dimY + 3}`} />
          </g>
        )}

        {items.map(({ c, a, p }) => {
          const ly = (placed.get(c.key) ?? p.y) + LABEL_H / 2;
          const left = a.side === "left";
          const elbow = left ? bodyLeft - 18 : bodyRight + 18;
          const end = left ? COL + 6 : l.width - COL - 6;
          const slant = left ? -12 : 12;
          const path = `M${p.x} ${p.y} H${elbow} L${elbow + slant} ${ly} H${end}`;
          return (
            <g key={c.key} className={`dw-lead dw-lead--${c.tone ?? "plain"}`}>
              <path d={path} />
              <circle className="dw-anchor-ring" cx={p.x} cy={p.y} r={4.5} />
              <circle className="dw-anchor" cx={p.x} cy={p.y} r={2.1} />
            </g>
          );
        })}
      </svg>

      {dimension && (
        <span className="dw-dim-label" style={{ left: l.originX + (spec.w * s) / 2, top: dimY + 7 }}>
          {spec.w.toFixed(1)} mm
        </span>
      )}

      {items.map(({ c, a }) => (
        <div
          key={c.key}
          className={`dw-callout dw-callout--${a.side} dw-callout--${c.tone ?? "plain"}`}
          style={a.side === "left" ? { top: placed.get(c.key), width: COL - 4, left: 0 } : { top: placed.get(c.key), width: COL - 4, right: 0 }}
        >
          <span className="dw-callout-label">{c.label}</span>
          <span className="dw-callout-value">{c.value}</span>
        </div>
      ))}
    </div>
  );
}
