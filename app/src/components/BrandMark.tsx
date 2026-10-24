const BARS: [number, number][] = [
  [11.6, 2.6],
  [12.8, 5.8],
  [14, 4.1],
  [15.2, 6.8],
  [16.4, 3.3],
];

export function BrandMark({ size = 28 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 28 28" aria-hidden="true" className="brand-mark">
      <rect x="6.75" y="2.75" width="14.5" height="22.5" rx="3.75" fill="none" stroke="var(--text-1)" strokeWidth="1.6" />
      {BARS.map(([x, h]) => (
        <line key={x} x1={x} x2={x} y1={13 - h / 2} y2={13 + h / 2} stroke="var(--signal)" strokeWidth="0.95" strokeLinecap="round" />
      ))}
      <circle cx="14" cy="21" r="1.3" fill="var(--text-1)" />
    </svg>
  );
}
