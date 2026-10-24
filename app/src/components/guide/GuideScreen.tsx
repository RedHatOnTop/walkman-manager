import { useEffect, useState, type ReactElement } from "react";
import { dictionaries, type Lang } from "../../i18n/strings";

/**
 * What the player's own screen shows at a step of an install, drawn as the owner will see
 * it. Settings wording is the stock 4.06 Settings app's own, in both languages.
 */
export type GuideScene =
  | "about"
  | "aboutKit"
  | "usbDebugging"
  | "oemUnlock"
  | "allow"
  | "setupLanguage"
  | "setupDebug"
  | "logo"
  | "boot"
  | "settings"
  | "recovery"
  | "home"
  | "charge"
  // An NW-A50 series player under Walkman OS, and the kit's picker.
  | "usbDrive"
  | "updating"
  | "picker";

/** Steps through `count` frames, `ms` each, and starts over; holds the last when motion is reduced. */
/**
 * Frames and frame length of the scenes whose keys the stage points at, so that its arrows
 * keep time with the screen.
 */
export const BEATS = {
  recovery: [6, 800],
  picker: [4, 1300],
} as const;

export function useFrames(count: number, ms: number, key: unknown): number {
  const reduced = typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
  const [frame, setFrame] = useState(reduced ? count - 1 : 0);
  useEffect(() => {
    if (reduced) {
      setFrame(count - 1);
      return;
    }
    setFrame(0);
    const t = window.setInterval(() => setFrame((f) => (f + 1) % count), ms);
    return () => window.clearInterval(t);
  }, [count, ms, reduced, key]);
  return frame;
}

interface Props {
  scene: GuideScene;
  w: number;
  h: number;
  lang: Lang;
}

