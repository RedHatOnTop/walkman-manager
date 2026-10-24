import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { asError, bridge, type AnalysisDone, type SensmeDone } from "../api/bridge";
import type {
  Album,
  DeviceSummary,
  ErrorReport,
  LibraryState,
  PlayerLibrary,
  ScanProgress,
  Snapshot,
  SyncProgress,
  SyncReport,
  TagField,
  Track,
  VolumeMusic,
} from "../api/types";
import { useT } from "../i18n";
import { useData, useReadings } from "./data";
import { displayName } from "./describe";
import { presenceMap, type PresenceMap } from "./music";

export interface PlayerMusic {
  volumes: VolumeMusic[] | null;
  error: ErrorReport | null;
  loading: boolean;
}

export interface ActiveSync {
  planId: string;
  deviceId: string;
  progress: SyncProgress | null;
  report: SyncReport | null;
}

/** The music on one player read as a library of albums. */
export interface Shelf {
  library: PlayerLibrary | null;
  tracks: Map<string, Track>;
  error: ErrorReport | null;
  loading: boolean;
}

export interface ActivePull extends ActiveSync {
  destRoot: string;
}

/** Looked-up tags being written into one album's files. */
export interface ActiveRetag {
  id: string;
  /** null for the computer's files. */
  deviceId: string | null;
  album: Album;
  progress: SyncProgress | null;
  report: SyncReport | null;
  /** Player only: the folder that holds the originals. */
  backup: string | null;
}

/** SensMe analysis being made and written into the files of the library or a player. */
export interface ActiveAnalysis {
  planId: string;
  /** null for the computer's files. */
  deviceId: string | null;
  progress: SyncProgress | null;
  result: AnalysisDone | null;
}

/** Tags being read off a player for SensMe data. */
export interface SensmeReading {
  deviceId: string;
  done: number;
  total: number;
  result: SensmeDone | null;
}

interface Library {
  state: LibraryState | null;
  error: ErrorReport | null;
  scanning: boolean;
  progress: ScanProgress | null;
  tracks: Map<string, Track>;
  addRoot(): Promise<void>;
  removeRoot(path: string): Promise<void>;
  rescan(): void;
  cancelScan(): void;
  artUrl(key: string): string;

  music: Record<string, PlayerMusic>;
  loadMusic(deviceId: string): void;

  selected: Set<string>;
  toggle(albumId: string): void;
  select(albumIds: string[]): void;
  clear(): void;

  sync: ActiveSync | null;
  startSync(planId: string, deviceId: string): Promise<void>;
  cancelSync(): void;
  dismissSync(): void;

  shelves: Record<string, Shelf>;
  loadShelf(deviceId: string): void;
  pull: ActivePull | null;
  startPull(planId: string, deviceId: string, destRoot: string): Promise<void>;
  cancelPull(): void;
  dismissPull(): void;

  retag: ActiveRetag | null;
  startRetag(id: string, deviceId: string | null, album: Album, albumFields: TagField[], tracks: string[], cover: boolean): Promise<void>;
  cancelRetag(): void;
  dismissRetag(): void;

  /** Asks for the firmware file; resolves false when the picker was closed. */
  importSensme(): Promise<boolean>;
  sensme: SensmeReading | null;
  readSensme(deviceId: string): Promise<void>;
  cancelSensme(): void;

  analysis: ActiveAnalysis | null;
  startAnalysis(planId: string, deviceId: string | null): Promise<void>;
  cancelAnalysis(): void;
  dismissAnalysis(): void;
}

const NO_TRACKS = new Map<string, Track>();

const LibraryContext = createContext<Library | null>(null);

