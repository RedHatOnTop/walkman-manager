import { useEffect, useId, useState, type CSSProperties } from "react";
import { outline } from "../drawing/outline";
import { BODIES, type BodySpec, type Ends, type SideFeature, type SidePart } from "../drawing/specs";
import { FINISHES, type Finish } from "./finishes";
import { dictionaries } from "../../i18n/strings";
import { GuideScreen, type GuideScene } from "../guide/GuideScreen";

/** What the player's own screen shows, as far as the manager can tell. */
export type ScreenState =
  | { kind: "off" }
  | { kind: "idle" }
  | { kind: "playing"; big: string; unit: string; detail: string }
  | { kind: "auth" }
  | { kind: "busy" }
  | { kind: "guide"; scene: GuideScene };

export type { SidePart };

/** A control a guide points at: a side part, or one end of the volume keys. */
export type CueKey = SidePart | "volume-up" | "volume-down";

/**
 * Where on the right side a control is, in millimetres from the top, and how wide its key
 * is across the side. Sony puts volume + above volume −, on two keys or on one rocker.
 */
export function cueSpan(spec: BodySpec, key: CueKey): { y0: number; y1: number; bw: number } | undefined {
  const end = key === "volume-up" ? "up" : key === "volume-down" ? "down" : null;
  const keys = spec.right.filter((p) => p.part === (end ? "volume" : key));
  const first = keys[0];
  const last = keys[keys.length - 1];
  if (!first || !last) return undefined;
  const k = end === "down" ? last : first;
  const bw = k.kind === "rocker" || k.kind === "hold" ? spec.d * 0.4 : Math.min(k.y1 - k.y0, spec.d * spec.key);
  if (!end) return { y0: first.y0, y1: last.y1, bw };
  if (keys.length > 1) return { y0: k.y0, y1: k.y1, bw };
  const mid = (k.y0 + k.y1) / 2;
  return end === "up" ? { y0: k.y0, y1: mid, bw } : { y0: mid, y1: k.y1, bw };
}

interface Props {
  art: string;
  /** Pixels per millimetre; both bodies share one scale, so their sizes compare. */
  scale: number;
  finish?: Finish;
  screen: ScreenState;
  battery?: number | null;
  charging?: boolean;
  /** Turned toward the viewer to show the side and top; flat when false. */
  turned?: boolean;
  /** Output jack carrying a live stream. */
  live?: boolean;
  /** Side controls a guide asks the owner to press. */
  cue?: CueKey[];
  lang?: "ko" | "en";
  className?: string;
  style?: CSSProperties;
}

function useClock(on: boolean) {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    if (!on) return;
    const timer = window.setInterval(() => setNow(new Date()), 15_000);
    return () => window.clearInterval(timer);
  }, [on]);
  return now;
}

