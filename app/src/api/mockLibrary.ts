// Development only: an invented music library for the browser preview. Artists and
// albums are made up; covers are generated.
import type {
  Album,
  AnalysisPlan,
  AudioFormat,
  Candidate,
  Codec,
  LibraryState,
  Plan,
  PlanItem,
  PlayerLibrary,
  Proposal,
  SensMe,
  PullItem,
  PullPlan,
  Quality,
  ScanProgress,
  SearchResult,
  SyncProgress,
  SyncReport,
  TagChange,
  TagField,
  Track,
  Volume,
  VolumeMusic,
  EngineView,
} from "./types";
import { belowMusic, codecKey, destPath } from "../state/music";
import type { AnalysisDone, SensmeDone } from "./bridge";

interface Spec {
  artist: string | null;
  album: string;
  year: number;
  genre: string;
  codec: Codec;
  rate: number;
  bits: number | null;
  kbps?: number;
  n: number;
  art?: false;
  lyrics?: true;
}

const SPECS: Spec[] = [
  { artist: "Ayla Moreau", album: "Northern Lights in E Minor", year: 2021, genre: "Classical", codec: "flac", rate: 96000, bits: 24, n: 9 },
  { artist: "The Lantern Orchestra", album: "Cartography", year: 2019, genre: "Soundtrack", codec: "dsf", rate: 5644800, bits: 1, n: 8 },
  { artist: "Hyejin Park Trio", album: "Late Blue", year: 2022, genre: "Jazz", codec: "flac", rate: 192000, bits: 24, n: 7 },
  { artist: "Kōsuke Arai", album: "Paper Cranes", year: 2017, genre: "Ambient", codec: "flac", rate: 44100, bits: 16, n: 11 },
  { artist: "Velvet Static", album: "Night Drive Tapes", year: 2020, genre: "Electronic", codec: "mp3", rate: 44100, bits: null, kbps: 320, n: 12 },
  { artist: "Marisol Vega", album: "Casa de Luz", year: 2023, genre: "Latin", codec: "alac", rate: 48000, bits: 24, n: 10, lyrics: true },
  { artist: "Ensemble Lumen", album: "Bach: Partitas", year: 2016, genre: "Classical", codec: "dsf", rate: 2822400, bits: 1, n: 6 },
  { artist: "Noah Lindqvist", album: "Fjord", year: 2018, genre: "Folk", codec: "flac", rate: 96000, bits: 24, n: 9 },
  { artist: "서울 스트링 콰르텟", album: "봄의 기록", year: 2024, genre: "Classical", codec: "flac", rate: 96000, bits: 24, n: 8 },
  { artist: "Echo Park Social", album: "Summer Reels", year: 2015, genre: "Pop", codec: "aac", rate: 44100, bits: null, kbps: 256, n: 11 },
  { artist: "Iris & the Tides", album: "Low Water", year: 2019, genre: "Indie", codec: "flac", rate: 44100, bits: 16, n: 10, lyrics: true },
  { artist: null, album: "Midnight Jazz Sessions", year: 2014, genre: "Jazz", codec: "flac", rate: 44100, bits: 16, n: 14 },
  { artist: "Theo Marchetti", album: "Opera Fragments", year: 2012, genre: "Classical", codec: "wav", rate: 96000, bits: 24, n: 7, art: false },
  { artist: "Mira Solis", album: "Glass Garden", year: 2023, genre: "Electronic", codec: "flac", rate: 48000, bits: 24, n: 9 },
  { artist: "Coastline", album: "Undertow", year: 2021, genre: "Indie", codec: "opus", rate: 48000, bits: null, kbps: 160, n: 8 },
  { artist: "Daniel Okafor", album: "Lagos Morning", year: 2020, genre: "Afrobeat", codec: "flac", rate: 44100, bits: 16, n: 10 },
  { artist: "이하윤", album: "밤의 정원", year: 2025, genre: "K-Indie", codec: "flac", rate: 96000, bits: 24, n: 9, lyrics: true },
  { artist: "Kestrel", album: "Altitude", year: 2022, genre: "Ambient", codec: "dsf", rate: 11289600, bits: 1, n: 5 },
  { artist: "Analog Bloom", album: "Warm Circuits", year: 2013, genre: "Electronic", codec: "ape", rate: 44100, bits: 16, n: 10 },
  { artist: "Ruth Calder", album: "Hymns for the Unbelieving", year: 2018, genre: "Folk", codec: "flac", rate: 88200, bits: 24, n: 12 },
  { artist: "Pale Atlas", album: "Field Recordings Vol. 2", year: 2024, genre: "Ambient", codec: "flac", rate: 96000, bits: 24, n: 6, art: false },
  { artist: "Tomás Ibarra", album: "Guitarra Sola", year: 2011, genre: "Classical", codec: "alac", rate: 44100, bits: 16, n: 13 },
  { artist: "Saffron Line", album: "Monsoon", year: 2016, genre: "World", codec: "mp3", rate: 44100, bits: null, kbps: 245, n: 10 },
  { artist: "Northbound Brass", album: "Fanfares", year: 2022, genre: "Classical", codec: "wav", rate: 192000, bits: 24, n: 6 },
];

const WORDS = [
  "Prelude", "Harbour", "Lanterns", "Undertow", "Glass", "Paper", "Echoes", "Morning", "Drift", "Signal", "Ember",
  "Tidal", "Quiet", "Hollow", "Silver", "Meridian", "Cinder", "Arc", "Fable", "Static", "Bloom", "Cobalt", "North",
  "Lullaby", "Orbit", "Dusk", "Saltwater", "Velvet", "Ivory", "Weather",
];
const KO_WORDS = ["새벽", "물결", "편지", "정원", "오후", "창가", "기억", "바람", "노을", "여름밤", "골목", "첫눈"];

function hash(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 16777619);
  return h >>> 0;
}

function esc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

