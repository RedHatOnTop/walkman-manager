import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { asError, bridge } from "../api/bridge";
import type { Environment, ErrorReport, KnownDevice, Profile, Scan, Snapshot } from "../api/types";

interface AppData {
  ready: boolean;
  fatal: ErrorReport | null;
  scan: Scan;
  known: KnownDevice[];
  profiles: Profile[];
  env: Environment | null;
  rescan(): Promise<void>;
}

const DataContext = createContext<AppData | null>(null);

export function DataProvider({ children }: { children: ReactNode }) {
  const [ready, setReady] = useState(false);
  const [fatal, setFatal] = useState<ErrorReport | null>(null);
  const [scan, setScan] = useState<Scan>({ devices: [], issues: [] });
  const [known, setKnown] = useState<KnownDevice[]>([]);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [env, setEnv] = useState<Environment | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    (async () => {
      try {
        const b = await bridge();
        // Listen before the first read: a scan that lands in between is announced only once.
        let heard = false;
        const off = await b.onDevices((next) => {
          heard = true;
          setScan(next);
          b.known().then(setKnown, () => {});
        });
        if (disposed) {
          off();
          return;
        }
        unlisten = off;
        const [s, k, p, e] = await Promise.all([b.scan(), b.known(), b.profiles(), b.environment()]);
        if (disposed) return;
        if (!heard) setScan(s);
        setKnown(k);
        setProfiles(p);
        setEnv(e);
        setReady(true);
      } catch (e) {
        if (!disposed) setFatal(asError(e));
      }
    })();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const rescan = useCallback(async () => {
    const b = await bridge();
    setScan(await b.rescan());
    setKnown(await b.known());
  }, []);

  return (
    <DataContext.Provider value={{ ready, fatal, scan, known, profiles, env, rescan }}>{children}</DataContext.Provider>
  );
}

export function useData(): AppData {
  const ctx = useContext(DataContext);
  if (!ctx) throw new Error("useData outside DataProvider");
  return ctx;
}

export function useProfile(id: string | null | undefined): Profile | undefined {
  const { profiles } = useData();
  return profiles.find((p) => p.id === id);
}

const FOCUS_POLL_MS = 4000;
const BACKGROUND_POLL_MS = 15000;

export interface Reading {
  snapshot: Snapshot | null;
  error: ErrorReport | null;
  loading: boolean;
}

interface Readings {
  readings: Record<string, Reading>;
  refresh(id: string): void;
  setFocus(id: string | null): void;
}

const ReadingsContext = createContext<Readings | null>(null);

/**
 * Reads every running player: the one on screen often, the rest occasionally,
 * so the sidebar and the overview always have battery and storage at hand.
 * A failed read keeps the last good snapshot next to the error.
 */
export function ReadingsProvider({ children }: { children: ReactNode }) {
  const { scan } = useData();
  const [readings, setReadings] = useState<Record<string, Reading>>({});
  const [focus, setFocus] = useState<string | null>(null);
  const inFlight = useRef(new Set<string>());
  const lastRead = useRef(new Map<string, number>());

  const running = scan.devices.filter((d) => d.mode === "running").map((d) => d.id);
  const runningKey = running.join("|");

  const read = useCallback(async (id: string) => {
    if (inFlight.current.has(id)) return;
    inFlight.current.add(id);
    lastRead.current.set(id, Date.now());
    setReadings((r) => ({ ...r, [id]: { snapshot: r[id]?.snapshot ?? null, error: r[id]?.error ?? null, loading: true } }));
    try {
      const b = await bridge();
      const snap = await b.snapshot(id);
      setReadings((r) => ({ ...r, [id]: { snapshot: snap, error: null, loading: false } }));
    } catch (e) {
      setReadings((r) => ({ ...r, [id]: { snapshot: r[id]?.snapshot ?? null, error: asError(e), loading: false } }));
    } finally {
      inFlight.current.delete(id);
    }
  }, []);

  useEffect(() => {
    const ids = runningKey ? runningKey.split("|") : [];
    for (const id of ids) if (!lastRead.current.has(id)) read(id);
    for (const id of [...lastRead.current.keys()]) if (!ids.includes(id)) lastRead.current.delete(id);
    const timer = window.setInterval(() => {
      if (document.visibilityState !== "visible") return;
      const now = Date.now();
      for (const id of ids) {
        const every = id === focus ? FOCUS_POLL_MS : BACKGROUND_POLL_MS;
        if (now - (lastRead.current.get(id) ?? 0) >= every - 100) read(id);
      }
    }, 1000);
    return () => window.clearInterval(timer);
  }, [runningKey, focus, read]);

  const value = useMemo(() => ({ readings, refresh: read, setFocus }), [readings, read]);
  return <ReadingsContext.Provider value={value}>{children}</ReadingsContext.Provider>;
}

export function useReadings(): Readings {
  const ctx = useContext(ReadingsContext);
  if (!ctx) throw new Error("useReadings outside ReadingsProvider");
  return ctx;
}

export function useReading(id: string | null | undefined): Reading {
  const { readings } = useReadings();
  return (id && readings[id]) || { snapshot: null, error: null, loading: false };
}