export function GuideScreen({ scene, w, h, lang }: Props) {
  const d = dictionaries[lang];
  const u = w / 100;
  const s = (key: keyof typeof d) => d[key];

  const bar = (title: string) => (
    <g>
      <text x={5 * u} y={7 * u} fontSize={3.4 * u} className="scr-dim">
        12:00
      </text>
      <path d={`M${6 * u} ${19 * u} l${2.6 * u} ${-2.6 * u} M${6 * u} ${19 * u} l${2.6 * u} ${2.6 * u} M${6 * u} ${19 * u} h${6 * u}`} className="gd-stroke" strokeWidth={0.6 * u} />
      <text x={17 * u} y={20.4 * u} fontSize={5.4 * u} className="scr-text scr-bold">
        {title}
      </text>
    </g>
  );

  const row = (y: number, title: string, sub: string | null, opts: { lit?: boolean; toggle?: boolean | null; tap?: boolean } = {}) => (
    <g key={`${title}-${y}`}>
      {opts.lit && <rect x={0} y={y - 6.5 * u} width={w} height={sub ? 15 * u : 12 * u} className="gd-lit" />}
      <text x={6 * u} y={y} fontSize={4.4 * u} className="scr-text">
        {title}
      </text>
      {sub && (
        <text x={6 * u} y={y + 5.4 * u} fontSize={3.3 * u} className="scr-dim">
          {sub}
        </text>
      )}
      {opts.toggle != null && (
        <g transform={`translate(${w - 18 * u} ${y - 3.6 * u})`}>
          <rect width={11 * u} height={5.2 * u} rx={2.6 * u} className={opts.toggle ? "gd-track is-on" : "gd-track"} />
          <circle cx={opts.toggle ? 8.4 * u : 2.6 * u} cy={2.6 * u} r={3.2 * u} className={opts.toggle ? "gd-knob is-on" : "gd-knob"} />
        </g>
      )}
      {opts.tap && <Tap cx={opts.toggle != null ? w - 12.5 * u : w * 0.42} cy={y - 1 * u} u={u} />}
    </g>
  );

  const dialog = (title: string, body: string, ok: string, tapOk: boolean) => {
    const cw = 84 * u;
    const cx = (w - cw) / 2;
    const cy = h * 0.34;
    return (
      <g className="gd-pop">
        <rect width={w} height={h} className="gd-scrim" />
        <rect x={cx} y={cy} width={cw} height={52 * u} rx={4 * u} className="scr-card" />
        <text x={cx + 6 * u} y={cy + 11 * u} fontSize={4.6 * u} className="scr-text scr-bold">
          {title}
        </text>
        <foreignObject x={cx + 6 * u} y={cy + 15 * u} width={cw - 12 * u} height={22 * u}>
          <p className="gd-body" style={{ fontSize: `${3.2 * u}px` }}>
            {body}
          </p>
        </foreignObject>
        <text x={cx + cw - 36 * u} y={cy + 46 * u} fontSize={3.8 * u} className="scr-dim scr-bold">
          {s("guide.screen.cancel")}
        </text>
        <text x={cx + cw - 15 * u} y={cy + 46 * u} fontSize={3.8 * u} textAnchor="middle" className="scr-accent scr-bold">
          {ok}
        </text>
        {tapOk && <Tap cx={cx + cw - 15 * u} cy={cy + 44.6 * u} u={u} />}
      </g>
    );
  };

  if (scene === "about" || scene === "aboutKit") return <About kit={scene === "aboutKit"} w={w} h={h} u={u} lang={lang} bar={bar} row={row} />;

  if (scene === "usbDebugging" || scene === "oemUnlock") {
    return <Developer which={scene} w={w} h={h} u={u} lang={lang} bar={bar} row={row} dialog={dialog} />;
  }

  if (scene === "allow") return <Allow w={w} h={h} u={u} lang={lang} />;

  if (scene === "setupLanguage" || scene === "setupDebug") return <Setup which={scene} w={w} h={h} u={u} lang={lang} />;

  if (scene === "logo") {
    return (
      <g>
        <rect width={w} height={h} fill="#000" />
        <rect x={w / 2 - 15 * u} y={h / 2 - 2.4 * u} width={30 * u} height={4.8 * u} rx={0.8 * u} className="gd-logo" />
      </g>
    );
  }

  if (scene === "boot") {
    return (
      <g>
        <rect width={w} height={h} fill="#000" />
        <rect x={w / 2 - 20 * u} y={h / 2 - 2.6 * u} width={40 * u} height={2.6 * u} rx={1.3 * u} className="gd-boot" />
        <rect x={w * 0.3} y={h / 2 + 8 * u} width={w * 0.4} height={0.5 * u} className="gd-boot-line" />
        <rect x={w * 0.3} y={h / 2 + 8 * u} width={w * 0.12} height={0.5 * u} className="gd-boot-sweep" />
      </g>
    );
  }

  if (scene === "settings") return <OpenSettings w={w} h={h} u={u} />;

  if (scene === "recovery") return <Recovery w={w} h={h} u={u} />;

  if (scene === "charge") {
    const r = 18 * u;
    const c = 2 * Math.PI * r;
    return (
      <g>
        <rect width={w} height={h} fill="#000" />
        <circle cx={w / 2} cy={h * 0.45} r={r} className="gd-ring-track" strokeWidth={1.4 * u} />
        <circle
          cx={w / 2}
          cy={h * 0.45}
          r={r}
          className="gd-ring-fill"
          strokeWidth={1.4 * u}
          strokeDasharray={`${c} ${c}`}
          style={{ strokeDashoffset: c * 0.5 }}
          transform={`rotate(-90 ${w / 2} ${h * 0.45})`}
        />
        <text x={w / 2} y={h * 0.45 + 3.4 * u} textAnchor="middle" fontSize={10 * u} className="scr-big">
          50%
        </text>
      </g>
    );
  }

  if (scene === "usbDrive") {
    const c = h * 0.42;
    return (
      <g>
        <rect width={w} height={h} fill="#000" />
        <g transform={`translate(${w / 2} ${c})`} className="gd-stroke" strokeWidth={1.1 * u}>
          <path d={`M0 ${11 * u} V${-11 * u} M${-3 * u} ${-7.5 * u} L0 ${-11 * u} L${3 * u} ${-7.5 * u}`} />
          <path d={`M0 ${4 * u} L${-7 * u} ${-1 * u} V${-5 * u} M0 ${7 * u} L${7 * u} ${2 * u} V${-2 * u}`} />
          <circle cx={-7 * u} cy={-6.6 * u} r={1.6 * u} />
          <rect x={5.4 * u} y={-5.2 * u} width={3.2 * u} height={3.2 * u} />
          <circle cy={13 * u} r={2.2 * u} />
        </g>
        <text x={w / 2} y={c + 30 * u} textAnchor="middle" fontSize={4.6 * u} className="scr-text">
          {s("guide.screen.usbDrive")}
        </text>
      </g>
    );
  }

  if (scene === "updating") {
    return (
      <g>
        <rect width={w} height={h} fill="#000" />
        <text x={w / 2} y={h / 2 - 4 * u} textAnchor="middle" fontSize={4.8 * u} className="scr-text">
          {s("guide.screen.updating")}
        </text>
        <rect x={w * 0.2} y={h / 2 + 4 * u} width={w * 0.6} height={0.6 * u} className="gd-boot-line" />
        <rect x={w * 0.2} y={h / 2 + 4 * u} width={w * 0.16} height={0.6 * u} className="gd-boot-sweep" />
      </g>
    );
  }

  if (scene === "picker") return <Picker w={w} h={h} u={u} />;

  // home: the installed system's lock screen, with the read-back's mark.
  return (
    <g>
      <rect width={w} height={h} className="gd-home" />
      <text x={w / 2} y={h * 0.32} textAnchor="middle" fontSize={22 * u} className="scr-clock">
        12:00
      </text>
      <g transform={`translate(${w / 2} ${h * 0.58})`} className="gd-pop">
        <circle r={11 * u} className="gd-check-disc" />
        <path d={`M${-4.6 * u} 0 l${3.2 * u} ${3.4 * u} l${6.4 * u} ${-7 * u}`} className="gd-check" strokeWidth={1.3 * u} />
      </g>
    </g>
  );
}

