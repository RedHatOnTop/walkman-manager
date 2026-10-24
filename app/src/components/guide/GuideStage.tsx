import type { CSSProperties } from "react";
import { BODIES, type BodySpec } from "../drawing/specs";
import { DeviceRender, cueSpan, turnedPoint, type CueKey, type ScreenState } from "../render/DeviceRender";
import { BEATS, useFrames, type GuideScene } from "./GuideScreen";
import { dictionaries, type Lang, type StringKey } from "../../i18n/strings";

/**
 * A step of the install as the owner sees it on the desk: the player, what its screen
 * shows, and what their hands do. Screen steps turn the player toward the viewer like the
 * rest of the app; steps at the left edge (the USB-C port and the card tray) face it
 * square so that edge shows.
 */
export type StageScene =
  | "connect"
  | "card"
  | "charge"
  | "about"
  | "aboutKit"
  | "usbDebugging"
  | "oemUnlock"
  | "allow"
  | "setupLanguage"
  | "setupDebug"
  | "fastboot"
  | "hold"
  | "flash"
  | "boot"
  | "settings"
  | "recovery"
  | "done"
  | "idle"
  // An NW-A50 series player: the WALKMAN drive, its updater, the kit's picker, and the
  // player not yet connected under Walkman.
  | "drive"
  | "updating"
  | "picker"
  | "plug";

/** A key to press: what it is called, what it does, and the screen's frames it is pressed on. */
interface Press {
  key: CueKey;
  name: StringKey;
  does: StringKey;
  at: number[];
}

interface Look {
  screen: ScreenState;
  flat: boolean;
  cable?: "plug" | "in" | "data" | "hold";
  card?: boolean;
  /** The keys in the order they are pressed, keeping time with the screen's scene. */
  press?: { beat: keyof typeof BEATS; keys: Press[] };
}

const guide = (scene: GuideScene): ScreenState => ({ kind: "guide", scene });

const LOOKS: Record<StageScene, Look> = {
  connect: { screen: { kind: "idle" }, flat: true, cable: "plug" },
  card: { screen: { kind: "idle" }, flat: true, card: true },
  charge: { screen: guide("charge"), flat: true, cable: "in" },
  about: { screen: guide("about"), flat: false },
  aboutKit: { screen: guide("aboutKit"), flat: false },
  usbDebugging: { screen: guide("usbDebugging"), flat: false },
  oemUnlock: { screen: guide("oemUnlock"), flat: false },
  allow: { screen: guide("allow"), flat: false },
  setupLanguage: { screen: guide("setupLanguage"), flat: false },
  setupDebug: { screen: guide("setupDebug"), flat: false },
  fastboot: { screen: guide("logo"), flat: true, cable: "in" },
  hold: { screen: guide("logo"), flat: true, cable: "hold" },
  flash: { screen: guide("logo"), flat: true, cable: "data" },
  boot: { screen: guide("boot"), flat: false },
  settings: { screen: guide("settings"), flat: false },
  recovery: {
    screen: guide("recovery"),
    flat: false,
    press: {
      beat: "recovery",
      keys: [
        { key: "volume-down", name: "guide.key.volumeDown", does: "guide.press.down", at: [0, 1] },
        { key: "power", name: "guide.key.power", does: "guide.press.select", at: [3, 4] },
      ],
    },
  },
  done: { screen: guide("home"), flat: false },
  idle: { screen: { kind: "idle" }, flat: false },
  drive: { screen: guide("usbDrive"), flat: true },
  updating: { screen: guide("updating"), flat: true },
  picker: {
    screen: guide("picker"),
    flat: false,
    press: {
      beat: "picker",
      keys: [
        { key: "volume", name: "guide.key.volume", does: "guide.press.toAndroid", at: [0] },
        { key: "play", name: "guide.key.play", does: "guide.press.start", at: [2] },
      ],
    },
  },
  plug: { screen: { kind: "idle" }, flat: true },
};

/** Room left of the body for the cable and the tray, in millimetres. */
const LEFT = 62;
/** A beat for scenes with nothing to press. */
const STILL = [1, 60_000] as const;

export function GuideStage({ art, scene, scale, lang, label }: { art: string; scene: StageScene; scale: number; lang: Lang; label?: string }) {
  const spec = BODIES[art];
  const look = LOOKS[scene];
  if (!spec) return null;
  const usb = spec.left.find((p) => p.kind === "usb-c");
  const tray = spec.left.find((p) => p.kind === "card-tray");
  const d = dictionaries[lang];
  const press = look.press;
  const [count, ms] = press ? BEATS[press.beat] : STILL;
  const frame = useFrames(count, ms, scene);
  const reduced = typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
  const now = press?.keys.filter((k) => reduced || k.at.includes(frame)).map((k) => k.key) ?? [];
  const vars = { "--gw": `${spec.w * scale}px`, "--gh": `${spec.h * scale}px`, "--gs": scale } as CSSProperties;
  return (
    <div className={`gstage gstage--${scene}${look.flat ? " is-flat" : ""}${press ? " has-press" : ""}`} style={vars}>
      <div className="gstage-body">
        <DeviceRender art={art} scale={scale} screen={look.screen} turned={!look.flat} cue={now} lang={lang} battery={scene === "charge" ? 50 : null} charging={scene === "charge"} />
        {press && <Presses spec={spec} scale={scale} keys={press.keys} now={now} lang={lang} />}
        {look.flat && (usb || tray) && (
          <svg
            className="gstage-over"
            viewBox={`${-LEFT} -8 ${spec.w + LEFT + 8} ${spec.h + 16}`}
            style={{ left: `${-LEFT * scale}px`, top: `${-8 * scale}px`, width: `${(spec.w + LEFT + 8) * scale}px`, height: `${(spec.h + 16) * scale}px` }}
            aria-hidden="true"
          >
            <defs>
              <clipPath id="gstage-outside">
                <rect x={-LEFT} y={-8} width={LEFT - 0.2} height={spec.h + 16} />
              </clipPath>
            </defs>
            {usb && look.cable && <Cable y={(usb.y0 + usb.y1) / 2} h={spec.h} mode={look.cable} label={d["guide.stage.keep"]} />}
            {usb && look.cable === "plug" && <circle cx={-0.6} cy={(usb.y0 + usb.y1) / 2} r={6.5} className="gstage-cue" />}
            {usb && <rect x={-0.7} y={usb.y0 + 2} width={1} height={usb.y1 - usb.y0 - 4} rx={0.4} className="gstage-port" />}
            {tray && <Tray y0={tray.y0} y1={tray.y1} out={!!look.card} />}
          </svg>
        )}
      </div>
      {label && <p className="gstage-label">{label}</p>}
    </div>
  );
}

