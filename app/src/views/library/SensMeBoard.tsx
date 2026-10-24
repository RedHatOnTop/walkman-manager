import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { asError, bridge } from "../../api/bridge";
import type { Album, AnalysisPlan, EngineView, ErrorReport, SensMe, Track } from "../../api/types";
import { Cover } from "../../components/Cover";
import { Icon } from "../../components/Icon";
import { bytes, percent } from "../../format";
import { usePrefs, useT, type StringKey } from "../../i18n";
import { useLibrary, type MusicPlayer } from "../../state/library";
import { codecName, duration } from "../../state/music";
import { ErrorText } from "../../components/ErrorText";

const TIME = [0, 1, 2, 3, 4];
const MOOD = [5, 6, 7, 8, 9, 10, 11, 12];
/** Rows added per step as a channel's list scrolls. */
const PAGE = 150;
/** Files the analysis can be written into, as the backend tells them apart: by name. */
const ANALYSABLE = /\.(mp[123]|flac)$/i;
/** The tempo bar's scale; the Walkman clamps its tempo axis to the same range. */
const BPM_LOW = 60;
const BPM_HIGH = 180;

/** Line drawings for the channels, on a 48 unit square. */
const GLYPHS: Record<number, ReactNode> = {
  0: <path d="M6 32h36M14 32a10 10 0 0 1 20 0M24 13v5M12.5 18.5l3 3M35.5 18.5l-3 3" />,
  1: (
    <>
      <circle cx="24" cy="24" r="8" />
      <path d="M24 7v5M24 36v5M7 24h5M36 24h5M12 12l3.5 3.5M32.5 32.5 36 36M12 36l3.5-3.5M32.5 15.5 36 12" />
    </>
  ),
  2: <path d="M6 30h36M12 36h24M18 42h12M15 30a9 9 0 0 1 18 0" />,
  3: <path d="M30 9a15 15 0 1 0 9 24 12 12 0 0 1-9-24z" />,
  4: <path d="M25 12a13 13 0 1 0 9 20 10 10 0 0 1-9-20zM37 8v6M34 11h6M41 21v3M39.5 22.5h3" />,
  5: <path d="M27 5 12 27h11l-3 16 16-23H25l2-15z" />,
  6: <path d="M5 19c4.75-4.5 9.5-4.5 14.25 0s9.5 4.5 14.25 0 6.75-3 9.5-1.5M5 30c4.75-4.5 9.5-4.5 14.25 0s9.5 4.5 14.25 0 6.75-3 9.5-1.5" />,
  7: (
    <>
      <circle cx="24" cy="24" r="7" />
      <circle cx="24" cy="24" r="15" strokeDasharray="2.5 4.5" />
    </>
  ),
  8: <path d="m8 36 10-10 7 6 13-16M30 16h8v8" />,
  9: <path d="M24 39S10 30.5 10 20.5a7.5 7.5 0 0 1 14-3.75 7.5 7.5 0 0 1 14 3.75C38 30.5 24 39 24 39z" />,
  10: <path d="M11 9h26L24 24 11 9zM24 24v13M17 39h14M30 9l5-5" />,
  11: (
    <>
      <circle cx="24" cy="24" r="16" />
      <circle cx="24" cy="24" r="4" />
      <path d="M24 12a12 12 0 0 1 12 12M24 16a8 8 0 0 1 8 8" />
    </>
  ),
  12: <path d="M4 26h6l4-14 6 24 6-30 6 32 4-16h8" />,
};

function channelKey(kind: "ch" | "gloss", ch: number): StringKey {
  return `sensme.${kind}.${ch}` as StringKey;
}

function Glyph({ ch, size = 28 }: { ch: number; size?: number }) {
  return (
    <svg className="sm-glyph" width={size} height={size} viewBox="0 0 48 48" fill="none" stroke="currentColor" strokeWidth={2.4} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {GLYPHS[ch]}
    </svg>
  );
}

/** Tempo, chorus and channels of one track, for the album sheet's track rows. */
export function SensMeMarks({ sensme }: { sensme: SensMe }) {
  const t = useT();
  return (
    <span className="sm-marks">
      {sensme.channels?.map((ch) => (
        <span key={ch} className={`sm-dot ch-${ch}`} title={`${t(channelKey("ch", ch))} · ${t(channelKey("gloss", ch))}`} />
      ))}
      {sensme.bpm != null && <span className="mono num">{t("sensme.bpm", { n: Math.round(sensme.bpm) })}</span>}
    </span>
  );
}

