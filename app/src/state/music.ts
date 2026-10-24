import type { Album, AudioFormat, Codec, Quality, Track, VolumeMusic } from "../api/types";

export const QUALITIES: Quality[] = ["dsd", "hi-res", "lossless", "lossy"];

const CODEC_NAMES: Record<Codec, string> = {
  flac: "FLAC",
  alac: "ALAC",
  wav: "WAV",
  aiff: "AIFF",
  ape: "APE",
  dsf: "DSF",
  dff: "DFF",
  mp3: "MP3",
  aac: "AAC",
  wma: "WMA",
  vorbis: "Vorbis",
  opus: "Opus",
  "wav-pack": "WavPack",
  musepack: "Musepack",
};

/** The key the player profiles use for a codec. */
export function codecKey(c: Codec): string {
  return c === "wav-pack" ? "wavpack" : c;
}

export function codecName(c: Codec): string {
  return CODEC_NAMES[c];
}

function khz(rate: number): string {
  const k = rate / 1000;
  return Number.isInteger(k) ? String(k) : k.toFixed(1);
}

/** "FLAC 24/96", "DSD128", "MP3 320k": ASCII only, set in mono. */
export function formatLabel(f: AudioFormat): string {
  if (f.codec === "dsf" || f.codec === "dff") {
    if (!f.sampleRate) return "DSD";
    for (const base of [2_822_400, 3_072_000]) {
      const ratio = f.sampleRate / base;
      if (Number.isInteger(ratio)) return `DSD${64 * ratio}`;
    }
    return `DSD ${(f.sampleRate / 1e6).toFixed(1)}M`;
  }
  const name = CODEC_NAMES[f.codec];
  const lossless = ["flac", "alac", "wav", "aiff", "ape", "wav-pack"].includes(f.codec);
  if (lossless && f.sampleRate) return f.bits ? `${name} ${f.bits}/${khz(f.sampleRate)}` : `${name} ${khz(f.sampleRate)}k`;
  if (f.bitrateKbps && !lossless) return `${name} ${f.bitrateKbps}k`;
  return name;
}

export function duration(ms: number): string {
  const s = Math.round(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}` : `${m}:${String(sec).padStart(2, "0")}`;
}

/** Mirrors `uwm_core::library::plan::dest_path`, so presence matches what a sync would write. */
export function destPath(rel: string): string {
  return rel
    .split("/")
    .filter((p) => p && p !== "." && p !== "..")
    .map((p) => {
      let s = p.replace(/[<>:"\\|?*\u0000-\u001f\u007f-\u009f]/g, "_");
      s = s.replace(/[. ]+$/, "");
      return s || "_";
    })
    .join("/");
}

/** Mirrors `uwm_core::library::pull::below_music`: where a player file lands below the computer's music folder. */
export function belowMusic(rel: string): string {
  const cut = rel.indexOf("/");
  return cut > 0 && rel.slice(0, cut).toLowerCase() === "music" ? rel.slice(cut + 1) : rel;
}

/** relative path (lower case) → size, for every file on a player's volumes. */
export type PresenceMap = Map<string, number>;

export function presenceMap(volumes: VolumeMusic[]): PresenceMap {
  const m: PresenceMap = new Map();
  for (const v of volumes) for (const f of v.files) m.set(f.rel.toLowerCase(), f.size);
  return m;
}

export function trackOnPlayer(t: Track, map: PresenceMap): boolean {
  return map.get(destPath(t.rel).toLowerCase()) === t.size;
}

export interface AlbumPresence {
  present: number;
  total: number;
}

export function albumPresence(album: Album, tracks: Map<string, Track>, map: PresenceMap): AlbumPresence {
  let present = 0;
  for (const id of album.tracks) {
    const t = tracks.get(id);
    if (t && trackOnPlayer(t, map)) present++;
  }
  return { present, total: album.tracks.length };
}

export function matchesQuery(album: Album, q: string): boolean {
  if (!q) return true;
  const hay = `${album.title}\n${album.artist ?? ""}\n${album.genre ?? ""}\n${album.year ?? ""}`.toLowerCase();
  return q
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((w) => hay.includes(w));
}

/** Tracks of the computer's library by where a pull would put them, for "already here" marks. */
export type PcPresence = Set<string>;

export function pcPresence(tracks: Iterable<Track>): PcPresence {
  const out: PcPresence = new Set();
  for (const t of tracks) out.add(`${destPath(t.rel).toLowerCase()}\0${t.size}`);
  return out;
}

export function trackOnPc(t: Track, pc: PcPresence): boolean {
  return pc.has(`${belowMusic(destPath(t.rel)).toLowerCase()}\0${t.size}`);
}

export function albumOnPc(album: Album, tracks: Map<string, Track>, pc: PcPresence): AlbumPresence {
  let present = 0;
  for (const id of album.tracks) {
    const t = tracks.get(id);
    if (t && trackOnPc(t, pc)) present++;
  }
  return { present, total: album.tracks.length };
}