/**
 * Numbered labels right of the turned player, each with an arrow to its key on the side;
 * the key to press now is lit.
 */
function Presses({ spec, scale, keys, now, lang }: { spec: BodySpec; scale: number; keys: Press[]; now: CueKey[]; lang: Lang }) {
  const d = dictionaries[lang];
  const tagX = turnedPoint(spec, scale, spec.w / 2, 0, -spec.d / 2).x + 44;
  const marks = keys.flatMap((k, i) => {
    const at = cueSpan(spec, k.key);
    if (!at) return [];
    // The arrow stops just behind the key, on the side's surface.
    const tip = turnedPoint(spec, scale, spec.w / 2 + 0.6, (at.y0 + at.y1) / 2 - spec.h / 2, -at.bw / 2 - 0.4);
    return [{ ...k, n: i + 1, tip, y: tip.y, on: now.includes(k.key) }];
  });
  // Labels stand in the keys' order down the side, numbered in the order they are pressed.
  let floor = -Infinity;
  for (const m of [...marks].sort((a, b) => a.tip.y - b.tip.y)) {
    m.y = Math.max(m.tip.y, floor + 46);
    floor = m.y;
  }
  return (
    <>
      <svg className="gstage-arrows" width={tagX} height={spec.h * scale} aria-hidden="true">
        {marks.map((m) => {
          const { x, y } = m.tip;
          const from = tagX - 10;
          return (
            <g key={m.key} className={m.on ? "gstage-arrow is-on" : "gstage-arrow"}>
              <path d={`M${from} ${m.y} C${from - 22} ${m.y} ${x + 26} ${y} ${x + 6} ${y}`} className="gstage-arrow-line" />
              <path d={`M${x} ${y} l7.5 -4.2 v8.4 z`} className="gstage-arrow-head" />
            </g>
          );
        })}
      </svg>
      {marks.map((m) => (
        <div key={m.key} className={m.on ? "gstage-tag is-on" : "gstage-tag"} style={{ left: tagX, top: m.y }}>
          <span className="gstage-tag-n">{m.n}</span>
          <span className="gstage-tag-key">{d[m.name]}</span>
          <span className="gstage-tag-does">{d[m.does]}</span>
        </div>
      ))}
    </>
  );
}

/** A USB-C cable from below left into the port; `plug` slides it in, `data` runs pieces along it. */
function Cable({ y, h, mode, label }: { y: number; h: number; mode: "plug" | "in" | "data" | "hold"; label: string }) {
  const path = `M -29 ${y} C -42 ${y}, -50 ${y + 8}, -50 ${y + 30} L -50 ${h + 8}`;
  return (
    <g>
      <g className={mode === "plug" ? "gstage-plug is-moving" : "gstage-plug"}>
        <path d={path} className="gstage-cable" />
        <path d={path} className="gstage-cable-hi" />
        {mode === "data" && <path d={path} className="gstage-data" />}
        <rect x={-30} y={y - 2.3} width={9} height={4.6} rx={1.6} className="gstage-relief" />
        <rect x={-22} y={y - 3.4} width={16.4} height={6.8} rx={2.4} className="gstage-shell" />
        <rect x={-21} y={y - 2.6} width={14} height={1.2} rx={0.6} className="gstage-shell-hi" />
        <rect x={-6.2} y={y - 1.5} width={6.2} height={3} rx={1.1} className="gstage-tip" />
      </g>
      {mode === "hold" && (
        <g className="gstage-keep" transform={`translate(-34 ${y - 15})`}>
          <rect x={-22} y={-4.6} width={44} height={8.4} rx={4.2} className="gstage-keep-pill" />
          <text y={1.2} textAnchor="middle" fontSize={3.3} className="gstage-keep-text">
            {label}
          </text>
          <path d="M 10 3.8 L 12 9" className="gstage-keep-tail" />
        </g>
      )}
    </g>
  );
}

/** The microSD tray: flush, or pulled out with the card on it. */
function Tray({ y0, y1, out }: { y0: number; y1: number; out: boolean }) {
  const len = y1 - y0;
  return (
    <g>
      <g clipPath="url(#gstage-outside)">
        <g className={out ? "gstage-tray is-out" : "gstage-tray"}>
          <rect x={-16} y={y0} width={16.4} height={len} rx={1.4} className="gstage-tray-plate" />
          <path
            d={`M -13.5 ${y0 + 3} h 8.5 l 2.2 2.2 v 12 a 1 1 0 0 1 -1 1 h -9.7 a 1 1 0 0 1 -1 -1 v -13.2 a 1 1 0 0 1 1 -1 z`}
            className="gstage-card"
          />
        </g>
      </g>
      <rect x={-0.9} y={y0} width={1.3} height={len} rx={0.5} className="gstage-tray-lip" />
    </g>
  );
}