/**
 * The kit's picker as bootpick shows it: Walkman, then a volume key moves to Android and play
 * starts it. Its words are the picker's own, English on the player.
 */
function Picker({ w, h, u }: { w: number; h: number; u: number }) {
  const frame = useFrames(...BEATS.picker, "picker");
  const on = frame === 0 ? 0 : 1;
  const starting = frame === 3;
  const options: [string, string][] = [
    ["Walkman", "Sony firmware"],
    ["Android", "Android 5.1"],
  ];
  return (
    <g>
      <rect width={w} height={h} fill="#000" />
      <text x={4.2 * u} y={11 * u} fontSize={5.4 * u} className="scr-text gd-pick-light">
        Select System
      </text>
      <rect x={0} y={18.3 * u} width={w} height={0.3 * u} className="gd-pick-rule" />
      {options.map(([title, sub], i) => {
        const y = 18.7 * u + i * 17.7 * u;
        return (
          <g key={title}>
            {i === on && <rect x={0} y={y} width={w} height={17.5 * u} className="gd-pick-row" />}
            <text x={4.2 * u} y={y + 7.6 * u} fontSize={5.4 * u} className={`${starting && i !== on ? "gd-pick-off" : "scr-text"} gd-pick-light`}>
              {title}
            </text>
            <text x={4.2 * u} y={y + 14 * u} fontSize={4.6 * u} className="scr-dim gd-pick-light">
              {sub}
            </text>
            <circle cx={90.8 * u} cy={y + 8.8 * u} r={2.7 * u} className="gd-pick-radio" strokeWidth={0.4 * u} />
            {i === on && <circle cx={90.8 * u} cy={y + 8.8 * u} r={1.5 * u} className="gd-pick-dot" />}
          </g>
        );
      })}
      {starting ? (
        <text x={4.2 * u} y={134 * u} fontSize={4.6 * u} className="scr-dim gd-pick-light">
          Starting Android...
        </text>
      ) : (
        <g>
          <text x={4.2 * u} y={133 * u} fontSize={4.6 * u} className="scr-dim gd-pick-light">
            Starts automatically
          </text>
          <rect x={4.2 * u} y={137.3 * u} width={w - 8.4 * u} height={0.3 * u} className="gd-pick-rule" />
          <text x={4.2 * u} y={146.7 * u} fontSize={4.6 * u} className="scr-text gd-pick-light">
            VOL + / −
          </text>
          <text x={50 * u} y={146.7 * u} fontSize={4.6 * u} className="scr-dim gd-pick-light">
            Move
          </text>
          <text x={4.2 * u} y={154 * u} fontSize={4.6 * u} className="scr-text gd-pick-light">
            ▶II / POWER
          </text>
          <text x={50 * u} y={154 * u} fontSize={4.6 * u} className="scr-dim gd-pick-light">
            Start
          </text>
        </g>
      )}
    </g>
  );
}

