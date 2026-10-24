/** A rectangle's path with its top and its bottom corners rounded to separate radii. */
export function outline(x: number, y: number, w: number, h: number, top: number, bottom: number): string {
  const r = x + w;
  const b = y + h;
  const t = top;
  const u = bottom;
  return (
    `M${x + t} ${y}H${r - t}A${t} ${t} 0 0 1 ${r} ${y + t}V${b - u}` +
    `A${u} ${u} 0 0 1 ${r - u} ${b}H${x + u}A${u} ${u} 0 0 1 ${x} ${b - u}V${y + t}A${t} ${t} 0 0 1 ${x + t} ${y}Z`
  );
}