/** Made-up sleeve art in a handful of print styles, so the grid reads like a record shelf. */
function cover(title: string, artist: string | null): string {
  const h = hash(title);
  const hue = h % 360;
  const hue2 = (hue + 30 + ((h >> 9) % 120)) % 360;
  const style = (h >> 4) % 6;
  const name = esc(artist ?? "Various Artists");
  const t = esc(title);
  const sans = "font-family='Helvetica Neue, Helvetica, Arial, sans-serif'";
  const serif = "font-family='Georgia, Times New Roman, serif'";
  const words = title.split(" ");
  let body = "";
  if (style === 0) {
    // Swiss: flat colour, the title set huge and cropped by the edge.
    body = `<rect width="300" height="300" fill="hsl(${hue} 62% 52%)"/>
<text x="16" y="34" ${sans} font-size="13" font-weight="600" fill="hsl(${hue} 70% 14%)">${name}</text>
<text x="10" y="292" ${sans} font-size="${words.length > 2 ? 64 : 92}" font-weight="800" letter-spacing="-4" fill="hsl(${hue} 75% 92%)">${esc(words[0] ?? "")}</text>
${words.length > 1 ? `<text x="12" y="${words.length > 2 ? 226 : 200}" ${sans} font-size="20" font-weight="700" fill="hsl(${hue} 70% 14%)">${esc(words.slice(1).join(" "))}</text>` : ""}`;
  } else if (style === 1) {
    // A low sun over a horizon, title in small caps under it.
    body = `<defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="hsl(${hue} 45% 16%)"/><stop offset="1" stop-color="hsl(${hue2} 60% 46%)"/></linearGradient></defs>
<rect width="300" height="300" fill="url(#g)"/>
<circle cx="${110 + (h % 80)}" cy="168" r="58" fill="hsl(${(hue + 30) % 360} 90% 72%)"/>
<rect y="192" width="300" height="108" fill="hsl(${hue} 40% 10%)" opacity=".86"/>
${[0, 1, 2, 3].map((i) => `<rect x="${60 + (h % 60) + i * 6}" y="${204 + i * 14}" width="${110 - i * 22}" height="2" fill="hsl(${(hue + 30) % 360} 90% 72%)" opacity="${0.6 - i * 0.12}"/>`).join("")}
<text x="150" y="40" text-anchor="middle" ${sans} font-size="12" font-weight="600" letter-spacing="4" fill="#fff" opacity=".85">${name.toUpperCase()}</text>
<text x="150" y="276" text-anchor="middle" ${serif} font-size="24" font-style="italic" fill="#fff">${t}</text>`;
  } else if (style === 2) {
    // Paper: off-white stock, black serif, one colour block.
    body = `<rect width="300" height="300" fill="#efe9df"/>
<rect x="${24 + (h % 40)}" y="70" width="${120 + (h % 60)}" height="${120 + ((h >> 3) % 50)}" fill="hsl(${hue} 55% 45%)"/>
<text x="24" y="44" ${serif} font-size="22" fill="#141414">${t}</text>
<text x="24" y="276" ${sans} font-size="11" font-weight="600" letter-spacing="2.5" fill="#141414">${name.toUpperCase()}</text>
<line x1="24" y1="256" x2="276" y2="256" stroke="#141414" stroke-width="1"/>`;
  } else if (style === 3) {
    // Concentric grooves, like the label of a record.
    body = `<rect width="300" height="300" fill="hsl(${hue} 30% 9%)"/>
${Array.from({ length: 14 }, (_, i) => `<circle cx="150" cy="150" r="${30 + i * 9}" fill="none" stroke="hsl(${hue2} 70% 70%)" stroke-opacity="${0.08 + (i % 4) * 0.05}" stroke-width="${1 + (i % 3)}"/>`).join("")}
<circle cx="150" cy="150" r="26" fill="hsl(${hue2} 75% 60%)"/>
<circle cx="150" cy="150" r="3" fill="hsl(${hue} 30% 9%)"/>
<text x="150" y="270" text-anchor="middle" ${sans} font-size="17" font-weight="700" fill="#fff">${t}</text>
<text x="150" y="288" text-anchor="middle" ${sans} font-size="10.5" fill="#fff" opacity=".6">${name}</text>`;
  } else if (style === 4) {
    // Duotone bands with the artist set vertically.
    body = `<rect width="300" height="300" fill="hsl(${hue2} 35% 86%)"/>
${Array.from({ length: 7 }, (_, i) => `<rect x="0" y="${i * 44}" width="300" height="${10 + ((h >> i) % 26)}" fill="hsl(${hue} 60% 30%)" opacity="${0.35 + (i % 3) * 0.2}"/>`).join("")}
<rect x="196" y="0" width="104" height="300" fill="hsl(${hue} 60% 22%)"/>
<text transform="translate(262 22) rotate(90)" ${sans} font-size="30" font-weight="800" letter-spacing="-1" fill="hsl(${hue2} 35% 86%)">${name}</text>
<text x="18" y="286" ${sans} font-size="15" font-weight="700" fill="hsl(${hue} 60% 14%)">${t}</text>`;
  } else {
    // Grain gradient with a thin frame and centred title.
    body = `<defs><radialGradient id="g" cx="${0.2 + (h % 6) / 10}" cy="0.3" r="1"><stop offset="0" stop-color="hsl(${hue2} 70% 64%)"/><stop offset=".6" stop-color="hsl(${hue} 50% 30%)"/><stop offset="1" stop-color="hsl(${hue} 45% 12%)"/></radialGradient></defs>
<rect width="300" height="300" fill="url(#g)"/>
<rect x="14" y="14" width="272" height="272" fill="none" stroke="#fff" stroke-opacity=".55"/>
<text x="150" y="150" text-anchor="middle" ${serif} font-size="${t.length > 16 ? 20 : 28}" fill="#fff">${t}</text>
<text x="150" y="176" text-anchor="middle" ${sans} font-size="10.5" font-weight="600" letter-spacing="3" fill="#fff" opacity=".75">${name.toUpperCase()}</text>`;
  }
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 300 300">${body}</svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

const EXT: Record<Codec, string> = {
  flac: "flac", alac: "m4a", wav: "wav", aiff: "aiff", ape: "ape", dsf: "dsf", dff: "dff", mp3: "mp3",
  aac: "m4a", wma: "wma", vorbis: "ogg", opus: "opus", "wav-pack": "wv", musepack: "mpc",
};

function quality(f: AudioFormat): Quality {
  if (f.codec === "dsf" || f.codec === "dff") return "dsd";
  if (["mp3", "aac", "wma", "vorbis", "opus", "musepack"].includes(f.codec)) return "lossy";
  if (f.sampleRate == null && f.bits == null) return (f.bitrateKbps ?? 0) > 1536 ? "hi-res" : "lossless";
  return (f.sampleRate ?? 0) > 48000 || (f.bits ?? 0) > 16 ? "hi-res" : "lossless";
}

const ROOT = "/home/listener/Music";
const covers = new Map<string, string>();

/** `?sensme=none` previews the library before the firmware table is brought in. */
let sensmeTable = typeof location === "undefined" || new URLSearchParams(location.search).get("sensme") !== "none";

/** Made-up analysis for about two in three tracks of the formats a Walkman reads it from. */
function analysis(key: string, codec: Codec, durationMs: number): SensMe | undefined {
  if (!["mp3", "aac", "alac", "flac"].includes(codec)) return undefined;
  const h = hash(key);
  if (h % 3 === 0) return undefined;
  const moods = new Set([5 + ((h >>> 3) % 8)]);
  if ((h >>> 7) % 3 === 0) moods.add(5 + ((h >>> 11) % 8));
  return {
    bpm: 62 + ((h >>> 5) % 118) + ((h >>> 13) % 100) / 100,
    chorusMs: Math.round(durationMs * (0.3 + ((h >>> 17) % 30) / 100)),
    features: "32".repeat(86),
    channels: sensmeTable ? [(h >>> 1) % 5, ...[...moods].sort((a, b) => a - b)] : undefined,
  };
}

function generate(s: Spec, ai: number, root: string, prefix: string, albumId: string, artKey: string | null): { album: Album; tracks: Track[] } {
  const korean = /[가-힣]/.test(s.album);
  const format: AudioFormat = { codec: s.codec, sampleRate: s.rate, bits: s.bits, channels: 2, bitrateKbps: s.kbps ?? null };
  const folder = `${prefix}${s.artist ?? "Various Artists"}/${s.album}`;
  const tracks: Track[] = [];
  let total = 0;
  let bytes = 0;
  for (let i = 0; i < s.n; i++) {
    const words = korean ? KO_WORDS : WORDS;
    const title = `${words[(ai * 7 + i * 3) % words.length]}${korean ? "" : i % 3 === 0 ? "" : ` ${WORDS[(ai + i * 5) % WORDS.length]}`}`;
    const durationMs = 150_000 + (hash(`${s.album}${i}`) % 300_000);
    const perSec =
      s.codec === "dsf" ? (s.rate * 2) / 8 : s.kbps ? (s.kbps * 1000) / 8 : (s.rate * (s.bits ?? 16) * 2) / 8 / (s.codec === "wav" ? 1 : 1.6);
    const size = Math.round((perSec * durationMs) / 1000);
    const rel = `${folder}/${String(i + 1).padStart(2, "0")} ${title}.${EXT[s.codec]}`;
    tracks.push({
      id: hash(`${root}/${rel}`).toString(16).padStart(8, "0"),
      path: `${root}/${rel}`,
      rel,
      root: 0,
      size,
      modified: 1_760_000_000 - ai * 86_400 * 9,
      title,
      artist: s.artist ?? ["Lena Hart", "The Blue Room", "Oskar Veil", "June Tamura"][i % 4],
      album: s.album,
      albumArtist: s.artist ?? undefined,
      genre: s.genre,
      track: i + 1,
      year: s.year,
      durationMs,
      format,
      lyrics: s.lyrics && i % 2 === 0 ? rel.replace(/\.[^.]+$/, ".lrc") : undefined,
      sensme: analysis(rel, s.codec, durationMs),
    });
    total += durationMs;
    bytes += size;
  }
  if (artKey) covers.set(artKey, cover(s.album, s.artist));
  const album: Album = {
    id: albumId,
    title: s.album,
    artist: s.artist,
    compilation: s.artist == null,
    year: s.year,
    genre: s.genre,
    tracks: tracks.map((t) => t.id),
    durationMs: total,
    bytes,
    quality: quality(format),
    format,
    mixedFormats: false,
    art: artKey,
    folder: `${root}/${folder}`,
    modified: 1_760_000_000 - ai * 86_400 * 9,
  };
  return { album, tracks };
}

function byArtist(a: Album, b: Album): number {
  return (a.artist ?? "~").localeCompare(b.artist ?? "~") || a.title.localeCompare(b.title);
}

function build(): LibraryState {
  const tracks: Track[] = [];
  const albums: Album[] = [];
  SPECS.forEach((s, ai) => {
    const made = generate(s, ai, ROOT, "", `album-${ai}`, s.art === false ? null : `mock-${ai}`);
    tracks.push(...made.tracks);
    albums.push(made.album);
  });
  albums.sort(byArtist);
  return {
    roots: [ROOT, "/run/media/listener/Archive/Hi-Res"],
    scanning: false,
    index: { version: 1, roots: [ROOT], scannedAtMs: Date.now(), tracks, albums, skipped: [] },
    sensmeTable,
  };
}

let library: LibraryState | null = null;
function lib(): LibraryState {
  if (!library) library = build();
  return library;
}

export function mockArtUrl(key: string): string {
  lib();
  return covers.get(key) ?? "";
}

type Listener<T> = (payload: T) => void;
const scanListeners = new Set<Listener<{ running: boolean; progress: ScanProgress }>>();
const libraryListeners = new Set<Listener<void>>();
const progressListeners = new Set<Listener<{ planId: string; progress: SyncProgress }>>();
const doneListeners = new Set<Listener<{ planId: string; deviceId: string; report: SyncReport }>>();
const pullProgressListeners = new Set<Listener<{ planId: string; progress: SyncProgress }>>();
const pullDoneListeners = new Set<Listener<{ planId: string; deviceId: string; destRoot: string; report: SyncReport }>>();
const retagProgressListeners = new Set<Listener<{ id: string; progress: SyncProgress }>>();
const retagDoneListeners = new Set<Listener<{ id: string; deviceId: string | null; report: SyncReport; backup: string | null }>>();
const sensmeProgressListeners = new Set<Listener<{ deviceId: string; done: number; total: number }>>();
const sensmeDoneListeners = new Set<Listener<SensmeDone>>();

function subscribe<T>(set: Set<Listener<T>>, l: Listener<T>): Promise<() => void> {
  set.add(l);
  return Promise.resolve(() => set.delete(l));
}

export const onLibraryScan = (l: Listener<{ running: boolean; progress: ScanProgress }>) => subscribe(scanListeners, l);
export const onLibrary = (l: Listener<void>) => subscribe(libraryListeners, l);
export const onSyncProgress = (l: Listener<{ planId: string; progress: SyncProgress }>) => subscribe(progressListeners, l);
export const onSyncDone = (l: Listener<{ planId: string; deviceId: string; report: SyncReport }>) => subscribe(doneListeners, l);
export const onPullProgress = (l: Listener<{ planId: string; progress: SyncProgress }>) => subscribe(pullProgressListeners, l);
export const onPullDone = (l: Listener<{ planId: string; deviceId: string; destRoot: string; report: SyncReport }>) =>
  subscribe(pullDoneListeners, l);
export const onRetagProgress = (l: Listener<{ id: string; progress: SyncProgress }>) => subscribe(retagProgressListeners, l);
export const onRetagDone = (l: Listener<{ id: string; deviceId: string | null; report: SyncReport; backup: string | null }>) =>
  subscribe(retagDoneListeners, l);
export const onSensmeProgress = (l: Listener<{ deviceId: string; done: number; total: number }>) => subscribe(sensmeProgressListeners, l);
export const onSensmeDone = (l: Listener<SensmeDone>) => subscribe(sensmeDoneListeners, l);

export async function sensmeImport(): Promise<boolean> {
  await new Promise((r) => setTimeout(r, 400));
  sensmeTable = true;
  library = null;
  for (const l of libraryListeners) l();
  return true;
}

/** Players whose files were read for SensMe data in this preview. */
const sensmeRead = new Set<string>();

export async function sensmeReadPlayer(id: string): Promise<void> {
  const total = playerLibraries.get(id)?.sensmeUnread ?? 0;
  cancelled = false;
  let done = 0;
  const timer = setInterval(() => {
    done = Math.min(total, done + 7);
    for (const l of sensmeProgressListeners) l({ deviceId: id, done, total });
    if (done >= total || cancelled) {
      clearInterval(timer);
      if (!cancelled) sensmeRead.add(id);
      const analysed = Math.round(done * 0.62);
      for (const l of sensmeDoneListeners) l({ deviceId: id, read: done, analysed, cancelled, error: null });
    }
  }, 120);
}

export async function libraryState(): Promise<LibraryState> {
  return lib();
}

export async function rescanLibrary(): Promise<void> {
  const total = lib().index.tracks.length;
  lib().scanning = true;
  let done = 0;
  const timer = setInterval(() => {
    done = Math.min(total, done + 23);
    const running = done < total;
    for (const l of scanListeners) l({ running, progress: { found: total, done, reused: done } });
    if (!running) {
      clearInterval(timer);
      lib().scanning = false;
      for (const l of libraryListeners) l();
    }
  }, 60);
}

/** What each test player holds, as relative paths under its music roots. */
const ON_PLAYER: Record<string, Partial<Record<Volume["kind"], { album: number; part?: number }[]>>> = {
  "0000test0zx50700": {
    card: [{ album: 0 }, { album: 1 }, { album: 2 }, { album: 6 }, { album: 16 }, { album: 7, part: 4 }],
    internal: [{ album: 3 }],
  },
  TESTA55SERIAL01: { internal: [{ album: 5 }, { album: 10 }] },
};

const pushed: Record<string, Map<string, number>> = {};

export async function deviceMusic(id: string, volumes: Volume[]): Promise<VolumeMusic[]> {
  await new Promise((r) => setTimeout(r, 380));
  const { index } = lib();
  const byAlbum = (i: number) => index.albums.find((a) => a.id === `album-${i}`)!;
  return volumes.map((v) => {
    const files = (ON_PLAYER[id]?.[v.kind] ?? []).flatMap(({ album, part }) =>
      byAlbum(album)
        .tracks.slice(0, part)
        .map((tid) => index.tracks.find((t) => t.id === tid)!)
        .map((t) => ({ rel: destPath(t.rel), size: t.size, modified: t.modified })),
    );
    for (const [rel, size] of pushed[`${id}:${v.kind}`] ?? []) files.push({ rel, size, modified: 0 });
    return {
      kind: v.kind,
      root: v.kind === "card" ? "/storage/3564-3463/Music" : id === "TESTA55SERIAL01" ? "/contents/MUSIC" : "/sdcard/Music",
      exists: true,
      freeBytes: v.freeBytes,
      totalBytes: v.totalBytes,
      files,
    };
  });
}

const plans = new Map<string, Plan>();

export async function syncPlan(
  id: string,
  volume: Volume,
  trackIds: string[],
  codecs: Record<string, number>,
  maxFileBytes: number,
): Promise<Plan> {
  const music = (await deviceMusic(id, [volume]))[0]!;
  const there = new Map(music.files.map((f) => [f.rel.toLowerCase(), f.size]));
  const tracks = new Map(lib().index.tracks.map((t) => [t.id, t]));
  const items: PlanItem[] = [];
  for (const tid of trackIds) {
    const t = tracks.get(tid);
    if (!t) continue;
    const dest = destPath(t.rel);
    const max = codecs[codecKey(t.format.codec)];
    const reason =
      max == null ? ("codec" as const) : (t.format.sampleRate ?? 0) > max ? ("sample-rate" as const) : t.size >= maxFileBytes ? ("file-size" as const) : undefined;
    const existing = there.get(dest.toLowerCase());
    const action = reason ? "unplayable" : existing === t.size ? "skip" : existing != null ? "replace" : "copy";
    items.push({ trackId: t.id, source: t.path, dest, bytes: t.size, action, reason, replacesBytes: action === "replace" ? existing! : 0 });
  }
  const tally = (pred: (i: PlanItem) => boolean) =>
    items.filter(pred).reduce((acc, i) => ({ files: acc.files + 1, bytes: acc.bytes + i.bytes }), { files: 0, bytes: 0 });
  const copy = tally((i) => i.action === "copy" || i.action === "replace");
  const neededBytes = items
    .filter((i) => i.action === "copy" || i.action === "replace")
    .reduce((n, i) => n + Math.max(0, i.bytes - i.replacesBytes), 0);
  const plan: Plan = {
    id: `plan-${Date.now()}`,
    target: { deviceId: id, adbTarget: id, platform: id === "TESTA55SERIAL01" ? "walkman-linux" : "android", volume: volume.kind, root: music.root, freeBytes: volume.freeBytes },
    items,
    copy,
    skip: tally((i) => i.action === "skip"),
    unplayable: tally((i) => i.action === "unplayable"),
    neededBytes,
    fits: neededBytes === 0 || neededBytes + 256 * 1024 * 1024 <= volume.freeBytes,
  };
  plans.set(plan.id, plan);
  return plan;
}

let cancelled = false;

export async function syncStart(planId: string): Promise<void> {
  const plan = plans.get(planId);
  if (!plan) throw { kind: "no-such-plan", message: "the plan has expired; review it again", say: "plan-expired" };
  cancelled = false;
  const work = plan.items.filter((i) => i.action === "copy" || i.action === "replace");
  const total = work.reduce((n, i) => n + i.bytes, 0);
  const progress: SyncProgress = { doneFiles: 0, totalFiles: work.length, doneBytes: 0, totalBytes: total, current: null, failed: 0 };
  const sink = (pushed[`${plan.target.deviceId}:${plan.target.volume}`] ??= new Map());
  let i = 0;
  const step = () => {
    if (cancelled || i >= work.length) {
      progress.current = null;
      for (const l of progressListeners) l({ planId, progress: { ...progress } });
      const report: SyncReport = { copied: i, bytes: progress.doneBytes, verified: i, failures: [], cancelled, scanned: plan.target.platform === "android" && i > 0 ? [plan.target.root] : [] };
      for (const l of doneListeners) l({ planId, deviceId: plan.target.deviceId, report });
      return;
    }
    const item = work[i]!;
    progress.current = item.dest;
    for (const l of progressListeners) l({ planId, progress: { ...progress } });
    setTimeout(() => {
      sink.set(item.dest, item.bytes);
      i++;
      progress.doneFiles = i;
      progress.doneBytes += item.bytes;
      step();
    }, 90 + Math.min(500, item.bytes / 400_000));
  };
  step();
}

export async function syncCancel(): Promise<void> {
  cancelled = true;
}

/** Albums only on the test players, beside the library albums ON_PLAYER lists. */
const PLAYER_ONLY: Record<string, Spec[]> = {
  "0000test0zx50700": [
    { artist: "윤슬", album: "물비늘", year: 2019, genre: "K-Pop", codec: "flac", rate: 44100, bits: 16, n: 10 },
    { artist: "Harbor Lights", album: "Tidal Hours", year: 2021, genre: "Indie", codec: "flac", rate: 96000, bits: 24, n: 9 },
    { artist: "정은하", album: "겨울 편지", year: 2023, genre: "Ballad", codec: "flac", rate: 48000, bits: 24, n: 8 },
    { artist: "Mono Garden", album: "Small Rooms", year: 2014, genre: "Pop", codec: "mp3", rate: 44100, bits: null, kbps: 320, n: 11 },
    { artist: "Clara Wendt", album: "Préludes", year: 2010, genre: "Classical", codec: "flac", rate: 44100, bits: 16, n: 12 },
  ],
  TESTA55SERIAL01: [{ artist: "Paper Hours", album: "Daylight Saving", year: 2018, genre: "Indie", codec: "mp3", rate: 44100, bits: null, kbps: 256, n: 9 }],
};

/** What a player's media store tells about a format: its bit rate and nothing else. */
function storeFormat(t: Track): AudioFormat {
  const kbps = Math.round((t.size * 8) / t.durationMs);
  return { codec: t.format.codec, sampleRate: null, bits: null, channels: null, bitrateKbps: kbps };
}

const playerLibraries = new Map<string, PlayerLibrary>();

export async function playerLibrary(id: string): Promise<PlayerLibrary> {
  await new Promise((r) => setTimeout(r, 900));
  const walkmanOs = id === "TESTA55SERIAL01";
  const card = "/storage/3564-3463";
  const internal = walkmanOs ? "/contents/MUSIC" : "/storage/emulated/0";
  const prefix = walkmanOs ? "" : "Music/";
  const { index } = lib();
  const tracks: Track[] = [];
  const albums: Album[] = [];
  const roots = walkmanOs ? [internal] : [card, internal];
  const add = (album: Album, list: Track[], root: string) => {
    const moved = list.map((t) => {
      const rel = `${prefix}${destPath(t.rel.startsWith(prefix) ? t.rel.slice(prefix.length) : t.rel)}`;
      return {
        ...t,
        id: hash(`${id}\0${root}/${rel}`).toString(16).padStart(8, "0"),
        path: `${root}/${rel}`,
        rel,
        root: roots.indexOf(root),
        lyrics: undefined,
        format: storeFormat(t),
        title: walkmanOs ? t.rel.split("/").pop()!.replace(/^\d+ /, "").replace(/\.[^.]+$/, "") : t.title,
        album: walkmanOs ? undefined : t.album,
        genre: walkmanOs ? undefined : t.genre,
        year: walkmanOs ? undefined : t.year,
      };
    });
    tracks.push(...moved);
    const format = moved[0]!.format;
    albums.push({
      ...album,
      id: `p-${id}-${album.id}`,
      tracks: moved.map((t) => t.id),
      bytes: moved.reduce((n, t) => n + t.size, 0),
      durationMs: moved.reduce((n, t) => n + t.durationMs, 0),
      format,
      quality: quality(format),
      genre: walkmanOs ? null : album.genre,
      year: walkmanOs ? null : album.year,
      art: walkmanOs ? null : album.art,
      folder: `${root}/${prefix}${album.folder.slice(ROOT.length + 1)}`,
    });
  };
  for (const [kind, entries] of Object.entries(ON_PLAYER[id] ?? {})) {
    for (const { album: n, part } of entries ?? []) {
      const album = index.albums.find((a) => a.id === `album-${n}`)!;
      const list = album.tracks.slice(0, part).map((tid) => index.tracks.find((t) => t.id === tid)!);
      add(album, list, kind === "card" ? card : internal);
    }
  }
  (PLAYER_ONLY[id] ?? []).forEach((spec, i) => {
    const made = generate(spec, 40 + i, ROOT, "", `only-${i}`, walkmanOs ? null : `mock-p-${id}-${i}`);
    add(made.album, made.tracks, walkmanOs || i % 2 === 1 ? internal : card);
  });
  albums.sort(byArtist);
  const readable = (t: Track) => ["mp3", "aac", "alac", "flac"].includes(t.format.codec);
  const read = sensmeRead.has(id);
  for (const t of tracks) t.sensme = madeOnPlayer.get(t.id) ?? t.sensme;
  const out: PlayerLibrary = {
    deviceId: id,
    source: walkmanOs ? "folders" : "media-store",
    index: { version: 2, roots, scannedAtMs: Date.now(), tracks: read ? tracks : tracks.map((t) => ({ ...t, sensme: undefined })), albums, skipped: [] },
    sensmeUnread: read ? 0 : tracks.filter(readable).length,
  };
  playerLibraries.set(id, out);
  return out;
}

const pullPlans = new Map<string, PullPlan>();

export async function pullPlan(id: string, trackIds: string[]): Promise<PullPlan> {
  await new Promise((r) => setTimeout(r, 260));
  const player = playerLibraries.get(id);
  if (!player) throw { kind: "unsupported", message: `the music on ${id} has not been read yet`, say: "music-not-read" };
  const here = new Map(lib().index.tracks.map((t) => [t.rel.toLowerCase(), t.size]));
  const byId = new Map(player.index.tracks.map((t) => [t.id, t]));
  const items: PullItem[] = [];
  for (const tid of trackIds) {
    const t = byId.get(tid);
    if (!t) continue;
    const rel = belowMusic(destPath(t.rel));
    const existing = here.get(rel.toLowerCase());
    const action = existing == null ? "copy" : existing === t.size ? "present" : "conflict";
    items.push({ trackId: t.id, remote: t.path, local: `${ROOT}/${rel}`, bytes: t.size, action });
  }
  const tally = (a: PullItem["action"]) =>
    items.filter((i) => i.action === a).reduce((acc, i) => ({ files: acc.files + 1, bytes: acc.bytes + i.bytes }), { files: 0, bytes: 0 });
  const copy = tally("copy");
  const freeBytes = 412 * 1024 ** 3;
  const plan: PullPlan = {
    id: `pull-${Date.now()}`,
    deviceId: id,
    destRoot: ROOT,
    items,
    copy,
    present: tally("present"),
    conflict: tally("conflict"),
    freeBytes,
    fits: copy.bytes + 256 * 1024 * 1024 <= freeBytes,
  };
  pullPlans.set(plan.id, plan);
  return plan;
}

/** Puts pulled tracks into the computer's library, as the rescan after a real pull would. */
function adopt(deviceId: string, items: PullItem[]) {
  const player = playerLibraries.get(deviceId);
  if (!player || items.length === 0) return;
  const state = lib();
  const byId = new Map(player.index.tracks.map((t) => [t.id, t]));
  for (const item of items) {
    const t = byId.get(item.trackId)!;
    const rel = item.local.slice(ROOT.length + 1);
    const copy: Track = { ...t, id: hash(item.local).toString(16).padStart(8, "0"), path: item.local, rel, root: 0 };
    state.index.tracks.push(copy);
    const from = player.index.albums.find((a) => a.tracks.includes(t.id))!;
    const id = `pc-${from.id}`;
    const album = state.index.albums.find((a) => a.id === id);
    if (album) {
      album.tracks.push(copy.id);
      album.bytes += copy.size;
      album.durationMs += copy.durationMs;
    } else {
      state.index.albums.push({
        ...from,
        id,
        tracks: [copy.id],
        bytes: copy.size,
        durationMs: copy.durationMs,
        folder: item.local.slice(0, item.local.lastIndexOf("/")),
        modified: Math.floor(Date.now() / 1000),
      });
    }
  }
  state.index.albums.sort(byArtist);
  library = { ...state, index: { ...state.index } };
  for (const l of libraryListeners) l();
}

export async function pullStart(planId: string): Promise<void> {
  const plan = pullPlans.get(planId);
  if (!plan) throw { kind: "no-such-plan", message: "the plan has expired; review it again", say: "plan-expired" };
  cancelled = false;
  const work = plan.items.filter((i) => i.action === "copy");
  const progress: SyncProgress = {
    doneFiles: 0,
    totalFiles: work.length,
    doneBytes: 0,
    totalBytes: work.reduce((n, i) => n + i.bytes, 0),
    current: null,
    failed: 0,
  };
  let i = 0;
  const step = () => {
    if (cancelled || i >= work.length) {
      progress.current = null;
      for (const l of pullProgressListeners) l({ planId, progress: { ...progress } });
      adopt(plan.deviceId, work.slice(0, i));
      const report: SyncReport = { copied: i, bytes: progress.doneBytes, verified: i, failures: [], cancelled, scanned: [] };
      for (const l of pullDoneListeners) l({ planId, deviceId: plan.deviceId, destRoot: plan.destRoot, report });
      return;
    }
    const item = work[i]!;
    progress.current = item.local.slice(plan.destRoot.length + 1);
    for (const l of pullProgressListeners) l({ planId, progress: { ...progress } });
    setTimeout(() => {
      i++;
      progress.doneFiles = i;
      progress.doneBytes += item.bytes;
      step();
    }, 90 + Math.min(500, item.bytes / 400_000));
  };
  step();
}

// Looking albums up: made-up releases that differ from the tags in the ways real ones do.

const candidates = new Map<string, Candidate>();

export async function lookupSearch(q: { album: string; artist: string | null; tracks: number }): Promise<SearchResult> {
  await new Promise((r) => setTimeout(r, 650));
  const artist = q.artist ?? "Various Artists";
  const variants: [string, Candidate["source"], string | null, string | null, number, number, number][] = [
    [q.album, "music-brainz", "KR", "CD", q.tracks + 1, 0, 96],
    [q.album, "itunes", "KR", null, q.tracks + 1, 0, 91],
    [`${q.album} (Deluxe Edition)`, "music-brainz", "XW", "Digital Media", q.tracks + 4, 2, 64],
    [`${q.album} – Live`, "itunes", "KR", "EP", Math.max(3, q.tracks - 2), 5, 41],
  ];
  const list = variants.map(([title, source, country, format, n, later, score], i): Candidate => {
    const id = `${source}:${hash(`${q.album}\0${i}`).toString(16)}`;
    const c: Candidate = {
      id,
      source,
      title,
      artist,
      year: 2019 + later,
      date: `${2019 + later}-0${(i % 8) + 1}-1${i}`,
      country,
      label: i % 2 === 0 ? "Sunline Records" : "℗ Sunline Records",
      format,
      trackCount: n,
      score,
    };
    candidates.set(id, c);
    covers.set(`lk-${id}`, cover(i === 3 ? `${q.album} Live` : q.album, artist));
    return c;
  });
  return { candidates: list, failed: [] };
}

export async function lookupCover(id: string): Promise<string | null> {
  await new Promise((r) => setTimeout(r, 120 + (hash(id) % 400)));
  return covers.has(`lk-${id}`) ? `lk-${id}` : null;
}

interface StoredProposal {
  proposal: Proposal;
  tracks: Track[];
}
const proposals = new Map<string, StoredProposal>();

function findAlbum(albumId: string, device: string | null): { album: Album; tracks: Track[] } {
  const index = device ? playerLibraries.get(device)?.index : lib().index;
  const album = index?.albums.find((a) => a.id === albumId);
  if (!index || !album) throw { kind: "unsupported", message: "that album is no longer in the library", say: "album-gone" };
  const byId = new Map(index.tracks.map((t) => [t.id, t]));
  return { album, tracks: album.tracks.map((id) => byId.get(id)!).filter(Boolean) };
}

export async function lookupPropose(candidateId: string, albumId: string, device: string | null): Promise<Proposal> {
  await new Promise((r) => setTimeout(r, 700));
  const c = candidates.get(candidateId);
  if (!c) throw { kind: "unsupported", message: "that result has expired; search again", say: "result-expired" };
  const { album, tracks } = findAlbum(albumId, device);
  const changes: TagChange[] = [];
  const add = (field: TagField, from: string | null, to: string | null) => {
    if (to != null && from !== to) changes.push({ field, from, to });
  };
  add("album", album.title, c.title);
  add("albumArtist", tracks[0]?.albumArtist ?? null, c.artist);
  add("genre", album.genre, album.genre === "K-Indie" ? "K-Pop" : (album.genre ?? "Pop"));
  add("year", album.year?.toString() ?? null, c.year?.toString() ?? null);
  const fixes = (title: string, i: number) => (i === 1 ? title.toUpperCase() === title ? title : title.replace(/\b\w/g, (m) => m.toUpperCase()) : i === 3 ? `${title} (Remastered)` : title);
  const proposal: Proposal = {
    id: `retag-${hash(candidateId + albumId).toString(16)}`,
    candidate: c,
    album: changes,
    tracks: tracks.map((t, i) => {
      const matched = i === tracks.length - 1 && tracks.length > 3 ? null : { disc: 1, number: i + 1, title: fixes(t.title ?? "", i), artist: t.artist ?? c.artist, durationMs: t.durationMs + ((i * 700) % 2100) };
      const tc: TagChange[] = [];
      if (matched && matched.title !== t.title) tc.push({ field: "title", from: t.title ?? null, to: matched.title });
      if (matched && t.track !== matched.number) tc.push({ field: "track", from: t.track?.toString() ?? null, to: String(matched.number) });
      if (matched && i === 2) tc.push({ field: "artist", from: t.artist ?? null, to: `${c.artist} feat. Ola Strand` });
      return {
        trackId: t.id,
        name: t.rel.split("/").pop()!,
        matched,
        changes: tc,
        repairs: i === 0 && !!device,
        writable: t.format.codec !== "dsf" && t.format.codec !== "wma",
      };
    }),
    unmatched: [{ disc: 1, number: tracks.length + 1, title: "Hidden Track", artist: c.artist, durationMs: 201000 }],
    cover: { art: `lk-${candidateId}`, width: c.source === "itunes" ? 600 : 500, height: c.source === "itunes" ? 600 : 500, bytes: 84211 },
    deviceId: device,
    folder: album.folder,
    backupRoot: device ? "/home/listener/.local/share/dev.personnya.walkman-manager/backups" : null,
  };
  proposals.set(proposal.id, { proposal, tracks });
  return proposal;
}

export async function retagForget(id: string): Promise<void> {
  proposals.delete(id);
}

export async function retagStart(id: string, albumFields: TagField[], picked: string[], withCover: boolean): Promise<void> {
  const stored = proposals.get(id);
  if (!stored) throw { kind: "no-such-plan", message: "the proposal has expired; look the album up again", say: "proposal-expired" };
  proposals.delete(id);
  const { proposal, tracks } = stored;
  const all = albumFields.length > 0 || withCover;
  const work = proposal.tracks.filter((tp) => tp.writable && (all || tp.repairs || (picked.includes(tp.trackId) && tp.changes.length > 0)));
  cancelled = false;
  const progress: SyncProgress = { doneFiles: 0, totalFiles: work.length, doneBytes: 0, totalBytes: 0, current: null, failed: 0 };
  let i = 0;
  const step = () => {
    if (cancelled || i >= work.length) {
      progress.current = null;
      for (const l of retagProgressListeners) l({ id, progress: { ...progress } });
      for (const t of tracks) {
        const tp = proposal.tracks.find((x) => x.trackId === t.id);
        if (!tp || !work.includes(tp)) continue;
        for (const ch of albumFields.length > 0 ? proposal.album.filter((a) => albumFields.includes(a.field)) : []) {
          if (ch.field === "album") t.album = ch.to;
          if (ch.field === "genre") t.genre = ch.to;
        }
        if (picked.includes(t.id)) for (const ch of tp.changes) if (ch.field === "title") t.title = ch.to;
      }
      const album = (proposal.deviceId ? playerLibraries.get(proposal.deviceId)?.index : lib().index)?.albums.find((a) => a.folder === proposal.folder);
      if (album) {
        const title = proposal.album.find((a) => a.field === "album" && albumFields.includes("album"));
        if (title) album.title = title.to;
        if (withCover && proposal.cover) album.art = proposal.cover.art;
      }
      if (!proposal.deviceId) {
        library = { ...lib(), index: { ...lib().index } };
        for (const l of libraryListeners) l();
      }
      const report: SyncReport = { copied: i, bytes: 0, verified: i, failures: [], cancelled, scanned: [] };
      const backup = proposal.deviceId ? `${proposal.backupRoot}/NW-ZX507/2026-10-16 214500` : null;
      for (const l of retagDoneListeners) l({ id, deviceId: proposal.deviceId, report, backup });
      return;
    }
    const tp = work[i]!;
    progress.current = tracks.find((t) => t.id === tp.trackId)?.rel ?? tp.name;
    for (const l of retagProgressListeners) l({ id, progress: { ...progress } });
    setTimeout(() => {
      i++;
      progress.doneFiles = i;
      step();
    }, 380);
  };
  step();
}

/** `?engine=none` previews the board before Music Center's engine is found. */
let engineFound = typeof location === "undefined" || new URLSearchParams(location.search).get("engine") !== "none";
const ENGINE = {
  dll: "/home/listener/.wine/drive_c/Program Files (x86)/Sony/Music Center/AVLib/MMLib11.dll",
  wine: "/usr/bin/wine",
  ffmpeg: "/usr/bin/ffmpeg",
};
const analysisProgressListeners = new Set<Listener<{ planId: string; progress: SyncProgress }>>();
const analysisDoneListeners = new Set<Listener<AnalysisDone>>();
export const onAnalysisProgress = (l: Listener<{ planId: string; progress: SyncProgress }>) => subscribe(analysisProgressListeners, l);
export const onAnalysisDone = (l: Listener<AnalysisDone>) => subscribe(analysisDoneListeners, l);

function engineView(): EngineView {
  return engineFound
    ? { engine: ENGINE, problem: null, needEngine: false }
    : {
        engine: null,
        problem: { kind: "unsupported", message: "Music Center for PC was not found; install it, or choose its AVLib/MMLib11.dll", say: "no-music-center" },
        needEngine: true,
      };
}

export async function analysisEngine(): Promise<EngineView> {
  await new Promise((r) => setTimeout(r, 200));
  return engineView();
}

export async function analysisPickEngine(): Promise<EngineView | null> {
  await new Promise((r) => setTimeout(r, 400));
  engineFound = true;
  return engineView();
}

const analysisPlans = new Map<string, { deviceId: string | null; tracks: Track[] }>();
/** Analysis "written" into player files in this preview, by track id. */
const madeOnPlayer = new Map<string, SensMe>();

export async function analysisPlan(device: string | null): Promise<AnalysisPlan> {
  await new Promise((r) => setTimeout(r, 300));
  if (!engineFound) throw engineView().problem;
  const index = device ? playerLibraries.get(device)?.index : lib().index;
  if (!index) throw { kind: "unsupported", message: `the music on ${device} has not been read yet`, say: "music-not-read" };
  const missing = index.tracks.filter((t) => !t.sensme);
  const todo = missing.filter((t) => t.format.codec === "mp3" || t.format.codec === "flac");
  const others = new Map<Codec, number>();
  for (const t of missing) if (!todo.includes(t)) others.set(t.format.codec, (others.get(t.format.codec) ?? 0) + 1);
  const id = `analysis-${device ?? "here"}-${todo.length}`;
  analysisPlans.set(id, { deviceId: device, tracks: todo });
  return {
    id,
    deviceId: device,
    tracks: todo.length,
    bytes: todo.reduce((n, t) => n + t.size, 0),
    durationMs: todo.reduce((n, t) => n + t.durationMs, 0),
    others: [...others].map(([codec, tracks]) => ({ codec, tracks })),
    engine: ENGINE,
    backupRoot: device ? "/home/listener/.local/share/dev.personnya.walkman-manager/backups" : null,
    freeBytes: device ? 182_000_000_000 : null,
  };
}

export async function analysisStart(planId: string): Promise<void> {
  const plan = analysisPlans.get(planId);
  if (!plan) throw { kind: "no-such-plan", message: "the plan has expired; look again", say: "plan-expired" };
  analysisPlans.delete(planId);
  cancelled = false;
  const progress: SyncProgress = {
    doneFiles: 0,
    totalFiles: plan.tracks.length,
    doneBytes: 0,
    totalBytes: plan.tracks.reduce((n, t) => n + t.size, 0),
    current: null,
    failed: 0,
  };
  let i = 0;
  const step = () => {
    if (cancelled || i >= plan.tracks.length) {
      progress.current = null;
      for (const l of analysisProgressListeners) l({ planId, progress: { ...progress } });
      const report: SyncReport = { copied: i, bytes: progress.doneBytes, verified: i, failures: [], cancelled, scanned: [] };
      const backup = plan.deviceId ? "/home/listener/.local/share/dev.personnya.walkman-manager/backups/NW-ZX507/2026-10-17 141500" : null;
      if (!plan.deviceId) {
        library = { ...lib(), index: { ...lib().index } };
        for (const l of libraryListeners) l();
      }
      for (const l of analysisDoneListeners) l({ planId, deviceId: plan.deviceId, report, backup });
      return;
    }
    const t = plan.tracks[i]!;
    progress.current = t.rel;
    for (const l of analysisProgressListeners) l({ planId, progress: { ...progress } });
    setTimeout(() => {
      t.sensme = analysis(`${t.rel}#made`, "mp3", t.durationMs) ?? analysis(`${t.rel}#made2`, "mp3", t.durationMs);
      if (plan.deviceId && t.sensme) madeOnPlayer.set(t.id, t.sensme);
      i++;
      progress.doneFiles = i;
      progress.doneBytes += t.size;
      step();
    }, 160);
  };
  step();
}