function Tap({ cx, cy, u }: { cx: number; cy: number; u: number }) {
  return (
    <g className="gd-tap" transform={`translate(${cx} ${cy})`}>
      <circle r={4.2 * u} className="gd-tap-ring" />
      <circle r={2.2 * u} className="gd-tap-dot" />
    </g>
  );
}

type Row = (y: number, title: string, sub: string | null, opts?: { lit?: boolean; toggle?: boolean | null; tap?: boolean }) => ReactElement;

/**
 * About device: seven taps on Build number. Stock 4.06 keeps it under Settings › System; the
 * kit's Android 13 lists it on Settings' first page.
 */
function About({ kit, w, h, u, lang, bar, row }: { kit: boolean; w: number; h: number; u: number; lang: Lang; bar(t: string): ReactElement; row: Row }) {
  const d = dictionaries[lang];
  const frame = useFrames(11, 520, "about");
  const taps = Math.min(frame, 7);
  const left = 7 - taps;
  return (
    <g>
      <rect width={w} height={h} className="gd-bg" />
      {bar(d["guide.screen.about"])}
      {row(38 * u, d["guide.screen.model"], kit ? "NW-ZX500Series" : "NW-ZX507")}
      {row(55 * u, d["guide.screen.android"], kit ? "13" : "9")}
      <g key={`tap${frame}`}>{row(72 * u, d["guide.screen.build"], kit ? "TQ3C.230805.001.A4" : "4.06.00", { lit: true, tap: frame > 0 && frame <= 7 })}</g>
      {taps >= 3 && (
        <g transform={`translate(${w / 2} ${h - 22 * u})`} className="gd-pop" key={`toast${taps >= 7}`}>
          <rect x={-46 * u} y={-6 * u} width={92 * u} height={10 * u} rx={5 * u} className="gd-toast" />
          <text textAnchor="middle" y={0.6 * u} fontSize={3 * u} className="scr-text">
            {left > 0 ? d["guide.screen.steps"].replace("{n}", String(left)) : d["guide.screen.developer"]}
          </text>
        </g>
      )}
    </g>
  );
}

