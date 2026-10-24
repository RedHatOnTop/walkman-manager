// Mirrors the serde output of uwm-core. Field names are camelCase on the wire.

export type Link =
  | "adb"
  | "adb-network"
  | "fastboot"
  | "mass-storage"
  | "mtp"
  | "mtk-brom"
  | "mtk-preloader"
  | "nxp-serial-download";

export type Mode =
  | "running"
  | "unauthorized"
  | "offline"
  | "storage-only"
  | "bootloader"
  | "download"
  | "no-permission";

export interface UsbLink {
  port: string;
  vid: number;
  pid: number;
  speed: string | null;
  product: string | null;
}

export interface DeviceSummary {
  id: string;
  serial: string | null;
  profileId: string | null;
  family: string | null;
  model: string | null;
  modelBasis: string | null;
  mode: Mode;
  links: Link[];
  usb: UsbLink | null;
  adbTarget: string | null;
}

export interface Scan {
  devices: DeviceSummary[];
  issues: string[];
}

export interface KnownDevice {
  serial: string;
  profileId: string;
  family: string;
  model: string | null;
  modelBasis: string | null;
}

export interface Identity {
  serial: string | null;
  profileId: string | null;
  family: string | null;
  model: string | null;
  modelBasis: string | null;
}

export interface SystemInfo {
  platform: "walkman-linux" | "android";
  os: string;
  distribution: string | null;
  firmware: string | null;
  apiLevel: number | null;
  buildId: string | null;
  buildType: string | null;
  securityPatch: string | null;
  vendorFirmware: string | null;
  build: string | null;
  kernel: string | null;
  kernelDate: string | null;
  slot: string | null;
  bootloaderUnlocked: boolean | null;
  verifiedBoot: string | null;
  container: { root: string } | null;
}

export interface Power {
  level: number | null;
  status: string | null;
  voltageMv: number | null;
  temperatureC: number | null;
  health: string | null;
  externalPower: boolean;
}

export interface Volume {
  kind: "internal" | "card";
  mount: string;
  filesystem: string;
  totalBytes: number;
  usedBytes: number;
  freeBytes: number;
}

export type Encoding = { type: "pcm"; bits: number; rate: number } | { type: "dsd"; rate: number };

export interface AudioStream {
  card: number;
  device: number;
  state: string;
  format: string | null;
  rate: number | null;
  channels: number | null;
  encoding: Encoding | null;
}

/** An AudioFlinger output thread, read where the shell user cannot open /proc/asound. */
export interface AudioOutput {
  name: string;
  kind: string;
  active: boolean;
  format: string | null;
  rate: number | null;
  channels: number | null;
  encoding: Encoding | null;
}

export interface AudioCard {
  index: number;
  id: string;
  name: string;
  mainOutput: boolean;
}

export interface Snapshot {
  takenAtMs: number;
  identity: Identity;
  system: SystemInfo;
  power: Power | null;
  memory: { totalKb: number; availableKb: number | null } | null;
  volumes: Volume[];
  audio: { cards: AudioCard[]; streams: AudioStream[]; outputs: AudioOutput[] };
}

export interface Profile {
  id: string;
  family: string;
  art: "a50" | "zx500" | string;
  soc: string;
  dac: string;
  ramMb: number;
  bodyMm: [number, number, number];
  display: string;
  massG: number;
  stockOs: string;
  stockFirmwareLatest?: string;
  recovery: string[];
  variants: { model: string; storageGb: number }[];
  usb: { vid: number; pid: number; seen: string }[];
  playback: { maxFileBytes: number; codecs: Record<string, number> };
}

export interface Environment {
  version: string;
  os: string;
  adbPath: string | null;
  adbVersion: string | null;
}

/** A failed command. `say` names the `err.*` string to show, filled with `vars`; `detail` is
 * a path, an address or a tool's own words, shown under it as is; `message` is the English
 * the logs keep. */
export interface ErrorReport {
  kind: string;
  message: string;
  say?: string;
  vars?: Record<string, string>;
  detail?: string;
}

/** A text in each language the app speaks. */
export interface Localized {
  en: string;
  ko: string;
}

// Library ----------------------------------------------------------------

export type Codec =
  | "flac"
  | "alac"
  | "wav"
  | "aiff"
  | "ape"
  | "dsf"
  | "dff"
  | "mp3"
  | "aac"
  | "wma"
  | "vorbis"
  | "opus"
  | "wav-pack"
  | "musepack";

