/** The battery as the players draw it in their status bar: a cell filled to the level. */
export function Battery({ level, charging = false, size = 22 }: { level: number; charging?: boolean; size?: number }) {
  const v = Math.max(0, Math.min(100, level));
  const tone = charging ? " is-charging" : v < 15 ? " is-low" : "";
  return (
    <svg className={`battery${tone}`} width={size} height={size * 0.5} viewBox="0 0 22 11" aria-hidden="true">
      <rect className="battery-case" x={0.6} y={0.6} width={18.4} height={9.8} rx={2} />
      <rect className="battery-tip" x={19.8} y={3.5} width={1.6} height={4} rx={0.6} />
      <rect className="battery-fill" x={2.2} y={2.2} width={Math.max(0.8, (15.2 * v) / 100)} height={6.6} rx={0.8} />
    </svg>
  );
}