/** Settings › System › Developer options: a switch, its confirmation, the switch on. */
function Developer({
  which,
  w,
  h,
  u,
  lang,
  bar,
  row,
  dialog,
}: {
  which: "usbDebugging" | "oemUnlock";
  w: number;
  h: number;
  u: number;
  lang: Lang;
  bar(t: string): ReactElement;
  row: Row;
  dialog(title: string, body: string, ok: string, tapOk: boolean): ReactElement;
}) {
  const d = dictionaries[lang];
  const frame = useFrames(9, 700, which);
  const usb = which === "usbDebugging";
  const on = frame >= 6;
  const asking = frame >= 3 && frame < 6;
  return (
    <g>
      <rect width={w} height={h} className="gd-bg" />
      {bar(d["guide.screen.developer.title"])}
      {row(38 * u, d["guide.screen.oem"], d["guide.screen.oem.sub"], {
        lit: !usb,
        toggle: !usb && on,
        tap: !usb && frame === 2,
      })}
      <text x={6 * u} y={58 * u} fontSize={3.2 * u} className="scr-accent scr-bold">
        {d["guide.screen.debugging"]}
      </text>
      {row(68 * u, d["guide.screen.usb"], d["guide.screen.usb.sub"], {
        lit: usb,
        toggle: usb && on,
        tap: usb && frame === 2,
      })}
      {asking &&
        (usb
          ? dialog(d["guide.screen.usb.ask"], d["guide.screen.usb.body"], d["guide.screen.ok"], frame === 5)
          : dialog(d["guide.screen.oem.ask"], d["guide.screen.oem.body"], d["guide.screen.enable"], frame === 5))}
    </g>
  );
}

/** The prompt Android shows the first time a computer asks for adb. */
function Allow({ w, h, u, lang }: { w: number; h: number; u: number; lang: Lang }) {
  const d = dictionaries[lang];
  const frame = useFrames(7, 650, "allow");
  const cw = 84 * u;
  const cx = (w - cw) / 2;
  const cy = h * 0.32;
  const ticked = frame >= 2;
  return (
    <g>
      <rect width={w} height={h} className="gd-bg" />
      <rect width={w} height={h} className="scr-dimmed" />
      <rect x={cx} y={cy} width={cw} height={58 * u} rx={5 * u} className="scr-card" />
      <text x={cx + 6 * u} y={cy + 12 * u} fontSize={5.2 * u} className="scr-text scr-bold">
        {d["render.rsa.title"]}
      </text>
      <text x={cx + 6 * u} y={cy + 20 * u} fontSize={3.6 * u} className="scr-dim">
        {d["render.rsa.key"]}
      </text>
      <rect x={cx + 6 * u} y={cy + 27 * u} width={4 * u} height={4 * u} rx={0.8 * u} className={ticked ? "gd-box is-on" : "gd-box"} />
      {ticked && <path d={`M${cx + 6.8 * u} ${cy + 29 * u} l${1.1 * u} ${1.1 * u} l${1.9 * u} ${-2.1 * u}`} className="gd-check" strokeWidth={0.6 * u} />}
      <text x={cx + 12.5 * u} y={cy + 30.4 * u} fontSize={3.6 * u} className="scr-text">
        {d["render.rsa.always"]}
      </text>
      {frame === 1 && <Tap cx={cx + 8 * u} cy={cy + 29 * u} u={u} />}
      <rect x={cx + cw - 30 * u} y={cy + 41 * u} width={24 * u} height={10 * u} rx={5 * u} className="scr-allow" />
      <text x={cx + cw - 18 * u} y={cy + 47.3 * u} textAnchor="middle" fontSize={4 * u} className="scr-on-accent scr-bold">
        {d["render.rsa.allow"]}
      </text>
      {frame === 4 && <Tap cx={cx + cw - 18 * u} cy={cy + 46 * u} u={u} />}
    </g>
  );
}

/**
 * The kit's first-run setup (walkman-zx507 apps/setup) in its own black and gold: the step
 * marks, the question, and the gold Next. The language page picks the app's own language;
 * the last question turns USB debugging on.
 */