export type Quality = "lossy" | "lossless" | "hi-res" | "dsd";

export interface AudioFormat {
  codec: Codec;
  sampleRate: number | null;
  bits: number | null;
  channels: number | null;
  bitrateKbps: number | null;
}

export interface Track {
  id: string;
  path: string;
  rel: string;
  root: number;
  size: number;
  modified: number;
  title?: string;
  artist?: string;
  album?: string;
  albumArtist?: string;
  genre?: string;
  track?: number;
  disc?: number;
  year?: number;
  durationMs: number;
  format: AudioFormat;
  cover?: string;
  lyrics?: string;
  /** Its text tags are legacy CP949 or Shift_JIS bytes that other players show garbled. */
  legacyText?: boolean;
  /** Sony's 12 TONE ANALYSIS, which a Walkman sorts into SensMe channels. */
  sensme?: SensMe;
}

export interface SensMe {
  bpm?: number;
  /** Where the chorus starts; a SensMe channel begins the track there. */
  chorusMs?: number;
  features: string;
  /** Channel numbers 0..12; absent until the firmware table is known, or when the data is a mix the port does not follow. */
  channels?: number[];
}

/** Music Center for PC's analysis engine and what runs it here. */
export interface EngineInfo {
  dll: string;
  /** Wine, or null where the engine runs natively. */
  wine: string | null;
  ffmpeg: string;
}

export interface EngineView {
  engine: EngineInfo | null;
  problem: ErrorReport | null;
  /** The engine itself is what is missing, so choosing its file helps. */
  needEngine: boolean;
}

/** What analysing the unanalysed tracks of the library or of a player would do. */
export interface AnalysisPlan {
  id: string;
  deviceId: string | null;
  /** Tracks without analysis in a format it can be written into (MP3, FLAC). */
  tracks: number;
  bytes: number;
  durationMs: number;
  /** Tracks without analysis in other formats, which are left as they are. */
  others: { codec: Codec; tracks: number }[];
  engine: EngineInfo;
  /** Player only: where the originals are kept, and the room there. */
  backupRoot: string | null;
  freeBytes: number | null;
}

export interface Album {
  id: string;
  title: string;
  artist: string | null;
  compilation: boolean;
  year: number | null;
  genre: string | null;
  tracks: string[];
  durationMs: number;
  bytes: number;
  quality: Quality;
  format: AudioFormat;
  mixedFormats: boolean;
  art: string | null;
  folder: string;
  modified: number;
}

export interface LibraryIndex {
  version: number;
  roots: string[];
  scannedAtMs: number;
  tracks: Track[];
  albums: Album[];
  skipped: { path: string; reason: string }[];
}

export interface LibraryState {
  roots: string[];
  scanning: boolean;
  index: LibraryIndex;
  /** Whether the firmware's SensMe table has been brought in, so channels can be worked out. */
  sensmeTable: boolean;
}

export interface ScanProgress {
  found: number;
  done: number;
  reused: number;
}

export interface RemoteFile {
  rel: string;
  size: number;
  modified: number;
}

export interface VolumeMusic {
  kind: Volume["kind"];
  root: string;
  exists: boolean;
  freeBytes: number;
  totalBytes: number;
  files: RemoteFile[];
}

export type PlanAction = "copy" | "replace" | "skip" | "unplayable";
export type Unplayable = "codec" | "sample-rate" | "file-size";

export interface PlanItem {
  trackId?: string;
  source: string;
  dest: string;
  bytes: number;
  action: PlanAction;
  reason?: Unplayable;
  replacesBytes: number;
}

export interface Tally {
  files: number;
  bytes: number;
}

export interface Plan {
  id: string;
  target: {
    deviceId: string;
    adbTarget: string;
    platform: SystemInfo["platform"];
    volume: Volume["kind"];
    root: string;
    freeBytes: number;
  };
  items: PlanItem[];
  copy: Tally;
  skip: Tally;
  unplayable: Tally;
  neededBytes: number;
  fits: boolean;
}

export interface SyncProgress {
  doneFiles: number;
  totalFiles: number;
  doneBytes: number;
  totalBytes: number;
  current: string | null;
  failed: number;
}

export interface SyncReport {
  copied: number;
  bytes: number;
  verified: number;
  failures: { dest: string; error: ErrorReport }[];
  cancelled: boolean;
  scanned: string[];
  scanError?: ErrorReport;
}