function Screen({ w, h, state, battery, charging, lang, uid }: { w: number; h: number; state: ScreenState; battery: number | null; charging: boolean; lang: "ko" | "en"; uid: string }) {
  const now = useClock(state.kind === "idle");
  if (state.kind === "off") return null;
  if (state.kind === "guide") return <GuideScreen scene={state.scene} w={w} h={h} lang={lang} />;
  const u = w / 100;
  const time = now.toLocaleTimeString(lang === "ko" ? "ko-KR" : "en-GB", { hour: "2-digit", minute: "2-digit", hour12: false });
  const date = now.toLocaleDateString(lang === "ko" ? "ko-KR" : "en-GB", { month: "long", day: "numeric", weekday: "short" });
  const status = (
    <g className="scr-status">
      <text x={5 * u} y={7 * u} fontSize={3.6 * u} className="scr-dim">
        {time}
      </text>
      {battery != null && (
        <g transform={`translate(${w - 15.5 * u} ${3.9 * u})`}>
          <rect width={9 * u} height={4 * u} rx={1 * u} className="scr-batt" strokeWidth={0.45 * u} />
          <rect x={0.9 * u} y={0.9 * u} width={Math.max(0.4, (7.2 * u * battery) / 100)} height={2.2 * u} rx={0.4 * u} className={charging ? "scr-batt-fill is-charging" : "scr-batt-fill"} />
          <rect x={9.2 * u} y={1.3 * u} width={0.8 * u} height={1.4 * u} rx={0.3 * u} className="scr-batt-tip" />
        </g>
      )}
    </g>
  );

  if (state.kind === "idle") {
    return (
      <g>
        <rect width={w} height={h} fill={`url(#scr-idle-${uid})`} />
        {status}
        <text x={w / 2} y={h * 0.42} textAnchor="middle" fontSize={25 * u} className="scr-clock">
          {time}
        </text>
        <text x={w / 2} y={h * 0.42 + 10 * u} textAnchor="middle" fontSize={4.6 * u} className="scr-dim">
          {date}
        </text>
      </g>
    );
  }

  if (state.kind === "playing") {
    // Laid out like the players' own now-playing screen: format, cover, title, progress, keys.
    const art = 76 * u;
    const ax = (w - art) / 2;
    const ay = 17 * u;
    const ty = ay + art + 10 * u;
    const py = ty + 15 * u;
    const ky = py + 14 * u;
    return (
      <g>
        <rect width={w} height={h} className="scr-black" />
        {status}
        <text x={w / 2} y={11.6 * u} textAnchor="middle" fontSize={3.3 * u} className="scr-accent">
          {`${state.big} ${state.unit}`}
        </text>
        <rect x={ax} y={ay} width={art} height={art} rx={1 * u} className="scr-art" />
        <circle cx={w / 2} cy={ay + art / 2} r={art * 0.3} className="scr-art-disc" strokeWidth={0.5 * u} />
        <circle cx={w / 2} cy={ay + art / 2} r={art * 0.05} className="scr-art-disc" strokeWidth={0.5 * u} />
        <rect x={w / 2 - 22 * u} y={ty} width={44 * u} height={2.8 * u} rx={1.4 * u} className="scr-line" />
        <rect x={w / 2 - 15 * u} y={ty + 5.4 * u} width={30 * u} height={2.2 * u} rx={1.1 * u} className="scr-line scr-line--dim" />
        <rect x={8 * u} y={py} width={84 * u} height={0.7 * u} className="scr-track" />
        <rect x={8 * u} y={py} width={84 * u * 0.36} height={0.7 * u} className="scr-progress" />
        <circle cx={8 * u + 84 * u * 0.36} cy={py + 0.35 * u} r={1.4 * u} className="scr-progress" />
        <g transform={`translate(${w / 2} ${ky})`} className="scr-keys">
          <path d={`M ${-24 * u} ${-3 * u} V ${3 * u} M ${-22.5 * u} 0 L ${-17 * u} ${-3 * u} V ${3 * u} Z`} strokeWidth={0.8 * u} />
          <rect x={-2.6 * u} y={-3.4 * u} width={1.8 * u} height={6.8 * u} rx={0.4 * u} />
          <rect x={0.8 * u} y={-3.4 * u} width={1.8 * u} height={6.8 * u} rx={0.4 * u} />
          <path d={`M ${24 * u} ${-3 * u} V ${3 * u} M ${22.5 * u} 0 L ${17 * u} ${-3 * u} V ${3 * u} Z`} strokeWidth={0.8 * u} />
        </g>
        <text x={w / 2} y={ky + 13 * u} textAnchor="middle" fontSize={3 * u} className="scr-dim">
          {state.detail}
        </text>
      </g>
    );
  }

  if (state.kind === "auth") {
    const cw = 84 * u;
    const cx = (w - cw) / 2;
    const cy = h * 0.32;
    return (
      <g>
        <rect width={w} height={h} className="scr-dimmed" />
        {status}
        <rect x={cx} y={cy} width={cw} height={58 * u} rx={5 * u} className="scr-card" />
        <text x={cx + 6 * u} y={cy + 12 * u} fontSize={5.2 * u} className="scr-text scr-bold">
          {dictionaries[lang]["render.rsa.title"]}
        </text>
        <text x={cx + 6 * u} y={cy + 20 * u} fontSize={3.6 * u} className="scr-dim">
          {dictionaries[lang]["render.rsa.key"]}
        </text>
        <rect x={cx + 6 * u} y={cy + 27 * u} width={4 * u} height={4 * u} rx={0.8 * u} className="scr-check" />
        <text x={cx + 12.5 * u} y={cy + 30.4 * u} fontSize={3.6 * u} className="scr-text">
          {dictionaries[lang]["render.rsa.always"]}
        </text>
        <rect x={cx + cw - 30 * u} y={cy + 41 * u} width={24 * u} height={10 * u} rx={5 * u} className="scr-allow" />
        <text x={cx + cw - 18 * u} y={cy + 47.3 * u} textAnchor="middle" fontSize={4 * u} className="scr-on-accent scr-bold">
          {dictionaries[lang]["render.rsa.allow"]}
        </text>
      </g>
    );
  }

  return (
    <g>
      <rect width={w} height={h} className="scr-dimmed" />
      <circle cx={w / 2} cy={h / 2} r={7 * u} className="scr-spinner" strokeWidth={1.2 * u} strokeDasharray={`${20 * u} ${30 * u}`} />
    </g>
  );
}