function Setup({ which, w, h, u, lang }: { which: "setupLanguage" | "setupDebug"; w: number; h: number; u: number; lang: Lang }) {
  const d = dictionaries[lang];
  const language = which === "setupLanguage";
  const frame = useFrames(language ? 8 : 9, language ? 650 : 700, which);
  const step = language ? 0 : 3;
  const next = { x: 61 * u, y: 144 * u, w: 33 * u, h: 13.6 * u };
  const tapNext = frame === (language ? 4 : 6);
  const page = language ? (
    <g>
      {[
        { y: 56, name: "English (United States)", lang: "en" },
        { y: 73, name: "한국어", lang: "ko" },
        { y: 90, name: "日本語", lang: "ja" },
        { y: 107, name: "中文 (中国)", lang: "zh" },
      ].map((r) => {
        const chosen = frame >= 2 ? r.lang === lang : r.lang === "en";
        return (
          <g key={r.name}>
            <text x={7.5 * u} y={r.y * u} fontSize={4.4 * u} className={chosen ? "gd-setup-gold" : "gd-setup-text"}>
              {r.name}
            </text>
            {chosen && <path d={`M${86 * u} ${(r.y - 1.6) * u} l${1.6 * u} ${1.6 * u} l${3 * u} ${-3.4 * u}`} className="gd-setup-check" strokeWidth={0.55 * u} />}
            {frame === 1 && r.lang === lang && <Tap cx={30 * u} cy={(r.y - 1.4) * u} u={u} />}
          </g>
        );
      })}
    </g>
  ) : (
    <g>
      <foreignObject x={7.5 * u} y={37 * u} width={85 * u} height={20 * u}>
        <p className="gd-body gd-setup-body" style={{ fontSize: `${3.3 * u}px` }}>
          {d["guide.screen.setup.debug.sub"]}
        </p>
      </foreignObject>
      <rect x={7.5 * u} y={64 * u} width={85 * u} height={17 * u} rx={4 * u} className="gd-setup-card" />
      <text x={12 * u} y={74 * u} fontSize={4.4 * u} className="gd-setup-text">
        {d["guide.screen.usb"]}
      </text>
      <g transform={`translate(${76 * u} ${69.9 * u})`}>
        <rect width={11 * u} height={5.2 * u} rx={2.6 * u} className={frame >= 3 ? "gd-setup-track is-on" : "gd-setup-track"} />
        <circle cx={frame >= 3 ? 8.4 * u : 2.6 * u} cy={2.6 * u} r={3.2 * u} className={frame >= 3 ? "gd-setup-knob is-on" : "gd-setup-knob"} />
      </g>
      {frame === 2 && <Tap cx={81.5 * u} cy={72.5 * u} u={u} />}
    </g>
  );
  return (
    <g>
      <rect width={w} height={h} fill="#000" />
      <text x={5 * u} y={7 * u} fontSize={3.4 * u} className="gd-setup-text">
        12:00
      </text>
      {[0, 1, 2, 3].map((i) => (
        <rect key={i} x={(7.5 + i * 21.6) * u} y={14.6 * u} width={19.6 * u} height={0.9 * u} rx={0.45 * u} className={i <= step ? "gd-setup-mark is-on" : "gd-setup-mark"} />
      ))}
      <text x={7.5 * u} y={30 * u} fontSize={7 * u} className="gd-setup-text gd-setup-title">
        {d[language ? "guide.screen.setup.language" : "guide.screen.setup.debug"]}
      </text>
      {page}
      {!language && (
        <text x={9 * u} y={152.6 * u} fontSize={4.2 * u} className="gd-setup-dim">
          {d["guide.screen.setup.back"]}
        </text>
      )}
      <rect x={next.x} y={next.y} width={next.w} height={next.h} rx={next.h / 2} className="gd-setup-gold" />
      <text x={next.x + next.w / 2} y={next.y + next.h / 2 + 1.5 * u} textAnchor="middle" fontSize={4.2 * u} className="gd-setup-on-gold">
        {d["guide.screen.setup.next"]}
      </text>
      {tapNext && <Tap cx={next.x + next.w / 2} cy={next.y + next.h / 2} u={u} />}
    </g>
  );
}