export interface PlayerLibrary {
  deviceId: string;
  /** `media-store`: read from the player's own music database; `folders`: albums from the folder layout. */
  source: "media-store" | "folders";
  index: LibraryIndex;
  /** Files that may hold SensMe data and were not read for it yet. */
  sensmeUnread: number;
}

export type PullAction = "copy" | "present" | "conflict";

export interface PullItem {
  trackId: string;
  remote: string;
  local: string;
  bytes: number;
  action: PullAction;
}

export interface PullPlan {
  id: string;
  deviceId: string;
  destRoot: string;
  items: PullItem[];
  copy: Tally;
  present: Tally;
  conflict: Tally;
  freeBytes: number | null;
  fits: boolean;
}

export type LookupSource = "music-brainz" | "itunes";

export interface Candidate {
  id: string;
  source: LookupSource;
  title: string;
  artist: string;
  year: number | null;
  date: string | null;
  country: string | null;
  label: string | null;
  format: string | null;
  trackCount: number;
  /** How well it fits the album asked about, 0–100. */
  score: number;
}

export interface LookupQuery {
  album: string;
  artist: string | null;
  tracks: number;
}

export interface SearchResult {
  candidates: Candidate[];
  /** A source that did not answer while the other did. */
  failed: [LookupSource, ErrorReport][];
}

export type TagField = "title" | "artist" | "album" | "albumArtist" | "genre" | "year" | "track" | "disc";

export interface TagChange {
  field: TagField;
  from: string | null;
  to: string;
}

export interface ReleaseTrack {
  disc: number;
  number: number;
  title: string;
  artist: string;
  durationMs: number | null;
}

export interface TrackProposal {
  trackId: string;
  name: string;
  matched: ReleaseTrack | null;
  changes: TagChange[];
  repairs: boolean;
  writable: boolean;
}

export interface Proposal {
  id: string;
  candidate: Candidate;
  album: TagChange[];
  tracks: TrackProposal[];
  unmatched: ReleaseTrack[];
  cover: { art: string; width: number; height: number; bytes: number } | null;
  deviceId: string | null;
  folder: string;
  backupRoot: string | null;
}

// Firmware Studio

export type FormatKind =
  | "sonyNwwm"
  | "sonyUpg"
  | "otaPayload"
  | "otaBlocks"
  | "ext4"
  | "sparse"
  | "xz"
  | "gzip"
  | "bootImage"
  | "vbMeta"
  | "kernelImage"
  | "zip"
  | "wmBackup"
  | "unknown";

export type FileFormat =
  | { kind: "sonyNwwm"; payloadBytes: number }
  | { kind: "otaPayload" | "otaBlocks"; device: string | null }
  | { kind: "ext4"; fsBytes: number; label: string }
  | { kind: "bootImage"; headerVersion: number }
  | { kind: "kernelImage"; release: string | null }
  /** An NW-A50 series backup's manifest.txt, with the images beside it. */
  | { kind: "wmBackup"; images: number }
  | { kind: "sonyUpg" | "sparse" | "xz" | "gzip" | "vbMeta" | "zip" | "unknown" };

export type Damage =
  | { kind: "truncated"; expected: number; actual: number }
  | { kind: "changed"; known: string }
  /** An image a backup's list names is missing, or reads otherwise than the player's MD5. */
  | { kind: "unlike"; file: string };

/** A file someone else publishes, and how the Studio knows a copy is that file. */
export interface KnownFile {
  id: string;
  title: Localized;
  format: FormatKind;
  bytes?: number;
  sha256?: string;
  md5?: string;
  /** A direct download the app may fetch for the owner. */
  url?: string;
  /** The heading in docs/guides/firmware-files.md and .ko.md that says where the publisher put it. */
  guide?: Localized;
  from?: string;
}

export interface VaultFile {
  id: string;
  added: number;
  path: string;
  bytes: number;
  modified: number;
  format: FileFormat;
  sha256: string;
  md5: string;
  known: string | null;
  damage: Damage | null;
  /** `changed`: size or time moved since it was hashed. */
  presence: "here" | "changed" | "gone";
}

export interface KitInput {
  id: string;
  optional: boolean;
  accepts: KnownFile[];
  /** Files of the owner's own making, taken by what they are. */
  formats: { kind: FormatKind; release?: string }[];
  /** The guide has a section on making or getting this input's file. */
  guided: boolean;
  /** The vault file that fills it. */
  filledBy: string | null;
}

