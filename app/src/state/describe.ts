import type { DeviceSummary, Encoding, KnownDevice, Mode, Snapshot } from "../api/types";
import type { ScreenState } from "../components/render/DeviceRender";

export type StatusTone = "live" | "warn" | "rescue" | "idle";

export function modeTone(mode: Mode | "disconnected"): StatusTone {
  switch (mode) {
    case "running":
      return "live";
    case "unauthorized":
    case "offline":
    case "no-permission":
      return "warn";
    case "bootloader":
    case "download":
      return "rescue";
    default:
      return "idle";
  }
}

export function displayName(
  device: Pick<DeviceSummary, "model" | "family"> | KnownDevice,
  snapshot: Snapshot | null,
  fallback: string,
): string {
  return device.model ?? snapshot?.identity.model ?? device.family ?? fallback;
}

/** Profile art key for a summary that may not have a profile yet. */
export function artFor(profileId: string | null | undefined): string | null {
  if (profileId === "nw-a50") return "a50";
  if (profileId === "nw-zx500") return "zx500";
  return null;
}

export function isPlaying(snapshot: Snapshot | null): boolean {
  return activeStream(snapshot)?.state === "running";
}

export function dacCard(snapshot: Snapshot | null) {
  return snapshot?.audio.cards.find((c) => c.mainOutput) ?? null;
}

/** What feeds the DAC: a stream on the S-Master card, or an AudioFlinger output where ALSA is hidden. */
export interface Feed {
  state: string;
  format: string | null;
  channels: number | null;
  encoding: Encoding | null;
  /** `hw:card,device`; null when only AudioFlinger could be read. */
  alsa: string | null;
}

export function activeStream(snapshot: Snapshot | null): Feed | null {
  if (!snapshot) return null;
  const card = dacCard(snapshot);
  if (card) {
    const st =
      snapshot.audio.streams.find((s) => s.card === card.index && s.state === "running") ??
      snapshot.audio.streams.find((s) => s.card === card.index && s.state !== "closed");
    return st ? { state: st.state, format: st.format, channels: st.channels, encoding: st.encoding, alsa: `hw:${st.card},${st.device}` } : null;
  }
  const out = snapshot.audio.outputs.find((o) => o.active);
  return out ? { state: "running", format: out.format, channels: out.channels, encoding: out.encoding, alsa: null } : null;
}

export function shortSystem(snapshot: Snapshot): string {
  const sys = snapshot.system;
  if (sys.platform === "walkman-linux") return `${sys.os} ${sys.firmware ?? ""}`.trim();
  const tag = sys.distribution === "TrebleDroid GSI" ? "GSI" : sys.distribution === "Sony firmware" ? null : sys.distribution;
  return tag ? `${sys.os} · ${tag}` : sys.os;
}

/** What the player's own screen is most likely showing, for its render. */
export function screenFor(mode: Mode | "disconnected", snapshot: Snapshot | null): ScreenState {
  if (mode === "unauthorized") return { kind: "auth" };
  if (mode === "offline") return { kind: "busy" };
  if (mode !== "running") return { kind: "off" };
  if (!snapshot) return { kind: "busy" };
  const st = activeStream(snapshot);
  const enc = st?.state === "running" ? st.encoding : null;
  if (!enc) return { kind: "idle" };
  if (enc.type === "dsd") {
    const ratio = enc.rate / 2_822_400;
    const name = Number.isInteger(ratio) ? `DSD${64 * ratio}` : "DSD";
    return { kind: "playing", big: (enc.rate / 1e6).toFixed(1), unit: `MHz · ${name}`, detail: "Native DSD" };
  }
  const k = enc.rate / 1000;
  return {
    kind: "playing",
    big: Number.isInteger(k) ? String(k) : k.toFixed(1),
    unit: `kHz · ${enc.bits}-bit`,
    detail: `PCM · ${st?.channels ?? 2} ch`,
  };
}

/** A user volume's free space, summed. */
export function freeBytes(snapshot: Snapshot | null): number | null {
  if (!snapshot || snapshot.volumes.length === 0) return null;
  return snapshot.volumes.reduce((n, v) => n + v.freeBytes, 0);
}

/** The live output as a large figure: "96" + "kHz", with the rest as a note. */
export function outputFigure(snapshot: Snapshot | null): { value: string; unit: string; note: string } | null {
  const screen = screenFor("running", snapshot);
  if (screen.kind !== "playing") return null;
  const [unit = "", ...rest] = screen.unit.split(" · ");
  return { value: screen.big, unit, note: [...rest, screen.detail].join(" · ") };
}