export function LibraryProvider({ children }: { children: ReactNode }) {
  const { scan } = useData();
  const [state, setState] = useState<LibraryState | null>(null);
  const [error, setError] = useState<ErrorReport | null>(null);
  const [scanning, setScanning] = useState(false);
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const [music, setMusic] = useState<Record<string, PlayerMusic>>({});
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [sync, setSync] = useState<ActiveSync | null>(null);
  const [shelves, setShelves] = useState<Record<string, Shelf>>({});
  const [pull, setPull] = useState<ActivePull | null>(null);
  const [retag, setRetag] = useState<ActiveRetag | null>(null);
  const [sensme, setSensme] = useState<SensmeReading | null>(null);
  const [analysis, setAnalysis] = useState<ActiveAnalysis | null>(null);
  const [artUrl, setArtUrl] = useState<(key: string) => string>(() => () => "");
  const loadingMusic = useRef(new Set<string>());
  const loadingShelf = useRef(new Set<string>());

  const reload = useCallback(async () => {
    try {
      const b = await bridge();
      const s = await b.library();
      setState(s);
      setScanning(s.scanning);
      setError(null);
    } catch (e) {
      setError(asError(e));
    }
  }, []);

  const loadMusic = useCallback(async (deviceId: string) => {
    if (loadingMusic.current.has(deviceId)) return;
    loadingMusic.current.add(deviceId);
    setMusic((m) => ({ ...m, [deviceId]: { volumes: m[deviceId]?.volumes ?? null, error: null, loading: true } }));
    try {
      const b = await bridge();
      const volumes = await b.deviceMusic(deviceId);
      setMusic((m) => ({ ...m, [deviceId]: { volumes, error: null, loading: false } }));
    } catch (e) {
      setMusic((m) => ({ ...m, [deviceId]: { volumes: m[deviceId]?.volumes ?? null, error: asError(e), loading: false } }));
    } finally {
      loadingMusic.current.delete(deviceId);
    }
  }, []);

  const loadShelf = useCallback(async (deviceId: string) => {
    if (loadingShelf.current.has(deviceId)) return;
    loadingShelf.current.add(deviceId);
    setShelves((m) => ({ ...m, [deviceId]: { ...(m[deviceId] ?? { library: null, tracks: NO_TRACKS }), error: null, loading: true } }));
    try {
      const library = await (await bridge()).playerLibrary(deviceId);
      const tracks = new Map(library.index.tracks.map((t) => [t.id, t]));
      setShelves((m) => ({ ...m, [deviceId]: { library, tracks, error: null, loading: false } }));
    } catch (e) {
      setShelves((m) => ({ ...m, [deviceId]: { ...(m[deviceId] ?? { library: null, tracks: NO_TRACKS }), error: asError(e), loading: false } }));
    } finally {
      loadingShelf.current.delete(deviceId);
    }
  }, []);
  const shelfIds = useRef<string[]>([]);
  shelfIds.current = Object.keys(shelves);

  useEffect(() => {
    const offs: (() => void)[] = [];
    let disposed = false;
    (async () => {
      const b = await bridge();
      setArtUrl(() => b.artUrl);
      const subs = await Promise.all([
        b.onLibraryScan(({ running, progress }) => {
          setScanning(running);
          setProgress(running ? progress : null);
        }),
        b.onLibrary(() => reload()),
        b.onSyncProgress(({ planId, progress }) =>
          setSync((s) => (s && s.planId === planId ? { ...s, progress } : s)),
        ),
        b.onSyncDone(({ planId, deviceId, report }) => {
          setSync((s) => (s && s.planId === planId ? { ...s, report } : s));
          loadMusic(deviceId);
          if (shelfIds.current.includes(deviceId)) loadShelf(deviceId);
        }),
        b.onPullProgress(({ planId, progress }) =>
          setPull((s) => (s && s.planId === planId ? { ...s, progress } : s)),
        ),
        b.onPullDone(({ planId, report }) => setPull((s) => (s && s.planId === planId ? { ...s, report } : s))),
        b.onRetagProgress(({ id, progress }) => setRetag((s) => (s && s.id === id ? { ...s, progress } : s))),
        b.onRetagDone(({ id, deviceId, report, backup }) => {
          setRetag((s) => (s && s.id === id ? { ...s, report, backup } : s));
          if (deviceId && shelfIds.current.includes(deviceId)) loadShelf(deviceId);
        }),
        b.onSensmeProgress(({ deviceId, done, total }) =>
          setSensme((s) => (s && s.deviceId === deviceId ? { ...s, done, total } : s)),
        ),
        b.onSensmeDone((result) => {
          setSensme((s) => (s && s.deviceId === result.deviceId ? { ...s, result } : s));
          if (shelfIds.current.includes(result.deviceId)) loadShelf(result.deviceId);
        }),
        b.onAnalysisProgress(({ planId, progress }) => setAnalysis((s) => (s && s.planId === planId ? { ...s, progress } : s))),
        b.onAnalysisDone((result) => {
          setAnalysis((s) => (s && s.planId === result.planId ? { ...s, result } : s));
          if (result.deviceId && shelfIds.current.includes(result.deviceId)) loadShelf(result.deviceId);
        }),
      ]);
      if (disposed) subs.forEach((off) => off());
      else offs.push(...subs);
      // Read after listening, so a scan that finishes in between is not missed.
      if (!disposed) await reload();
    })();
    return () => {
      disposed = true;
      offs.forEach((off) => off());
    };
  }, [reload, loadMusic, loadShelf]);

  // Listings of players that went away are dropped, so a replug reads them again.
  const runningKey = scan.devices
    .filter((d) => d.mode === "running")
    .map((d) => d.id)
    .join("|");
  useEffect(() => {
    const ids = new Set(runningKey ? runningKey.split("|") : []);
    setMusic((m) => {
      const kept = Object.fromEntries(Object.entries(m).filter(([id]) => ids.has(id)));
      return Object.keys(kept).length === Object.keys(m).length ? m : kept;
    });
    setShelves((m) => {
      const kept = Object.fromEntries(Object.entries(m).filter(([id]) => ids.has(id)));
      return Object.keys(kept).length === Object.keys(m).length ? m : kept;
    });
  }, [runningKey]);

  const tracks = useMemo(() => new Map((state?.index.tracks ?? []).map((t) => [t.id, t])), [state]);

  const value = useMemo<Library>(
    () => ({
      state,
      error,
      scanning,
      progress,
      tracks,
      addRoot: async () => {
        try {
          const s = await (await bridge()).addRoot();
          setState(s);
          setScanning(s.scanning);
        } catch (e) {
          setError(asError(e));
        }
      },
      removeRoot: async (path) => {
        try {
          const s = await (await bridge()).removeRoot(path);
          setState(s);
          setScanning(s.scanning);
        } catch (e) {
          setError(asError(e));
        }
      },
      rescan: () => {
        setScanning(true);
        bridge().then((b) => b.rescanLibrary());
      },
      cancelScan: () => {
        bridge().then((b) => b.cancelLibraryScan());
      },
      artUrl,
      music,
      loadMusic,
      selected,
      toggle: (id) =>
        setSelected((s) => {
          const next = new Set(s);
          if (!next.delete(id)) next.add(id);
          return next;
        }),
      select: (ids) => setSelected((s) => new Set([...s, ...ids])),
      clear: () => setSelected(new Set()),
      sync,
      startSync: async (planId, deviceId) => {
        setSync({ planId, deviceId, progress: null, report: null });
        try {
          await (await bridge()).syncStart(planId);
        } catch (e) {
          setSync(null);
          throw e;
        }
      },
      cancelSync: () => {
        bridge().then((b) => b.syncCancel());
      },
      dismissSync: () => setSync(null),
      shelves,
      loadShelf,
      pull,
      startPull: async (planId, deviceId, destRoot) => {
        setPull({ planId, deviceId, destRoot, progress: null, report: null });
        try {
          await (await bridge()).pullStart(planId);
        } catch (e) {
          setPull(null);
          throw e;
        }
      },
      cancelPull: () => {
        bridge().then((b) => b.pullCancel());
      },
      dismissPull: () => setPull(null),
      retag,
      startRetag: async (id, deviceId, album, albumFields, trackIds, cover) => {
        setRetag({ id, deviceId, album, progress: null, report: null, backup: null });
        try {
          await (await bridge()).retagStart(id, albumFields, trackIds, cover);
        } catch (e) {
          setRetag(null);
          throw e;
        }
      },
      cancelRetag: () => {
        bridge().then((b) => b.syncCancel());
      },
      dismissRetag: () => setRetag(null),
      importSensme: async () => {
        const picked = await (await bridge()).sensmeImport();
        // Shelves already read are worked out again with the new table.
        if (picked) for (const id of shelfIds.current) loadShelf(id);
        return picked;
      },
      sensme,
      readSensme: async (deviceId) => {
        setSensme({ deviceId, done: 0, total: 0, result: null });
        try {
          await (await bridge()).sensmeRead(deviceId);
        } catch (e) {
          setSensme(null);
          throw e;
        }
      },
      cancelSensme: () => {
        bridge().then((b) => b.syncCancel());
      },
      analysis,
      startAnalysis: async (planId, deviceId) => {
        setAnalysis({ planId, deviceId, progress: null, result: null });
        try {
          await (await bridge()).analysisStart(planId);
        } catch (e) {
          setAnalysis(null);
          throw e;
        }
      },
      cancelAnalysis: () => {
        bridge().then((b) => b.syncCancel());
      },
      dismissAnalysis: () => setAnalysis(null),
    }),
    [state, error, scanning, progress, tracks, artUrl, music, loadMusic, selected, sync, shelves, loadShelf, pull, retag, sensme, analysis],
  );

  return <LibraryContext.Provider value={value}>{children}</LibraryContext.Provider>;
}