export interface Kit {
  id: string;
  profile: string;
  inputs: KitInput[];
  ready: boolean;
  /** The Studio can run the kit's build steps. */
  buildable: boolean;
  /** The kit can go back to stock, and the files that takes are here; null when it cannot. */
  restore: boolean | null;
  /** The kit makes debug builds too and keeps root on request. */
  variants: boolean;
  /** `fastboot`: partitions written over fastboot; `storage`: files for the player's own drive and a UPG. */
  set: "fastboot" | "storage";
}

export type RepoState =
  | { state: "unset" }
  | { state: "ready" | "missing" | "notGit" | "noCommit"; path: string }
  /** The kit comes from its repository's published archive, downloaded on the first build. */
  | { state: "published"; have: boolean; bytes: number };

/** A licence's text as its publisher keeps it, named by its sha256. */
/** What this computer's fastboot can do: write images, and also make the userdata image `-w` formats with. */
export type Flasher = "missing" | "writes" | "formats";

/** Google's platform-tools as the app keeps them, and the adb and fastboot it runs. */
export interface ToolsView {
  /** The revision downloaded, if any. */
  installed: string | null;
  /** The revision the app downloads, and its archive's size. */
  revision: string;
  bytes: number;
  agreed: boolean;
  fetching: { done: number; total: number } | null;
  adb: string | null;
  fastboot: string | null;
  flasher: Flasher;
}

export interface ToolsDone {
  view: ToolsView;
  error: ErrorReport | null;
  cancelled: boolean;
}

export interface LicenceText {
  id: string;
  text: string;
  sha256: string;
}

/** `stock`: Sony's firmware from the kit's restore entry, for going back. */
export type Variant = "release" | "debug" | "stock";

export interface BuildOptions {
  variant: Variant;
  /** Keep TrebleDroid's su in a release build. */
  root: boolean;
}

export type BuildOutcome =
  | { kind: "running" | "done" | "cancelled" | "interrupted" }
  | { kind: "failed"; step: string | null; tail: string[]; code: number | null };

export interface InstallSet {
  kit: string;
  commit: string | null;
  dirty: boolean;
  variant: Variant;
  root: boolean;
  model: string | null;
  firmware: string | null;
  series: string | null;
  slot: string;
  files: Record<string, { bytes: number; sha256: string }>;
  flash: { partition: string; file: string; flags: string[] }[];
  after: { kernel: string; check: { path: string; sha256: string }[]; absent: string[] };
}

/** A storage kit's install.json: files for the player's drive, taken in by a UPG its updater runs. */
export interface StorageSet {
  kit: string;
  commit: string | null;
  dirty: boolean;
  /** The Sony build the kit took the owner's files from. */
  firmware: string;
  files: Record<string, { bytes: number; sha256: string }>;
  /** The order the files go onto the drive, the UPG last. */
  copy: string[];
  /** Files a player that has one keeps. */
  keep: string[];
  upg: { file: string; key: "stock" | "w1"; recovery: string };
  after: { log: string; expect: string[] };
}

export interface BuildRecord {
  id: string;
  kit: string;
  commit: string;
  /** Seconds since the epoch. */
  started: number;
  ended: number | null;
  options: BuildOptions;
  inputs: { input: string; path: string; sha256: string }[];
  outcome: BuildOutcome;
  total: number;
  steps: { name: string; secs: number }[];
  set: InstallSet | null;
  /** A storage kit's set instead. */
  storage?: StorageSet;
  dir: string;
}

export interface BuildProgress {
  id: string | null;
  kit: string;
  options: BuildOptions;
  started: number;
  /** `fetch` while the kit's archive downloads, `copy` while its files come from a checkout. */
  phase: "fetch" | "copy" | "run";
  fetched: number;
  fetchTotal: number;
  index: number;
  total: number;
  step: string | null;
  /** ms since the epoch. */
  stepStarted: number;
  lines: string[];
}

export interface BuildDone {
  record: BuildRecord | null;
  error: ErrorReport | null;
}

export interface HostCheck {
  kind: "tool" | "python" | "java" | "ndk" | "sdk" | "space" | "adbKey";
  name: string;
  ok: boolean;
  found: string | null;
}

export interface StudioView {
  kits: Kit[];
  files: VaultFile[];
  /** Every file the Studio knows, for naming vault files no kit takes. */
  catalog: KnownFile[];
  /** The folder fetched files go to. */
  downloads: string;
  /** Each buildable kit's repository on this computer. */
  repos: Record<string, RepoState>;
  /** Per player family: the UPG key has been read from a player. */
  keys: Record<string, boolean>;
  /** Per kit with a licence: the owner agreed to it. */
  licences: Record<string, boolean>;
  builds: BuildRecord[];
  building: BuildProgress | null;
}

