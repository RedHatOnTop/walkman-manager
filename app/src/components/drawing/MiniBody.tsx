import { outline } from "./outline";
import { BODIES } from "./specs";

/** A player's front outline to scale, for lists where the full drawing would not fit. */
export function MiniBody({ art, scale = 0.62, awake = false }: { art: string; scale?: number; awake?: boolean }) {
  const spec = BODIES[art];
  if (!spec) return null;
  const s = scale;
  const d = spec.display;
  const w = spec.w * s + 2;
  const h = spec.h * s + 2;
  return (
    <svg className={`mini-body${awake ? " is-awake" : ""}`} width={w} height={h} viewBox={`0 0 ${w} ${h}`} aria-hidden="true">
      <path className="mini-shell" d={outline(1, 1, spec.w * s, spec.h * s, spec.radius.top * s, spec.radius.bottom * s)} />
      <rect className="mini-screen" x={1 + d.x * s} y={1 + d.y * s} width={d.w * s} height={d.h * s} rx={Math.max(0.6, d.r * s)} />
    </svg>
  );
}
