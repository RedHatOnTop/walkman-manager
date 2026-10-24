import type {
  AnalysisPlan,
  BuildDone,
  BuildOptions,
  BuildProgress,
  EngineView,
  Environment,
  ErrorReport,
  HostCheck,
  InstallJob,
  InstallMode,
  KnownDevice,
  LicenceText,
  LibraryState,
  LookupQuery,
  Plan,
  PlanView,
  PlayerLibrary,
  Profile,
  Proposal,
  PullPlan,
  Scan,
  ScanProgress,
  SearchResult,
  Snapshot,
  StudioDone,
  StudioProgress,
  StudioView,
  SyncProgress,
  SyncReport,
  TagField,
  ToolsDone,
  ToolsView,
  Volume,
  VolumeMusic,
} from "./types";
import type { Lang } from "../i18n/strings";

type Unlisten = () => void;

export interface Bridge {
  environment(): Promise<Environment>;
  profiles(): Promise<Profile[]>;
  scan(): Promise<Scan>;
  rescan(): Promise<Scan>;
  known(): Promise<KnownDevice[]>;
  snapshot(id: string): Promise<Snapshot>;
  onDevices(listener: (scan: Scan) => void): Promise<Unlisten>;

  library(): Promise<LibraryState>;
  addRoot(): Promise<LibraryState>;
  removeRoot(path: string): Promise<LibraryState>;
  rescanLibrary(): Promise<void>;
  cancelLibraryScan(): Promise<void>;
  onLibraryScan(listener: (e: { running: boolean; progress: ScanProgress }) => void): Promise<Unlisten>;
  onLibrary(listener: () => void): Promise<Unlisten>;
  /** URL of a cached cover thumbnail. */
  artUrl(key: string): string;

  deviceMusic(id: string): Promise<VolumeMusic[]>;
  syncPlan(id: string, volume: Volume["kind"], tracks: string[]): Promise<Plan>;
  syncStart(planId: string): Promise<void>;
  syncCancel(): Promise<void>;
  onSyncProgress(listener: (e: { planId: string; progress: SyncProgress }) => void): Promise<Unlisten>;
  onSyncDone(listener: (e: { planId: string; deviceId: string; report: SyncReport }) => void): Promise<Unlisten>;

  playerLibrary(id: string): Promise<PlayerLibrary>;
  pullPlan(id: string, tracks: string[]): Promise<PullPlan>;
  pullStart(planId: string): Promise<void>;
  /** One transfer runs at a time, so this stops a send as well. */
  pullCancel(): Promise<void>;
  onPullProgress(listener: (e: { planId: string; progress: SyncProgress }) => void): Promise<Unlisten>;
  onPullDone(listener: (e: { planId: string; deviceId: string; destRoot: string; report: SyncReport }) => void): Promise<Unlisten>;

  lookupSearch(query: LookupQuery): Promise<SearchResult>;
  /** Art key of a search result's cover, or null when it has none. */
  lookupCover(id: string): Promise<string | null>;
  lookupPropose(candidate: string, album: string, device: string | null): Promise<Proposal>;
  retagStart(id: string, albumFields: TagField[], tracks: string[], cover: boolean): Promise<void>;
  retagForget(id: string): Promise<void>;
  onRetagProgress(listener: (e: { id: string; progress: SyncProgress }) => void): Promise<Unlisten>;
  onRetagDone(listener: (e: { id: string; deviceId: string | null; report: SyncReport; backup: string | null }) => void): Promise<Unlisten>;

  /** Asks for the firmware file holding the SensMe table; false when the picker was closed. */
  sensmeImport(): Promise<boolean>;
  sensmeRead(id: string): Promise<void>;
  onSensmeProgress(listener: (e: { deviceId: string; done: number; total: number }) => void): Promise<Unlisten>;
  onSensmeDone(listener: (e: SensmeDone) => void): Promise<Unlisten>;

  analysisEngine(): Promise<EngineView>;
  /** Asks for Music Center's MMLib11.dll; null when the picker was closed. */
  analysisPickEngine(): Promise<EngineView | null>;
  analysisPlan(device: string | null): Promise<AnalysisPlan>;
  analysisStart(planId: string): Promise<void>;
  onAnalysisProgress(listener: (e: { planId: string; progress: SyncProgress }) => void): Promise<Unlisten>;
  onAnalysisDone(listener: (e: AnalysisDone) => void): Promise<Unlisten>;