function covers(list: Track[], albumOf: Map<string, Album>, n: number): { key: string; art: string | null; title: string }[] {
  const seen = new Set<string>();
  const out = [];
  for (const tr of list) {
    const album = albumOf.get(tr.id);
    const key = album?.id ?? tr.id;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push({ key, art: album?.art ?? tr.cover ?? null, title: album?.title ?? tr.title ?? "" });
    if (out.length === n) break;
  }
  return out;
}

/** A channel as a line of the sheet: its mark, name and gloss, how many tracks, a few sleeves. */
function ChannelRow({ ch, list, albumOf, open }: { ch: number; list: Track[]; albumOf: Map<string, Album>; open(): void }) {
  const t = useT();
  return (
    <button className={`sm-ch ch-${ch}`} onClick={open} disabled={list.length === 0}>
      <Glyph ch={ch} size={22} />
      <span className="sm-ch-text">
        <span className="sm-ch-name">{t(channelKey("ch", ch))}</span>
        <span className="sm-ch-gloss">{t(channelKey("gloss", ch))}</span>
      </span>
      <span className="sm-ch-thumbs" aria-hidden="true">
        {covers(list, albumOf, 3).map((c) => (
          <span key={c.key} className="sm-thumb">
            <Cover art={c.art} title={c.title} />
          </span>
        ))}
      </span>
      <span className="sm-ch-count num">{t("sensme.tracks", { n: list.length.toLocaleString() })}</span>
    </button>
  );
}

function TrackRows({ list, albumOf, current, onOpenAlbum }: { list: Track[]; albumOf: Map<string, Album>; current: number | null; onOpenAlbum(id: string): void }) {
  const t = useT();
  const { state } = useLibrary();
  const [limit, setLimit] = useState(PAGE);
  const more = useRef<HTMLLIElement>(null);
  const hasMore = list.length > limit;
  useEffect(() => setLimit(PAGE), [list]);
  useEffect(() => {
    const el = more.current;
    if (!el || !hasMore) return;
    const io = new IntersectionObserver((entries) => entries[0]?.isIntersecting && setLimit((n) => n + PAGE), {
      root: el.closest(".main"),
      rootMargin: "800px 0px",
    });
    io.observe(el);
    return () => io.disconnect();
  }, [hasMore, limit]);

  return (
    <ol className="sm-rows">
      <li className="sm-rows-head" aria-hidden="true">
        <span />
        <span>{t("sensme.col.track")}</span>
        <span>{state?.sensmeTable ? t(current == null ? "sensme.col.channels" : "sensme.col.others") : ""}</span>
        <span>BPM</span>
        <span>{t("sensme.col.chorus")}</span>
      </li>
      {list.slice(0, limit).map((tr) => {
        const album = albumOf.get(tr.id);
        const s = tr.sensme!;
        const bpm = s.bpm ?? null;
        const others = (s.channels ?? []).filter((c) => c !== current);
        return (
          <li key={tr.id}>
            <button className="sm-row" onClick={() => album && onOpenAlbum(album.id)} disabled={!album}>
              <span className="sm-row-art">
                <Cover art={album?.art ?? tr.cover ?? null} title={album?.title ?? tr.title ?? ""} />
              </span>
              <span className="sm-row-title">
                <span>{tr.title ?? tr.rel.split("/").pop()}</span>
                <span className="sm-row-sub">{[tr.artist, album?.title].filter(Boolean).join(" · ")}</span>
              </span>
              <span className="sm-row-chs">
                {s.channels?.length === 0 ? (
                  <span className="sm-row-none" title={t("sensme.noChannelTitle")}>
                    {t("sensme.noChannel")}
                  </span>
                ) : (
                  others.map((c) => (
                    <span key={c} className={`sm-chip ch-${c}`}>
                      <Glyph ch={c} size={13} />
                      {t(channelKey("ch", c))}
                    </span>
                  ))
                )}
              </span>
              <span className="sm-row-tempo" title={bpm != null ? t("sensme.bpm", { n: bpm.toFixed(1) }) : undefined}>
                {bpm != null && (
                  <>
                    <span className="sm-tempo-bar" aria-hidden="true">
                      <span style={{ width: `${Math.min(100, Math.max(4, ((bpm - BPM_LOW) / (BPM_HIGH - BPM_LOW)) * 100))}%` }} />
                    </span>
                    <span className="mono num">{Math.round(bpm)}</span>
                  </>
                )}
              </span>
              <span className="sm-row-chorus mono num" title={t("sensme.chorusTitle")}>
                {s.chorusMs != null && s.chorusMs < tr.durationMs ? duration(s.chorusMs) : ""}
              </span>
            </button>
          </li>
        );
      })}
      {hasMore && <li ref={more} className="sm-rows-more" aria-hidden="true" />}
    </ol>
  );
}