/** Settings from the installed system's home screen: the quick settings pulled down from the top, whose gear opens Settings. */
function OpenSettings({ w, h, u }: { w: number; h: number; u: number }) {
  const frame = useFrames(7, 750, "settings");
  const pulled = frame >= 3;
  const swipe = frame === 1 || frame === 2;
  const tiles = [0, 1, 2, 3, 4];
  return (
    <g>
      <rect width={w} height={h} className="gd-home" />
      <rect x={5 * u} y={11 * u} width={w - 10 * u} height={9 * u} rx={2.4 * u} className="gd-search" />
      {Array.from({ length: 34 }, (_, i) => {
        const x = 1.5 * u + i * (w - 3 * u) / 33;
        const len = (14 + 10 * Math.sin(i * 0.55) + 6 * Math.cos(i * 1.7)) * u;
        return <rect key={i} x={x} y={h * 0.53 - len / 2} width={0.35 * u} height={len} className="gd-wave" />;
      })}
      <circle cx={w * 0.38} cy={h - 13.5 * u} r={6.6 * u} className="gd-dock-music" />
      <circle cx={w * 0.62} cy={h - 13.5 * u} r={6.6 * u} className="gd-dock-app" />
      {swipe && (
        <g className="gd-swipe" key="swipe">
          <path d={`M${w / 2} ${4 * u} v${18 * u}`} className="gd-swipe-trail" strokeWidth={2.4 * u} />
          <circle cx={w / 2} cy={22 * u} r={2.4 * u} className="gd-tap-dot" />
        </g>
      )}
      {pulled && (
        <g className="gd-drop">
          <rect width={w} height={h} className="gd-shade" />
          <text x={5 * u} y={18 * u} fontSize={9 * u} className="scr-clock">
            12:00
          </text>
          <rect x={5 * u} y={26 * u} width={w - 12 * u} height={9 * u} rx={4.5 * u} className="gd-qs-on" />
          {tiles.map((i) => (
            <rect
              key={i}
              x={i % 2 ? w / 2 + 1 * u : 5 * u}
              y={39 * u + Math.floor(i / 2) * 15 * u}
              width={w / 2 - 6 * u}
              height={12.5 * u}
              rx={6 * u}
              className={i === 0 || i === 3 ? "gd-qs-on" : "gd-qs"}
            />
          ))}
          <circle cx={w - 25 * u} cy={h - 23 * u} r={6.4 * u} className="gd-qs" />
          <g transform={`translate(${w - 25 * u} ${h - 23 * u})`}>
            <circle r={2.6 * u} className="gd-gear" strokeWidth={0.7 * u} strokeDasharray={`${0.9 * u} ${0.55 * u}`} />
            <circle r={1 * u} className="gd-gear" strokeWidth={0.6 * u} />
          </g>
          <circle cx={w - 10 * u} cy={h - 23 * u} r={6.4 * u} className="gd-qs-on" />
          {frame >= 5 && <Tap cx={w - 25 * u} cy={h - 23 * u} u={u} />}
        </g>
      )}
    </g>
  );
}

/**
 * Stock recovery after the bootloader gave up on slot b: the volume keys move the
 * highlight, the power key picks. Recovery draws its own English text.
 */
function Recovery({ w, h, u }: { w: number; h: number; u: number }) {
  const frame = useFrames(...BEATS.recovery, "recovery");
  const second = frame >= 2;
  return (
    <g>
      <rect width={w} height={h} fill="#000" />
      <text x={6 * u} y={22 * u} fontSize={4.4 * u} className="scr-text scr-bold">
        Can&apos;t load Android system.
      </text>
      <text x={6 * u} y={29 * u} fontSize={3.2 * u} className="scr-dim">
        Your data may be corrupt.
      </text>
      <rect x={4 * u} y={44 * u} width={w - 8 * u} height={9 * u} className={second ? "gd-rec" : "gd-rec is-on"} />
      <text x={7 * u} y={49.8 * u} fontSize={3.8 * u} className="scr-text">
        Try again
      </text>
      <rect x={4 * u} y={55 * u} width={w - 8 * u} height={9 * u} className={second ? "gd-rec is-on" : "gd-rec"} />
      <text x={7 * u} y={60.8 * u} fontSize={3.8 * u} className="scr-text">
        Factory data reset
      </text>
    </g>
  );
}