  studioView(): Promise<StudioView>;
  /** Looks at the files and keeps them in the vault; progress and the result come as events. */
  studioAdd(paths: string[]): Promise<void>;
  /** False when the picker was closed. */
  studioPick(): Promise<boolean>;
  /** Downloads a known file from its publisher. */
  studioFetch(file: string): Promise<void>;
  studioRemove(id: string): Promise<StudioView>;
  studioCancel(): Promise<void>;
  studioOpenGuide(file: string, lang: Lang): Promise<void>;
  onStudioProgress(listener: (e: StudioProgress) => void): Promise<Unlisten>;
  onStudioChanged(listener: () => void): Promise<Unlisten>;
  onStudioDone(listener: (e: StudioDone) => void): Promise<Unlisten>;
  studioOpenInputGuide(kit: string, input: string, lang: Lang): Promise<void>;
  /** Asks for the folder of the kit's repository. */
  studioPickRepo(kit: string): Promise<StudioView>;
  /** Reads the UPG key from a connected player. */
  studioReadKey(device: string): Promise<StudioView>;
  studioHost(kit: string): Promise<{ checks: HostCheck[]; ok: boolean }>;
  /** The licence the kit's downloads come under, read from its publisher. */
  studioLicence(kit: string): Promise<LicenceText>;
  /** Agrees to the text `studioLicence` returned, named by its sha256. */
  studioAgree(kit: string, sha256: string): Promise<StudioView>;
  studioBuild(kit: string, options: BuildOptions): Promise<void>;
  studioBuildCancel(): Promise<void>;
  studioOpenBuild(id: string): Promise<void>;
  studioForgetBuild(id: string): Promise<StudioView>;
  onStudioBuild(listener: (p: BuildProgress | null) => void): Promise<Unlisten>;
  onStudioBuildDone(listener: (d: BuildDone) => void): Promise<Unlisten>;

  installPlan(build: string, device: string, mode: InstallMode): Promise<PlanView>;
  installState(): Promise<InstallJob | null>;
  installStart(build: string, device: string, mode: InstallMode, confirm: string): Promise<void>;
  /** Stops at the next point where stopping leaves the player whole. */
  installCancel(): Promise<void>;
  /** After a clean install: do not wait for USB debugging. */
  installSkip(): Promise<void>;
  installClear(): Promise<void>;
  installLeave(): Promise<void>;
  onInstall(listener: (j: InstallJob) => void): Promise<Unlisten>;

  toolsView(): Promise<ToolsView>;
  /** The licence Google's platform-tools come under, read from Google's SDK repository. */
  toolsLicence(): Promise<LicenceText>;
  toolsAgree(sha256: string): Promise<ToolsView>;
  /** Downloads the platform-tools; progress and the end come as events. */
  toolsFetch(): Promise<void>;
  toolsCancel(): Promise<void>;
  onToolsProgress(listener: (p: { done: number; total: number }) => void): Promise<Unlisten>;
  onToolsDone(listener: (d: ToolsDone) => void): Promise<Unlisten>;
  /** Paths of files dropped on the window; null when the shell has no drop support. */
  onFileDrop(listener: (e: { over: boolean; paths: string[] }) => void): Promise<Unlisten>;
}

export interface AnalysisDone {
  planId: string;
  deviceId: string | null;
  report: SyncReport;
  /** Player only: the folder that holds the originals. */
  backup: string | null;
}

export interface SensmeDone {
  deviceId: string;
  read: number;
  analysed: number;
  cancelled: boolean;
  error: ErrorReport | null;
}