export function useLibrary(): Library {
  const ctx = useContext(LibraryContext);
  if (!ctx) throw new Error("useLibrary outside LibraryProvider");
  return ctx;
}

export interface MusicPlayer {
  device: DeviceSummary;
  snapshot: Snapshot | null;
  name: string;
  /** "A55", "ZX507": the model without Sony's NW- prefix, ASCII for mono type. */
  short: string;
  music: PlayerMusic | undefined;
  presence: PresenceMap | null;
}

/** Running players that take music, with what is on them; listings load on first use. */
export function useMusicPlayers(): MusicPlayer[] {
  const { scan } = useData();
  const { readings } = useReadings();
  const { music, loadMusic } = useLibrary();
  const t = useT();
  const running = scan.devices.filter((d) => d.mode === "running" && d.profileId);
  const key = running.map((d) => d.id).join("|");

  useEffect(() => {
    for (const id of key ? key.split("|") : []) if (!music[id]) loadMusic(id);
  }, [key, music, loadMusic]);

  const presence = useMemo(() => {
    const out: Record<string, PresenceMap> = {};
    for (const [id, m] of Object.entries(music)) if (m.volumes) out[id] = presenceMap(m.volumes);
    return out;
  }, [music]);

  return running.map((device) => {
    const snapshot = readings[device.id]?.snapshot ?? null;
    const name = displayName(device, snapshot, t("device.unknownModel"));
    return {
      device,
      snapshot,
      name,
      short: name.replace(/^NW-/, ""),
      music: music[device.id],
      presence: presence[device.id] ?? null,
    };
  });
}
