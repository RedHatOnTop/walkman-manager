import type { Lang } from "./i18n/strings";

/** Decimal units, the way storage is sold and the way the players report it in their own UI. */
export function bytes(n: number, lang: Lang): string {
  const fmt = (v: number, digits: number) =>
    v.toLocaleString(lang === "ko" ? "ko-KR" : "en-US", { maximumFractionDigits: digits, minimumFractionDigits: digits });
  if (n >= 1e12) return `${fmt(n / 1e12, 2)} TB`;
  if (n >= 1e9) return `${fmt(n / 1e9, n >= 1e11 ? 0 : 1)} GB`;
  if (n >= 1e6) return `${fmt(n / 1e6, 0)} MB`;
  if (n >= 1e3) return `${fmt(n / 1e3, 0)} KB`;
  return `${n} B`;
}

export function sampleRate(hz: number): string {
  const k = hz / 1000;
  return `${Number.isInteger(k) ? k : k.toFixed(1)} kHz`;
}

const DSD_BASES = [2_822_400, 3_072_000];

/** "DSD64 · 2.8 MHz" for the one-bit rate the ALSA format implies. */
export function dsdLabel(rate: number): string {
  for (const base of DSD_BASES) {
    const ratio = rate / base;
    if (Number.isInteger(ratio) && ratio >= 1) return `DSD${64 * ratio} · ${(rate / 1e6).toFixed(1)} MHz`;
  }
  return `DSD · ${(rate / 1e6).toFixed(2)} MHz`;
}

export function kib(kb: number, lang: Lang): string {
  return bytes(kb * 1024, lang);
}

export function percent(part: number, whole: number): number {
  if (whole <= 0) return 0;
  return Math.min(100, Math.max(0, (part / whole) * 100));
}

export function usbId(vid: number, pid: number): string {
  const h = (v: number) => v.toString(16).padStart(4, "0");
  return `${h(vid)}:${h(pid)}`;
}

/** Keeps the first and last characters of an identifier visible. */
export function masked(serial: string): string {
  if (serial.length <= 6) return "•".repeat(serial.length);
  return `${serial.slice(0, 3)}${"•".repeat(Math.min(8, serial.length - 6))}${serial.slice(-3)}`;
}