function Detail({ ch, list, albumOf, onBack, onOpenAlbum }: { ch: number; list: Track[]; albumOf: Map<string, Album>; onBack(): void; onOpenAlbum(id: string): void }) {
  const t = useT();
  const bpms = list.map((tr) => tr.sensme?.bpm).filter((b) => b != null);
  const total = list.reduce((n, tr) => n + tr.durationMs, 0);
  return (
    <div className={`sm-detail ch-${ch}`}>
      <div className="sm-detail-head">
        <div className="sm-detail-what">
          <button className="sm-back" onClick={onBack}>
            <Icon name="chevron" size={15} />
            {t("sensme.back")}
          </button>
          <h2>
            <Glyph ch={ch} size={30} />
            {t(channelKey("ch", ch))}
          </h2>
          <p>{t(channelKey("gloss", ch))}</p>
        </div>
        <div className="sm-detail-facts num">
          <span>
            <b>{list.length.toLocaleString()}</b>
            {t("library.stat.tracks")}
          </span>
          <span>
            <b>{duration(total)}</b>
          </span>
          {bpms.length > 0 && (
            <span>
              <b className="mono">
                {Math.round(Math.min(...bpms))}–{Math.round(Math.max(...bpms))}
              </b>
              BPM
            </span>
          )}
        </div>
      </div>
      {list.length === 0 ? <p className="sec-note">{t("sensme.emptyChannel")}</p> : <TrackRows list={list} albumOf={albumOf} current={ch} onOpenAlbum={onOpenAlbum} />}
    </div>
  );
}

function PlayerRead({ player, unread }: { player: MusicPlayer; unread: number }) {
  const t = useT();
  const { sensme, readSensme, cancelSensme } = useLibrary();
  const [error, setError] = useState<ErrorReport | null>(null);
  const mine = sensme?.deviceId === player.device.id ? sensme : null;
  const running = mine != null && mine.result == null;
  const result = mine?.result ?? null;
  if (!running && !result && unread === 0 && !error) return null;
  const pct = mine && mine.total > 0 ? (mine.done / mine.total) * 100 : 0;
  return (
    <div className="sm-notice">
      <div className="sm-notice-text">
        {result && !running ? (
          <>
            <h3>{result.cancelled ? t("sensme.player.stopped", { read: result.read }) : t("sensme.player.done", { read: result.read, n: result.analysed })}</h3>
            {result.error && (
              <p className="selectable">
                <ErrorText error={result.error} />
              </p>
            )}
          </>
        ) : (
          <>
            <h3>{running ? t("sensme.player.reading", { done: mine.done.toLocaleString(), total: (mine.total || unread).toLocaleString() }) : t("sensme.player.title")}</h3>
            {running ? (
              <span className="scan-meter-track sm-meter">
                <span className="scan-meter-fill" style={{ width: `${pct}%` }} />
              </span>
            ) : (
              <p>{t("sensme.player.body", { device: player.name, n: unread.toLocaleString() })}</p>
            )}
            {error && (
              <p className="selectable">
                <ErrorText error={error} />
              </p>
            )}
          </>
        )}
      </div>
      {running ? (
        <button className="btn btn--ghost" onClick={cancelSensme}>
          {t("library.stop")}
        </button>
      ) : (
        unread > 0 && (
          <button
            className="btn btn--primary"
            onClick={() => {
              setError(null);
              readSensme(player.device.id).catch((e) => setError(asError(e)));
            }}
          >
            <Icon name="eye" size={16} />
            {t("sensme.player.read")}
          </button>
        )
      )}
    </div>
  );
}