export interface StudioProgress {
  /** The path being looked at, or the id of the known file being fetched. */
  name: string;
  fetching: boolean;
  done: number;
  total: number;
  index: number;
  count: number;
}

export interface StudioDone {
  added: string[];
  /** `file` is the known file a failed download was for. */
  failed: { name: string; file?: string; error: ErrorReport }[];
  cancelled: boolean;
}

// Installing a build

export type InstallMode = "clean" | "update" | "restore";

export type Stage =
  | { kind: "rebootBootloader" | "unlock" | "wipe" | "reboot" | "waitBoot" | "waitAdb" | "verify" | "dexopt" }
  | { kind: "flash"; partition: string; file: string; flags: string[]; bytes: number }
  | { kind: "setActive"; slot: string }
  // A storage set onto an NW-A50's WALKMAN drive, then the UPG.
  | { kind: "copy"; file: string; bytes: number }
  | { kind: "keep"; file: string }
  | { kind: "readBack"; bytes: number }
  | { kind: "pickWalkman"; file: string }
  | { kind: "trigger" | "waitUpdate" | "checkLog" };

export interface Gate {
  id: "model" | "firmware" | "oemUnlock" | "battery" | "card" | "runsKit" | "sameVariant" | "unlocked" | "fastboot" | "drive" | "room" | "backup" | "elevate" | "hasData";
  /** null: the player cannot tell in its current mode. */
  ok: boolean | null;
  value: string | null;
  want: string | null;
}

export interface InstallPlan {
  mode: InstallMode;
  stages: Stage[];
  gates: Gate[];
  erases: boolean;
  /** The word the owner types to start. */
  confirm: string;
  ready: boolean;
}

export interface Facts {
  serial: string;
  profile: string | null;
  booted: boolean;
  vendorFirmware: string | null;
  apiLevel: number | null;
  slot: string | null;
  unlocked: boolean | null;
  oemUnlockAllowed: boolean | null;
  running: Variant | null;
  battery: number | null;
  card: boolean;
  fastboot: Flasher;
}

/** The WALKMAN drive an NW-A50 series player lends this computer. */
export interface WalkmanDrive {
  disk: string;
  block: string;
  serial: string | null;
  label: string | null;
  /** 0 while the player lends no medium. */
  bytes: number;
  mount: string | null;
}

/** What an NW-A50 and its drive say before a storage install. */
export interface DriveFacts {
  serial: string;
  profile: string | null;
  drive: WalkmanDrive | null;
  free: number | null;
  /** Bytes each of the set's paths already takes on the drive. */
  present: Record<string, number>;
  choice: string | null;
  backupOnDrive: string | null;
  backupBuilt: string | null;
  elevate: boolean;
}

export interface PlanView {
  plan: InstallPlan;
  facts: Facts | null;
  drive: DriveFacts | null;
  set: InstallSet | null;
  storage: StorageSet | null;
  build: BuildRecord;
  stock: string | null;
}

export type Seen = "fastboot" | "adb" | "unauthorized" | "usb" | "drive" | "gone";

/** A storage install's read-back and what the UPG logged. */
export interface DriveVerdict {
  files: { path: string; ok: boolean; got: string | null }[];
  /** The lines the kit expects in the UPG's log, and whether this run wrote each. */
  lines: [string, boolean][];
  upgDone: boolean | null;
}

export interface Verdict {
  slot: string | null;
  slotOk: boolean;
  kernelOk: boolean;
  kernel: string | null;
  files: { path: string; ok: boolean; got: string | null }[];
  absent: [string, boolean][];
  ok: boolean;
}

export interface InstallJob {
  build: string;
  device: string;
  serial: string;
  mode: InstallMode;
  variant: Variant;
  stages: Stage[];
  index: number;
  state: "running" | "done" | "unverified" | "failed" | "cancelled";
  started: number;
  stageStarted: number;
  progress: number | null;
  seen: Seen | null;
  waiting: boolean;
  /** Something reached the player's partitions; restarting before the end may not boot. */
  touched: boolean;
  error: ErrorReport | null;
  verdict: Verdict | null;
  drive: DriveVerdict | null;
  log: string[];
}