/*
 * No marks are drawn on the body: Sony's wordmark and the Walkman logo are Sony's trademarks,
 * and this program is not Sony's.
 */
function Front({ spec, f, screen, battery, charging, lang }: { spec: BodySpec; f: (typeof FINISHES)[Finish]; screen: ScreenState; battery: number | null; charging: boolean; lang: "ko" | "en" }) {
  const uid = useId().replace(/:/g, "");
  const { w, h, radius: r, glass: g, display: d, lip } = spec;
  const sw = d.w * 10;
  const sh = d.h * 10;
  const body = outline(0, 0, w, h, r.top, r.bottom);
  const pane = outline(lip, lip, w - 2 * lip, h - 2 * lip, g.top, g.bottom);
  return (
    <svg className="r3d-face r3d-front" viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none" aria-hidden="true">
      <defs>
        <linearGradient id={`frame-${uid}`} x1="0" y1="0" x2="0.35" y2="1">
          <stop offset="0" stopColor={f.hi} />
          <stop offset="0.18" stopColor={f.side} />
          <stop offset="0.6" stopColor={f.mid} />
          <stop offset="1" stopColor={f.lo} />
        </linearGradient>
        <linearGradient id={`glass-${uid}`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#15171c" />
          <stop offset="0.45" stopColor="#0a0b0e" />
          <stop offset="1" stopColor="#060709" />
        </linearGradient>
        <linearGradient id={`reflect-${uid}`} x1="0" y1="0" x2="0.55" y2="1">
          <stop offset="0" stopColor="#fff" stopOpacity="0.075" />
          <stop offset="0.4" stopColor="#fff" stopOpacity="0.018" />
          <stop offset="0.62" stopColor="#fff" stopOpacity="0" />
        </linearGradient>
        <linearGradient id={`scr-idle-${uid}`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#101217" />
          <stop offset="1" stopColor="#050608" />
        </linearGradient>
        <linearGradient id={`chamfer-${uid}`} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#fff" stopOpacity={f.edge * 1.2} />
          <stop offset="0.5" stopColor="#fff" stopOpacity={f.edge * 0.3} />
          <stop offset="1" stopColor="#fff" stopOpacity={f.edge * 0.5} />
        </linearGradient>
        <clipPath id={`bodyclip-${uid}`}>
          <path d={body} />
        </clipPath>
      </defs>
      <path d={body} fill={`url(#frame-${uid})`} />
      {/* The rounded edge where the front turns into the sides catches more light than the flat frame. */}
      <g clipPath={`url(#bodyclip-${uid})`} fill="none" stroke="#fff" strokeOpacity={f.edge * 0.4}>
        <path d={body} strokeWidth={2.4} />
        <path d={body} strokeWidth={1} />
      </g>
      <path d={outline(0.15, 0.15, w - 0.3, h - 0.3, Math.max(r.top - 0.15, 0), r.bottom - 0.15)} fill="none" stroke="#000" strokeOpacity={0.35} strokeWidth={0.3} />
      {/* The polished chamfer where the frame turns in to the glass. */}
      <path d={outline(lip - 0.22, lip - 0.22, w - 2 * lip + 0.44, h - 2 * lip + 0.44, g.top + 0.22, g.bottom + 0.22)} fill="none" stroke={`url(#chamfer-${uid})`} strokeWidth={0.32} />
      <path d={pane} fill={`url(#glass-${uid})`} />
      <path d={pane} fill="none" stroke="#000" strokeOpacity={0.8} strokeWidth={0.14} />
      <svg x={d.x} y={d.y} width={d.w} height={d.h} viewBox={`0 0 ${sw} ${sh}`} className="r3d-screen">
        <rect width={sw} height={sh} className="scr-off" />
        <Screen w={sw} h={sh} state={screen} battery={battery} charging={charging} lang={lang} uid={uid} />
      </svg>
      <circle cx={spec.lamp.x} cy={spec.lamp.y} r={0.38} className={charging ? "r3d-lamp is-on" : "r3d-lamp"} />
      <path d={pane} fill={`url(#reflect-${uid})`} />
    </svg>
  );
}

/** A glyph engraved in a key, centred on (0, 0), `s` across. */
function Engraving({ part, plus, s }: { part: SidePart | undefined; plus: boolean; s: number }) {
  const k = s / 10;
  const line = { fill: "none", strokeWidth: 1.1 * k, strokeLinecap: "round" as const };
  switch (part) {
    case "power":
      return (
        <g className="r3d-engrave">
          <path d={`M ${-2.6 * k} ${-2.2 * k} A ${3.4 * k} ${3.4 * k} 0 1 0 ${2.6 * k} ${-2.2 * k}`} {...line} />
          <path d={`M 0 ${-4 * k} V ${-0.4 * k}`} {...line} />
        </g>
      );
    case "volume":
      return (
        <g className="r3d-engrave">
          <path d={`M ${-3 * k} 0 H ${3 * k}`} {...line} />
          {plus && <path d={`M 0 ${-3 * k} V ${3 * k}`} {...line} />}
        </g>
      );
    case "prev":
    case "next": {
      const m = part === "next" ? -1 : 1;
      return (
        <g className="r3d-engrave" transform={`scale(${m} 1)`}>
          <path d={`M ${-3.4 * k} ${-2.6 * k} V ${2.6 * k}`} {...line} />
          <path d={`M ${-2.6 * k} 0 L ${0.2 * k} ${-2.4 * k} V ${2.4 * k} Z M ${0.2 * k} 0 L ${3 * k} ${-2.4 * k} V ${2.4 * k} Z`} />
        </g>
      );
    }
    case "play":
      return (
        <g className="r3d-engrave">
          <path d={`M ${-3.4 * k} ${-2.6 * k} L ${0.2 * k} 0 L ${-3.4 * k} ${2.6 * k} Z`} />
          <path d={`M ${1.8 * k} ${-2.4 * k} V ${2.4 * k} M ${3.4 * k} ${-2.4 * k} V ${2.4 * k}`} {...line} />
        </g>
      );
    default:
      return null;
  }
}

/**
 * Across the side from the front edge to the back: the frame is rounded, so it catches the
 * light in a band a third of the way in and falls off towards both edges.
 */
const SIDE_SHADE: [number, "lo" | "mid" | "side" | "hi"][] = [
  [0, "mid"],
  [0.14, "side"],
  [0.34, "hi"],
  [0.58, "side"],
  [0.86, "mid"],
  [1, "lo"],
];

/** One control on the right side, drawn across the side's depth `d`. */
function SideControl({ p, d, share, f }: { p: SideFeature; d: number; share: number; f: (typeof FINISHES)[Finish] }) {
  const uid = useId().replace(/:/g, "");
  const len = p.y1 - p.y0;
  const cx = d / 2;
  const cy = (p.y0 + p.y1) / 2;
  // A key is a low dome in the body's own metal, lit from above.
  const fill = (
    <linearGradient id={`key-${uid}`} x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stopColor={f.hi} />
      <stop offset="0.45" stopColor={f.side} />
      <stop offset="1" stopColor={f.mid} />
    </linearGradient>
  );
  if (p.kind === "hold") {
    const sw = Math.min(d * 0.4, len);
    const knob = sw * 0.86;
    return (
      <g>
        <rect x={cx - sw / 2} y={p.y0} width={sw} height={len} rx={sw / 2} className="r3d-slot" />
        <defs>{fill}</defs>
        <rect x={cx - knob / 2} y={p.y0 + (sw - knob) / 2} width={knob} height={Math.min(knob * 1.25, len - (sw - knob))} rx={knob / 2} fill={`url(#key-${uid})`} stroke="#000" strokeOpacity={0.4} strokeWidth={0.14} />
      </g>
    );
  }
  const bw = p.kind === "rocker" ? d * 0.4 : Math.min(len, d * share);
  return (
    <g>
      <defs>{fill}</defs>
      <rect x={cx - bw / 2} y={p.y0} width={bw} height={len} rx={bw / 2} fill={`url(#key-${uid})`} stroke="#000" strokeOpacity={0.6} strokeWidth={0.22} />
      <rect x={cx - bw / 2 + 0.3} y={p.y0 + 0.3} width={bw - 0.6} height={len - 0.6} rx={bw / 2 - 0.3} fill="none" stroke="#fff" strokeOpacity={f.edge * 0.5} strokeWidth={0.12} />
      {p.kind === "rocker" ? (
        <>
          <g transform={`translate(${cx} ${p.y0 + bw * 0.75})`}>
            <Engraving part="volume" plus s={bw * 0.62} />
          </g>
          <g transform={`translate(${cx} ${p.y1 - bw * 0.75})`}>
            <Engraving part="volume" plus={false} s={bw * 0.62} />
          </g>
        </>
      ) : (
        <g transform={`translate(${cx} ${cy})`}>
          <Engraving part={p.part} plus={!!p.dot} s={bw * 0.5} />
        </g>
      )}
      {p.dot && <circle cx={cx + bw * 0.3} cy={p.kind === "rocker" ? p.y0 + bw * 0.75 : cy - bw * 0.3} r={0.32} className="r3d-dot" />}
    </g>
  );
}

/** `bleed` (mm) runs on under the front and `seam` (mm) under the corners, which are drawn over it. */
function Side({ spec, f, cue, bleed, seam }: { spec: BodySpec; f: (typeof FINISHES)[Finish]; cue: CueKey[]; bleed: number; seam: Ends }) {
  const uid = useId().replace(/:/g, "");
  const { d, h } = spec;
  // Only the flat run between the rounded corners; the corners are drawn as strips.
  const t = spec.radius.top - seam.top;
  const run = h - spec.radius.top - spec.radius.bottom + seam.top + seam.bottom;
  return (
    <svg className="r3d-face r3d-right" viewBox={`${-bleed} ${t} ${d + bleed} ${run}`} preserveAspectRatio="none" aria-hidden="true">
      <defs>
        <linearGradient id={`side-${uid}`} gradientUnits="userSpaceOnUse" x1="0" y1="0" x2={d} y2="0">
          {SIDE_SHADE.map(([at, tone]) => (
            <stop key={at} offset={at} stopColor={f[tone]} />
          ))}
        </linearGradient>
      </defs>
      <rect x={-bleed} y={t} width={d + bleed} height={run} fill={`url(#side-${uid})`} />
      <rect y={t} width={0.25} height={run} fill="#fff" fillOpacity={f.edge} />
      {spec.right.map((p) => (
        <SideControl key={p.y0} p={p} d={d} share={spec.key} f={f} />
      ))}
      {cue.map((c) => {
        const at = cueSpan(spec, c);
        if (!at) return null;
        const rw = at.bw + 1.4;
        return <rect key={c} x={d / 2 - rw / 2} y={at.y0 - 0.7} width={rw} height={at.y1 - at.y0 + 1.4} rx={rw / 2} className="r3d-cue" />;
      })}
    </svg>
  );
}

function Top({ spec, f, live, bleed, seam }: { spec: BodySpec; f: (typeof FINISHES)[Finish]; live: boolean; bleed: number; seam: Ends }) {
  const uid = useId().replace(/:/g, "");
  const { w, d } = spec;
  const r = spec.radius.top - seam.top;
  const ring = spec.jackRing;
  return (
    <svg className="r3d-face r3d-top" viewBox={`${r} 0 ${w - 2 * r} ${d + bleed}`} preserveAspectRatio="none" aria-hidden="true">
      <defs>
        <linearGradient id={`top-${uid}`} gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="0" y2={d}>
          <stop offset="0" stopColor={f.mid} />
          <stop offset="0.5" stopColor={f.hi} />
          <stop offset="1" stopColor={f.mid} />
        </linearGradient>
        {ring && (
          <radialGradient id={`ring-${uid}`} cx="0.4" cy="0.35" r="0.75">
            <stop offset="0" stopColor="#f3e2b8" />
            <stop offset="0.45" stopColor={ring} />
            <stop offset="1" stopColor="#6e5630" />
          </radialGradient>
        )}
      </defs>
      <rect x={r} width={w - 2 * r} height={d + bleed} fill={`url(#top-${uid})`} />
      <rect x={r} y={d - 0.45} width={w - 2 * r} height={0.45} fill="#fff" fillOpacity={f.edge * 1.6} />
      {spec.edge === "top" &&
        spec.edgeFeatures.map((e, i) => {
          const big = e.kind === "jack-44";
          const outer = big ? 4.3 : 3.6;
          const hole = big ? 2.25 : 1.8;
          return (
            <g key={e.x}>
              <circle cx={e.x} cy={d / 2} r={outer} fill={ring ? `url(#ring-${uid})` : f.lo} stroke="#000" strokeOpacity={0.45} strokeWidth={0.18} />
              <circle cx={e.x} cy={d / 2} r={hole + 0.55} className={live && i === 0 ? "r3d-jack is-live" : "r3d-jack"} />
              <circle cx={e.x} cy={d / 2} r={hole} fill="#030304" />
            </g>
          );
        })}
    </svg>
  );
}

/**
 * How far, in pixels, the side, top and corner faces run on under the front. Faces that only
 * meet leave an antialiased seam the page shows through.
 */
const BLEED = 3;
/**
 * How far, in pixels, the side and top faces run on under the corner strips at their ends; at
 * most half the corner's radius, or the run-on stands out past a tight corner.
 */
const SEAM = 3;

/** The turned views the faces are drawn for: the default and the one a player card leans to on hover. */
const VIEWS = [
  { rx: -7, ry: -26 },
  { rx: -9, ry: -34 },
];
/** `.r3d`'s perspective in render.css. */
const PERSPECTIVE = 2200;
/** Strips to the largest corner; smaller ones get fewer. */
const STEPS = 16;
const rad = (deg: number) => (deg * Math.PI) / 180;

/**
 * Where a point of the body lands in the default turned view, in pixels from the render's
 * top left: the faces' transform worked by hand. x, y, z are millimetres from the body's
 * centre, z towards the viewer.
 */
export function turnedPoint(spec: BodySpec, scale: number, x: number, y: number, z: number): { x: number; y: number } {
  const { rx, ry } = VIEWS[0]!;
  const [px, py, pz] = [x * scale, y * scale, z * scale];
  const x1 = px * Math.cos(rad(ry)) + pz * Math.sin(rad(ry));
  const z1 = -px * Math.sin(rad(ry)) + pz * Math.cos(rad(ry));
  const y2 = py * Math.cos(rad(rx)) - z1 * Math.sin(rad(rx));
  const z2 = py * Math.sin(rad(rx)) + z1 * Math.cos(rad(rx));
  const k = 1 / (1 - z2 / PERSPECTIVE);
  return { x: (spec.w * scale) / 2 + x1 * k, y: (spec.h * scale) / 2 + y2 * k };
}

/** How directly a surface with this outward normal angle faces the viewer; above zero it shows. */
function facing(phi: number, v: { rx: number; ry: number }): number {
  return -Math.sin(rad(phi)) * Math.sin(rad(v.rx)) - Math.cos(rad(phi)) * Math.sin(rad(v.ry)) * Math.cos(rad(v.rx));
}

/**
 * The rounded corners between the sides as narrow flat strips, each turned to its own
 * normal. φ is the normal's angle from the right side towards the top. Strips that never
 * face the viewer are left out: without preserve-3d nothing hides them. x, y (from the
 * body's centre) and len are in millimetres.
 */
function strips(spec: BodySpec) {
  const { top, bottom } = spec.radius;
  return (
    [
      ["tr", 1, -1, 0, top],
      ["br", 1, 1, -90, bottom],
      ["tl", -1, -1, 90, top],
    ] as const
  )
    .flatMap(([name, sx, sy, from, r]) => {
      const n = Math.max(2, Math.min(STEPS, Math.round(r * 3.5)));
      const step = 90 / n;
      return Array.from({ length: n }, (_, i) => {
        const phi = from + step * (i + 0.5);
        // Lit from above: the right side reads at 0.82, the top at 1.12.
        const light = Math.max(0.4, Math.min(1.12, 0.82 + 0.3 * Math.sin(rad(phi))));
        const x = sx * (spec.w / 2 - r) + r * Math.cos(rad(phi));
        const y = sy * (spec.h / 2 - r) - r * Math.sin(rad(phi));
        return { key: `${name}${i}`, x, y, phi, light, len: 2 * r * Math.sin(rad(step / 2)) * 1.08 };
      });
    })
    .filter((s) => VIEWS.some((v) => facing(s.phi, v) > 0.02));
}

const STRIPS = Object.fromEntries(Object.entries(BODIES).map(([art, spec]) => [art, strips(spec)]));

/**
 * The player as an object: its front to scale with the live screen, and when turned,
 * its right side with the controls and its top with any jacks there.
 */
export function DeviceRender({ art, scale, finish, screen, battery = null, charging = false, turned = true, live = false, cue = [], lang = "en", className, style }: Props) {
  const spec = BODIES[art];
  if (!spec) return null;
  const f = FINISHES[finish ?? (art === "zx500" ? "black" : "graphite")];
  const seam = { top: Math.min(SEAM / scale, spec.radius.top / 2), bottom: Math.min(SEAM / scale, spec.radius.bottom / 2) };
  const vars = {
    "--w": `${spec.w * scale}px`,
    "--h": `${spec.h * scale}px`,
    "--d": `${spec.d * scale}px`,
    "--r-top": `${spec.radius.top * scale}px`,
    "--r-bottom": `${spec.radius.bottom * scale}px`,
    "--bleed": `${BLEED}px`,
    "--seam-top": `${seam.top * scale}px`,
    "--seam-bottom": `${seam.bottom * scale}px`,
    "--f-lo": f.lo,
    "--f-mid": f.mid,
    "--f-side": f.side,
    "--f-hi": f.hi,
    ...style,
  } as CSSProperties;
  return (
    <div className={`r3d${turned ? " is-turned" : ""} ${className ?? ""}`} style={vars}>
      <div className="r3d-obj">
        {turned && <Side spec={spec} f={f} cue={cue} bleed={BLEED / scale} seam={seam} />}
        {turned && <Top spec={spec} f={f} live={live} bleed={BLEED / scale} seam={seam} />}
        {turned &&
          STRIPS[art]!.map((s) => (
            <span
              key={s.key}
              className="r3d-face r3d-round"
              style={{ "--x": `${s.x * scale}px`, "--y": `${s.y * scale}px`, "--c": `${s.len * scale}px`, "--phi": `${-s.phi}deg`, "--light": s.light } as CSSProperties}
            />
          ))}
        <Front spec={spec} f={f} screen={screen} battery={battery} charging={charging} lang={lang} />
      </div>
      <div className="r3d-shadow" />
    </div>
  );
}