/** The SensMe analysis of the tracks that have none, made here and written into the files. */
function AnalyseHere({ player, missing }: { player?: MusicPlayer; missing: number }) {
  const t = useT();
  const { lang } = usePrefs();
  const { analysis, startAnalysis, cancelAnalysis, dismissAnalysis } = useLibrary();
  const [engine, setEngine] = useState<EngineView | null>(null);
  const [plan, setPlan] = useState<AnalysisPlan | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ErrorReport | null>(null);
  const deviceId = player?.device.id ?? null;
  const mine = analysis && analysis.deviceId === deviceId ? analysis : null;
  const elsewhere = analysis != null && mine == null && analysis.result == null;

  useEffect(() => {
    let live = true;
    bridge()
      .then((b) => b.analysisEngine())
      .then((v) => live && setEngine(v))
      .catch((e) => live && setError(asError(e)));
    return () => {
      live = false;
    };
  }, []);
  useEffect(() => setPlan(null), [deviceId]);

  const act = (run: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    run()
      .catch((e) => setError(asError(e)))
      .finally(() => setBusy(false));
  };

  if (mine) {
    const p = mine.progress;
    const r = mine.result?.report ?? null;
    const done = p?.doneFiles ?? 0;
    const total = p?.totalFiles ?? 0;
    const pct = p && p.totalBytes > 0 ? percent(p.doneBytes, p.totalBytes) : 0;
    return (
      <div className="sm-notice sm-make">
        <div className="sm-notice-text">
          {r ? (
            <>
              <h3>
                {r.cancelled
                  ? r.copied > 0
                    ? t("sensme.make.stopped", { n: r.copied.toLocaleString() })
                    : t("sensme.make.stoppedNone")
                  : t("sensme.make.done", { n: r.copied.toLocaleString() })}
              </h3>
              {deviceId && r.copied > 0 && <p>{t("sensme.make.playerAfter")}</p>}
              {r.failures.length > 0 && (
                <details className="sm-make-failures">
                  <summary>{t("sensme.make.failed", { n: r.failures.length })}</summary>
                  <ul>
                    {r.failures.map((f) => (
                      <li key={f.dest} className="selectable">
                        <span>{f.dest}</span>
                        <span>
                          <ErrorText error={f.error} />
                        </span>
                      </li>
                    ))}
                  </ul>
                </details>
              )}
              {mine.result?.backup && (
                <p className="sm-notice-aside">
                  {t("meta.kept")} <bdi className="mono selectable">{mine.result.backup}</bdi>
                </p>
              )}
            </>
          ) : (
            <>
              <h3 className="num">{t("sensme.make.running", { done: done.toLocaleString(), total: (total || missing).toLocaleString() })}</h3>
              <span className="scan-meter-track sm-meter">
                <span className="scan-meter-fill" style={{ width: `${pct}%` }} />
              </span>
              {p?.current && <p className="sm-make-current mono">{p.current}</p>}
            </>
          )}
        </div>
        {r ? (
          <button className="btn btn--ghost" onClick={dismissAnalysis}>
            {t("album.close")}
          </button>
        ) : (
          <button className="btn btn--ghost" onClick={cancelAnalysis}>
            {t("library.stop")}
          </button>
        )}
      </div>
    );
  }

  if (missing === 0 || elsewhere) return null;

  if (engine && !engine.engine) {
    return (
      <div className="sm-notice sm-make sm-make--engine">
        <div className="sm-notice-text">
          <h3>{t("sensme.make.engineTitle", { n: missing.toLocaleString() })}</h3>
          <p>{t("sensme.make.engineBody")}</p>
          {engine.problem && !engine.needEngine && (
            <p className="selectable sm-notice-error">
              <ErrorText error={engine.problem} />
            </p>
          )}
          {error && (
          <p className="selectable sm-notice-error">
            <ErrorText error={error} />
          </p>
        )}
        </div>
        {engine.needEngine && (
          <button
            className="btn btn--primary"
            disabled={busy}
            onClick={() =>
              act(async () => {
                const v = await (await bridge()).analysisPickEngine();
                if (v) setEngine(v);
              })
            }
          >
            <Icon name="folder" size={16} />
            {t("sensme.make.pickEngine")}
          </button>
        )}
      </div>
    );
  }

  if (plan) {
    const short = plan.backupRoot != null && plan.freeBytes != null && plan.freeBytes < plan.bytes;
    const others = plan.others.reduce((n, o) => n + o.tracks, 0);
    return (
      <div className="sm-notice sm-make sm-make--plan">
        <div className="sm-notice-text">
          <h3 className="num">{plan.tracks > 0 ? t("sensme.make.planTitle", { n: plan.tracks.toLocaleString() }) : t("sensme.make.nothing")}</h3>
          {plan.tracks > 0 && (
            <>
              <p className="sm-make-facts num">
                <span>{bytes(plan.bytes, lang)}</span>
                <span>{duration(plan.durationMs)}</span>
              </p>
              <p>{player ? t("sensme.make.howPlayer", { device: player.name }) : t("sensme.make.how")}</p>
              {plan.backupRoot && (
                <p className="sm-notice-aside">
                  {t("meta.backup")} <bdi className="mono selectable">{plan.backupRoot}</bdi>
                  {plan.freeBytes != null && (
                    <span className={`sm-make-room num${short ? " is-short" : ""}`}>
                      {t(short ? "sensme.make.noRoom" : "sensme.make.room", { need: bytes(plan.bytes, lang), free: bytes(plan.freeBytes, lang) })}
                    </span>
                  )}
                </p>
              )}
            </>
          )}
          {others > 0 && (
            <p className="sm-notice-aside">
              {t("sensme.make.others", { n: others.toLocaleString(), formats: plan.others.map((o) => codecName(o.codec)).join(", ") })}
            </p>
          )}
          <p className="sm-make-engine mono selectable">
            <span>{plan.engine.dll}</span>
            {plan.engine.wine && <span>Wine · {plan.engine.wine}</span>}
          </p>
          {error && (
          <p className="selectable sm-notice-error">
            <ErrorText error={error} />
          </p>
        )}
        </div>
        <div className="sm-make-actions">
          <button className="btn btn--ghost" onClick={() => setPlan(null)} disabled={busy}>
            {t("send.cancel")}
          </button>
          {plan.tracks > 0 && (
            <button className="btn btn--primary" disabled={busy || short} onClick={() => act(() => startAnalysis(plan.id, deviceId).then(() => setPlan(null)))}>
              <Icon name="wave" size={16} />
              {t("sensme.make.start")}
            </button>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="sm-notice sm-make">
      <div className="sm-notice-text">
        <h3 className="num">{t("sensme.make.title", { n: missing.toLocaleString() })}</h3>
        <p>{t("sensme.make.body")}</p>
        {error && (
          <p className="selectable sm-notice-error">
            <ErrorText error={error} />
          </p>
        )}
      </div>
      <button
        className="btn btn--primary"
        disabled={busy || engine == null}
        onClick={() => act(async () => setPlan(await (await bridge()).analysisPlan(deviceId)))}
      >
        {engine == null ? <span className="spinner spinner--small" /> : <Icon name="wave" size={16} />}
        {t("sensme.make.prepare")}
      </button>
    </div>
  );
}

function TableNeeded() {
  const t = useT();
  const { importSensme } = useLibrary();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ErrorReport | null>(null);
  return (
    <div className="sm-notice sm-notice--table">
      <div className="sm-notice-text">
        <h3>{t("sensme.table.title")}</h3>
        <p>{t("sensme.table.body")}</p>
        <p className="sm-notice-aside">{t("sensme.table.meanwhile")}</p>
        {error && (
          <p className="selectable sm-notice-error">
            <ErrorText error={error} />
          </p>
        )}
      </div>
      <button
        className="btn btn--primary"
        disabled={busy}
        onClick={() => {
          setBusy(true);
          setError(null);
          importSensme()
            .catch((e) => setError(asError(e)))
            .finally(() => setBusy(false));
        }}
      >
        <Icon name="folder" size={16} />
        {t("sensme.table.pick")}
      </button>
    </div>
  );
}

function matches(tr: Track, album: Album | undefined, q: string): boolean {
  if (!q) return true;
  const hay = `${tr.title ?? ""}\n${tr.artist ?? ""}\n${album?.title ?? ""}\n${album?.artist ?? ""}`.toLowerCase();
  return q
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((w) => hay.includes(w));
}

export function SensMeBoard({
  tracks,
  albums,
  player,
  unread,
  query,
  onOpenAlbum,
}: {
  tracks: Track[];
  albums: Album[];
  /** The player whose shelf is shown; absent for the computer's library. */
  player?: MusicPlayer;
  unread: number;
  query: string;
  onOpenAlbum(id: string): void;
}) {
  const t = useT();
  const { state } = useLibrary();
  const table = state?.sensmeTable ?? false;
  const [open, setOpen] = useState<number | null>(null);

  const albumOf = useMemo(() => {
    const m = new Map<string, Album>();
    for (const a of albums) for (const id of a.tracks) m.set(id, a);
    return m;
  }, [albums]);
  const all = useMemo(() => tracks.filter((tr) => tr.sensme), [tracks]);
  const writable = useMemo(() => tracks.filter((tr) => !tr.sensme && ANALYSABLE.test(tr.path)).length, [tracks]);
  const analysed = useMemo(() => all.filter((tr) => matches(tr, albumOf.get(tr.id), query)), [all, albumOf, query]);
  const byTempo = useMemo(() => [...analysed].sort((a, b) => (a.sensme!.bpm ?? 0) - (b.sensme!.bpm ?? 0)), [analysed]);
  const channels = useMemo(() => {
    const m = new Map<number, Track[]>([...TIME, ...MOOD].map((c) => [c, []]));
    for (const tr of byTempo) for (const c of tr.sensme!.channels ?? []) m.get(c)?.push(tr);
    return m;
  }, [byTempo]);
  const partial = table ? analysed.filter((tr) => tr.sensme!.channels == null).length : 0;
  const top = useRef<HTMLElement>(null);

  useEffect(() => setOpen(null), [player?.device.id]);
  useEffect(() => {
    const el = top.current;
    if (el && el.getBoundingClientRect().top < 0) el.scrollIntoView({ block: "start" });
  }, [open]);

  return (
    <section className="sensme" ref={top}>
      <header className="sm-head">
        <div>
          <h2 className="sm-title">{t("sensme.title")}</h2>
          <p className="sm-lede">{t("sensme.lede")}</p>
        </div>
        {tracks.length > 0 && (
          <div className="sm-coverage">
            <span className="num">{t("sensme.coverage", { n: all.length.toLocaleString(), total: tracks.length.toLocaleString() })}</span>
            <span className="meter-cells" aria-hidden="true">
              {Array.from({ length: 20 }, (_, i) => (
                <i key={i} className={i < Math.round((all.length / tracks.length) * 20) ? "is-on" : undefined} />
              ))}
            </span>
          </div>
        )}
      </header>

      {player && <PlayerRead player={player} unread={unread} />}
      {unread === 0 && <AnalyseHere player={player} missing={writable} />}
      {!table && all.length > 0 && <TableNeeded />}

      {all.length === 0 ? (
        unread === 0 && (
          <div className="sm-none">
            <span className="sm-none-glyphs" aria-hidden="true">
              {TIME.map((c) => (
                <span key={c} className={`ch-${c}`}>
                  <Glyph ch={c} size={26} />
                </span>
              ))}
            </span>
            <h3>{t("sensme.none.title")}</h3>
            <p>{t("sensme.none.body")}</p>
          </div>
        )
      ) : !table ? (
        <div className="sm-plain">
          <h3 className="sm-group">
            {t("sensme.analysed")}
            <span className="sm-group-note">{t("sensme.byTempo")}</span>
          </h3>
          <TrackRows list={byTempo} albumOf={albumOf} current={null} onOpenAlbum={onOpenAlbum} />
        </div>
      ) : open != null ? (
        <Detail ch={open} list={channels.get(open) ?? []} albumOf={albumOf} onBack={() => setOpen(null)} onOpenAlbum={onOpenAlbum} />
      ) : (
        <>
          {(
            [
              ["time", TIME],
              ["mood", MOOD],
            ] as const
          ).map(([group, chs]) => (
            <section key={group} className="sec sm-sec">
              <header className="sec-head">
                <h3 className="sec-title">{t(`sensme.group.${group}`)}</h3>
              </header>
              <div className="sec-body sm-chs">
                {chs.map((c) => (
                  <ChannelRow key={c} ch={c} list={channels.get(c)!} albumOf={albumOf} open={() => setOpen(c)} />
                ))}
              </div>
            </section>
          ))}
          <p className="footnote sm-foot">
            {t("sensme.deviceNote")}
            {partial > 0 && <span> {t("sensme.partial", { n: partial })}</span>}
          </p>
        </>
      )}
    </section>
  );
}