export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function tauriBridge(): Promise<Bridge> {
  const { invoke, convertFileSrc } = await import("@tauri-apps/api/core");
  const { listen } = await import("@tauri-apps/api/event");
  const on =
    <T,>(event: string) =>
    async (listener: (payload: T) => void) =>
      listen<T>(event, (e) => listener(e.payload));
  return {
    environment: () => invoke("environment"),
    profiles: () => invoke("profiles"),
    scan: () => invoke("scan_devices"),
    rescan: () => invoke("rescan_devices"),
    known: () => invoke("known_devices"),
    snapshot: (id) => invoke("device_snapshot", { id }),
    onDevices: on<Scan>("devices"),

    library: () => invoke("library_state"),
    addRoot: () => invoke("library_add_root"),
    removeRoot: (path) => invoke("library_remove_root", { path }),
    rescanLibrary: () => invoke("library_rescan"),
    cancelLibraryScan: () => invoke("library_cancel_scan"),
    onLibraryScan: on("library-scan"),
    onLibrary: on("library"),
    artUrl: (key) => convertFileSrc(key, "art"),

    deviceMusic: (id) => invoke("device_music", { id }),
    syncPlan: (id, volume, tracks) => invoke("sync_plan", { id, volume, tracks }),
    syncStart: (planId) => invoke("sync_start", { planId }),
    syncCancel: () => invoke("sync_cancel"),
    onSyncProgress: on("sync-progress"),
    onSyncDone: on("sync-done"),

    playerLibrary: (id) => invoke("player_library", { id }),
    pullPlan: (id, tracks) => invoke("pull_plan", { id, tracks }),
    pullStart: (planId) => invoke("pull_start", { planId }),
    pullCancel: () => invoke("sync_cancel"),
    onPullProgress: on("pull-progress"),
    onPullDone: on("pull-done"),

    lookupSearch: (query) => invoke("lookup_search", { query }),
    lookupCover: (id) => invoke("lookup_cover", { id }),
    lookupPropose: (candidate, album, device) => invoke("lookup_propose", { candidate, album, device }),
    retagStart: (id, albumFields, tracks, cover) => invoke("retag_start", { id, albumFields, tracks, cover }),
    retagForget: (id) => invoke("retag_forget", { id }),
    onRetagProgress: on("retag-progress"),
    onRetagDone: on("retag-done"),

    sensmeImport: () => invoke("sensme_import"),
    sensmeRead: (id) => invoke("sensme_read", { id }),
    onSensmeProgress: on("sensme-progress"),
    onSensmeDone: on("sensme-done"),

    analysisEngine: () => invoke("analysis_engine"),
    analysisPickEngine: () => invoke("analysis_pick_engine"),
    analysisPlan: (device) => invoke("analysis_plan", { device }),
    analysisStart: (planId) => invoke("analysis_start", { planId }),
    onAnalysisProgress: on("analysis-progress"),
    onAnalysisDone: on("analysis-done"),

    studioView: () => invoke("studio_view"),
    studioAdd: (paths) => invoke("studio_add", { paths }),
    studioPick: () => invoke("studio_pick"),
    studioFetch: (file) => invoke("studio_fetch", { file }),
    studioRemove: (id) => invoke("studio_remove", { id }),
    studioCancel: () => invoke("studio_cancel"),
    studioOpenGuide: (file, lang) => invoke("studio_open_guide", { file, lang }),
    onStudioProgress: on("studio-progress"),
    onStudioChanged: on("studio-changed"),
    onStudioDone: on("studio-done"),
    studioOpenInputGuide: (kit, input, lang) => invoke("studio_open_input_guide", { kit, input, lang }),
    studioPickRepo: (kit) => invoke("studio_pick_repo", { kit }),
    studioReadKey: (device) => invoke("studio_read_key", { device }),
    studioHost: (kit) => invoke("studio_host", { kit }),
    studioLicence: (kit) => invoke("studio_licence", { kit }),
    studioAgree: (kit, sha256) => invoke("studio_agree", { kit, sha256 }),
    studioBuild: (kit, options) => invoke("studio_build", { kit, options }),
    studioBuildCancel: () => invoke("studio_build_cancel"),
    studioOpenBuild: (id) => invoke("studio_open_build", { id }),
    studioForgetBuild: (id) => invoke("studio_forget_build", { id }),
    onStudioBuild: on("studio-build"),
    onStudioBuildDone: on("studio-build-done"),
    installPlan: (build, device, mode) => invoke("install_plan", { build, device, mode }),
    installState: () => invoke("install_state"),
    installStart: (build, device, mode, confirm) => invoke("install_start", { build, device, mode, confirm }),
    installCancel: () => invoke("install_cancel"),
    installSkip: () => invoke("install_skip"),
    installClear: () => invoke("install_clear"),
    installLeave: () => invoke("install_leave"),
    onInstall: on("install"),

    toolsView: () => invoke("tools_view"),
    toolsLicence: () => invoke("tools_licence"),
    toolsAgree: (sha256) => invoke("tools_agree", { sha256 }),
    toolsFetch: () => invoke("tools_fetch"),
    toolsCancel: () => invoke("tools_cancel"),
    onToolsProgress: on("tools-progress"),
    onToolsDone: on("tools-done"),
    onFileDrop: async (listener) => {
      const { getCurrentWebview } = await import("@tauri-apps/api/webview");
      return getCurrentWebview().onDragDropEvent((e) => {
        const p = e.payload;
        if (p.type === "enter" || p.type === "over") listener({ over: true, paths: p.type === "enter" ? p.paths : [] });
        else if (p.type === "drop") listener({ over: false, paths: p.paths });
        else listener({ over: false, paths: [] });
      });
    },
  };
}

let instance: Promise<Bridge> | null = null;

/** The Tauri backend in the app; recorded device data in a plain browser during development. */
export function bridge(): Promise<Bridge> {
  if (!instance) {
    instance = inTauri
      ? tauriBridge()
      : import.meta.env.DEV
        ? import("./mock").then((m) => m.mockBridge())
        : Promise.reject(new Error("Ultimate Walkman Manager must run inside its desktop shell."));
  }
  return instance;
}

export function asError(e: unknown): ErrorReport {
  if (e && typeof e === "object" && "message" in e) {
    const r = e as Partial<Record<keyof ErrorReport, unknown>>;
    return {
      kind: typeof r.kind === "string" ? r.kind : "unknown",
      message: String(r.message),
      ...(typeof r.say === "string" ? { say: r.say } : {}),
      ...(typeof r.vars === "object" && r.vars !== null ? { vars: r.vars as Record<string, string> } : {}),
      ...(typeof r.detail === "string" ? { detail: r.detail } : {}),
      ...(typeof r.kind !== "string" ? { detail: String(r.message) } : {}),
    };
  }
  return { kind: "unknown", message: String(e), detail: String(e) };
}
