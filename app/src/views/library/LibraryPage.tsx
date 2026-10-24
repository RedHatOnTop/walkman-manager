import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { Album, Quality, Track } from "../../api/types";
import { Cover } from "../../components/Cover";
import { Icon, type IconName } from "../../components/Icon";
import { DeviceRender } from "../../components/render/DeviceRender";
import { bytes, percent } from "../../format";
import { usePrefs, useT } from "../../i18n";
import { artFor, screenFor } from "../../state/describe";
import { useLibrary, useMusicPlayers, type MusicPlayer } from "../../state/library";
import { albumOnPc, albumPresence, formatLabel, matchesQuery, pcPresence, QUALITIES, trackOnPc, trackOnPlayer } from "../../state/music";
import { AlbumSheet, type AlbumAction, type AlbumPlace } from "./AlbumSheet";
import { MetadataSheet } from "./MetadataSheet";
import { PullSheet } from "./PullSheet";
import { SendSheet } from "./SendSheet";
import { SensMeBoard } from "./SensMeBoard";
import { ErrorText } from "../../components/ErrorText";

type Sort = "artist" | "recent" | "title";
type View = "albums" | "sensme";
/** Tiles added per step as the grid scrolls; a player can hold thousands of albums. */
const PAGE = 120;
/** "pc" for the computer's library, otherwise a player's device id. */
type Source = string;
const PC = "pc";

function ScanMeter() {
  const t = useT();
  const { progress, cancelScan } = useLibrary();
  const pct = progress && progress.found > 0 ? percent(progress.done, progress.found) : 0;
  return (
    <span className="scan-meter" aria-live="polite">
      <span className="num">
        {progress ? t("library.scanning", { done: progress.done.toLocaleString(), found: progress.found.toLocaleString() }) : t("library.scanningShort")}
      </span>
      <span className="scan-meter-track">
        <span className="scan-meter-fill" style={{ width: `${pct}%` }} />
      </span>
      <button className="btn-link" onClick={cancelScan}>
        {t("library.stop")}
      </button>
    </span>
  );
}

function QualityTabs({ albums, filter, onFilter }: { albums: Album[]; filter: Quality | null; onFilter(q: Quality | null): void }) {
  const t = useT();
  const { lang } = usePrefs();
  const groups = QUALITIES.map((q) => {
    const of = albums.filter((a) => a.quality === q);
    return { q, count: of.length, bytes: of.reduce((n, a) => n + a.bytes, 0) };
  });
  return (
    <div className={`qtabs${filter ? " is-filtered" : ""}`} role="tablist" aria-label={t("library.quality")}>
      <button role="tab" aria-selected={filter == null} className={`qtab${filter == null ? " is-on" : ""}`} onClick={() => onFilter(null)}>
        {t("library.all")}
        <span className="qtab-count num">{albums.length}</span>
      </button>
      {groups
        .filter((g) => g.count > 0 || filter === g.q)
        .map((g) => (
          <button
            key={g.q}
            role="tab"
            aria-selected={filter === g.q}
            className={`qtab${filter === g.q ? " is-on" : ""}`}
            onClick={() => onFilter(filter === g.q ? null : g.q)}
            title={`${t(`quality.detail.${g.q}`)}${g.bytes > 0 ? ` · ${bytes(g.bytes, lang)}` : ""}`}
          >
            {t(`quality.${g.q}`)}
            <span className="qtab-count num">{g.count}</span>
          </button>
        ))}
    </div>
  );
}

interface Mark {
  key: string;
  short: string;
  present: number;
  total: number;
  title: string;
}

function Pips({ marks }: { marks: Mark[] }) {
  const shown = marks.filter((m) => m.present > 0);
  if (shown.length === 0) return null;
  return (
    <span className="pips">
      {shown.map((m) => (
        <span key={m.key} className={`pip mono ${m.present === m.total ? "pip--full" : "pip--part"}`} title={m.title}>
          {m.short}
        </span>
      ))}
    </span>
  );
}

function AlbumTile({ album, marks, action, open }: { album: Album; marks: Mark[]; action: AlbumAction | null; open(): void }) {
  const t = useT();
  const { selected, toggle } = useLibrary();
  const isSelected = selected.has(album.id);
  return (
    <div className={`atile${isSelected ? " is-selected" : ""}`}>
      <div className="atile-art">
        <button className="atile-cover" onClick={open} aria-label={album.title}>
          <Cover art={album.art} title={album.title} />
        </button>
        <button
          className="atile-check"
          onClick={() => toggle(album.id)}
          aria-pressed={isSelected}
          aria-label={isSelected ? t("album.selected") : t("album.select")}
          title={isSelected ? t("album.selected") : t("album.select")}
        >
          <Icon name="check" size={14} strokeWidth={2.2} />
        </button>
        {action && (
          <button className="atile-send" onClick={action.run} aria-label={action.label} title={action.label}>
            <Icon name={action.icon} size={17} />
          </button>
        )}
      </div>
      <div className="atile-meta">
        <span className="atile-title">{album.title}</span>
        <span className="atile-artist">{album.artist ?? t(album.compilation ? "library.variousArtists" : "library.unknownArtist")}</span>
        <div className="atile-foot">
          <span className="atile-format mono">{album.mixedFormats ? t("album.mixed") : formatLabel(album.format)}</span>
          <Pips marks={marks} />
        </div>
      </div>
    </div>
  );
}

function Empty({ players, onPick }: { players: MusicPlayer[]; onPick(id: string): void }) {
  const t = useT();
  const { addRoot, state } = useLibrary();
  const searched = (state?.roots.length ?? 0) > 0;
  return (
    <div className="lib-empty">
      <div className="lib-empty-art" aria-hidden="true">
        <span />
        <span />
        <span />
      </div>
      <h2>{t(searched ? "library.empty.noneTitle" : "library.empty.title")}</h2>
      <p>{t(searched ? "library.empty.noneBody" : "library.empty.body")}</p>
      <div className="lib-empty-actions">
        <button className="btn btn--primary" onClick={addRoot}>
          <Icon name="folder" size={16} />
          {t("library.addFolder")}
        </button>
        {players.map((p) => (
          <button className="btn" key={p.device.id} onClick={() => onPick(p.device.id)}>
            <Icon name="receive" size={16} />
            {t("library.empty.fromPlayer", { device: p.short })}
          </button>
        ))}
      </div>
    </div>
  );
}

function Tray({ albums, shown, primary }: { albums: Album[]; shown: Album[]; primary: { icon: IconName; label: string; run(ids: string[]): void } | null }) {
  const t = useT();
  const { lang } = usePrefs();
  const { selected, clear, select } = useLibrary();
  const picked = albums.filter((a) => selected.has(a.id));
  if (picked.length === 0) return null;
  const trackCount = picked.reduce((n, a) => n + a.tracks.length, 0);
  const size = picked.reduce((n, a) => n + a.bytes, 0);
  const allShown = shown.every((a) => selected.has(a.id));
  return (
    <div className="tray" role="region" aria-label={primary?.label}>
      <div className="tray-count">
        <strong>{picked.length === 1 ? t("library.selectedOne") : t("library.selectedMany", { n: picked.length })}</strong>
        <span className="num">{t("library.selectedDetail", { tracks: trackCount, size: bytes(size, lang) })}</span>
      </div>
      <div className="tray-actions">
        {!allShown && (
          <button className="btn btn--ghost" onClick={() => select(shown.map((a) => a.id))}>
            {t("library.selectShown")}
          </button>
        )}
        <button className="btn btn--ghost" onClick={clear}>
          {t("library.clear")}
        </button>
        {primary && (
          <button className="btn btn--primary" onClick={() => primary.run(picked.map((a) => a.id))}>
            <Icon name={primary.icon} size={16} />
            {primary.label}
          </button>
        )}
      </div>
    </div>
  );
}

/** A transfer that runs on after its sheet was closed, as a pill to reopen it. */
function TransferTray({ reopen }: { reopen(): void }) {
  const t = useT();
  const { sync, pull, retag } = useLibrary();
  const job = retag ?? pull ?? sync;
  if (!job) return null;
  const p = job.progress;
  const pct = p && p.totalBytes > 0 ? Math.floor(percent(p.doneBytes, p.totalBytes)) : 0;
  const [doneKey, busyKey] = retag ? (["meta.done", "meta.writing"] as const) : pull ? (["pull.done", "pull.copying"] as const) : (["send.done", "send.sending"] as const);
  const done = job.report ? (job.report.cancelled ? t("send.stopped") : t(doneKey)) : null;
  return (
    <button className="tray" onClick={reopen}>
      <span className="status-dot status-dot--live" />
      <span className="tray-count">
        <strong>{done ?? `${t(busyKey)} ${pct}%`}</strong>
        {p && <span className="num">{t("send.files", { done: p.doneFiles, total: p.totalFiles })}</span>}
      </span>
      <Icon name="chevron" size={16} />
    </button>
  );
}

/** Analysis that runs on while its board is out of sight, as a pill back to it. */
function AnalysisTray({ open }: { open(): void }) {
  const t = useT();
  const { analysis } = useLibrary();
  if (!analysis) return null;
  const p = analysis.progress;
  const r = analysis.result?.report;
  const pct = p && p.totalBytes > 0 ? Math.floor(percent(p.doneBytes, p.totalBytes)) : 0;
  return (
    <button className="tray" onClick={open}>
      <span className="status-dot status-dot--live" />
      <span className="tray-count">
        <strong>{r ? (r.cancelled ? t("send.stopped") : t("sensme.make.done", { n: r.copied.toLocaleString() })) : `${t("sensme.make.tray")} ${pct}%`}</strong>
        {p && <span className="num">{t("send.files", { done: p.doneFiles, total: p.totalFiles })}</span>}
      </span>
      <Icon name="chevron" size={16} />
    </button>
  );
}

function ViewSwitch({ view, onView }: { view: View; onView(v: View): void }) {
  const t = useT();
  return (
    <div className="lib-view" role="tablist">
      {(["albums", "sensme"] as const).map((v) => (
        <button key={v} role="tab" aria-selected={view === v} className={`lib-view-btn${view === v ? " is-on" : ""}`} onClick={() => onView(v)}>
          {t(`library.view.${v}`)}
        </button>
      ))}
    </div>
  );
}

/** The computer and each connected player, as the places a library can be read from. */
function SourceSwitch({ source, players, onPick }: { source: Source; players: MusicPlayer[]; onPick(s: Source): void }) {
  const t = useT();
  const { state, shelves } = useLibrary();
  const pcCount = state?.index.albums.length ?? 0;
  return (
    <div className="lib-switch" role="tablist" aria-label={t("library.source.aria")}>
      <button role="tab" aria-selected={source === PC} className={`lib-switch-btn${source === PC ? " is-on" : ""}`} onClick={() => onPick(PC)}>
        <span className="lib-switch-glyph">
          <Icon name="computer" size={20} />
        </span>
        <span className="lib-switch-name">{t("library.source.pc")}</span>
        {pcCount > 0 && <span className="lib-switch-count num">{pcCount.toLocaleString()}</span>}
      </button>
      {players.map((p) => {
        const on = source === p.device.id;
        const shelf = shelves[p.device.id];
        const art = artFor(p.device.profileId);
        return (
          <button key={p.device.id} role="tab" aria-selected={on} className={`lib-switch-btn${on ? " is-on" : ""}`} onClick={() => onPick(p.device.id)}>
            <span className="lib-switch-glyph lib-switch-glyph--device">
              {art ? <DeviceRender art={art} scale={0.27} turned={false} screen={on ? screenFor(p.device.mode, p.snapshot) : { kind: "off" }} /> : <Icon name="player" size={20} />}
            </span>
            <span className="lib-switch-name mono">{p.short}</span>
            {shelf?.loading ? (
              <span className="spinner spinner--small" />
            ) : (
              shelf?.library && <span className="lib-switch-count num">{shelf.library.index.albums.length.toLocaleString()}</span>
            )}
          </button>
        );
      })}
    </div>
  );
}

function volumeLabel(root: string, player: MusicPlayer | undefined): "storage.card" | "storage.internal" {
  const card = player?.snapshot?.volumes.find((v) => v.kind === "card");
  const id = card?.mount.split("/").pop();
  return id && root.includes(id) ? "storage.card" : "storage.internal";
}

function Stats({ albums, tracks }: { albums: Album[]; tracks: number }) {
  const t = useT();
  const { lang } = usePrefs();
  if (albums.length === 0) return null;
  const size = albums.reduce((n, a) => n + a.bytes, 0);
  return (
    <div className="lib-stats num">
      <span>
        <b>{albums.length.toLocaleString()}</b>
        {t("library.stat.albums")}
      </span>
      <span>
        <b>{tracks.toLocaleString()}</b>
        {t("library.stat.tracks")}
      </span>
      <span>
        <b>{bytes(size, lang)}</b>
      </span>
    </div>
  );
}

function ShelfLoading() {
  const t = useT();
  return (
    <>
      <div className="plan-reading lib-reading">
        <span className="spinner" />
        {t("library.player.reading")}
      </div>
      <div className="albums" aria-busy="true">
        {Array.from({ length: 10 }, (_, i) => (
          <div className="atile atile--ghost" key={i}>
            <div className="atile-art skeleton" />
            <span className="skeleton skeleton--line" />
          </div>
        ))}
      </div>
    </>
  );
}

export function LibraryPage({ source: initial, view: initialView }: { source?: string; view?: View }) {
  const t = useT();
  const { lang } = usePrefs();
  const lib = useLibrary();
  const players = useMusicPlayers();
  const [source, setSource] = useState<Source>(initial ?? PC);
  const [query, setQuery] = useState("");
  const [quality, setQuality] = useState<Quality | null>(null);
  const [sort, setSort] = useState<Sort>("artist");
  const [view, setView] = useState<View>(initialView ?? "albums");
  const [openAlbum, setOpenAlbum] = useState<string | null>(null);
  const [sending, setSending] = useState<string[] | null>(null);
  const [pulling, setPulling] = useState<string[] | null>(null);
  const [fixing, setFixing] = useState<Album | null>(null);
  const [jobOpen, setJobOpen] = useState(false);
  const [stuck, setStuck] = useState(false);
  const sentinel = useRef<HTMLDivElement>(null);
  const { clear, shelves, loadShelf } = lib;

  useEffect(() => {
    if (initial) setSource(initial);
  }, [initial]);

  const player = source === PC ? undefined : players.find((p) => p.device.id === source);
  // A player that was unplugged takes its shelf with it.
  useEffect(() => {
    if (source !== PC && !player) setSource(PC);
  }, [source, player]);

  const shelf = player ? shelves[player.device.id] : undefined;
  useEffect(() => {
    if (player && !shelf) loadShelf(player.device.id);
  }, [player, shelf, loadShelf]);

  const pick = (s: Source) => {
    if (s === source) return;
    clear();
    setQuality(null);
    setOpenAlbum(null);
    setSource(s);
  };

  const onPc = source === PC;
  const index = onPc ? lib.state?.index : shelf?.library?.index;
  const tracks = onPc ? lib.tracks : (shelf?.tracks ?? new Map<string, Track>());
  const albums = useMemo(() => index?.albums ?? [], [index]);
  const pc = useMemo(() => pcPresence(lib.state?.index.tracks ?? []), [lib.state]);
  const hasContent = albums.length > 0;

  useEffect(() => {
    const el = sentinel.current;
    if (!el) return;
    const io = new IntersectionObserver(
      (entries) => {
        const e = entries[0];
        if (e) setStuck(!e.isIntersecting);
      },
      { root: el.closest(".main") },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [hasContent]);

  const collator = useMemo(() => new Intl.Collator(lang === "ko" ? "ko" : "en", { sensitivity: "base", numeric: true }), [lang]);
  const shown = useMemo(() => {
    const list = albums.filter((a) => (!quality || a.quality === quality) && matchesQuery(a, query));
    if (sort === "recent") return [...list].sort((a, b) => b.modified - a.modified);
    if (sort === "title") return [...list].sort((a, b) => collator.compare(a.title, b.title));
    return [...list].sort(
      (a, b) =>
        Number(a.artist == null) - Number(b.artist == null) ||
        collator.compare(a.artist ?? "", b.artist ?? "") ||
        collator.compare(a.title, b.title),
    );
  }, [albums, quality, query, sort, collator]);

  const [limit, setLimit] = useState(PAGE);
  const more = useRef<HTMLDivElement>(null);
  useEffect(() => setLimit(PAGE), [source, quality, query, sort]);
  const hasMore = shown.length > limit;
  useEffect(() => {
    const el = more.current;
    if (!el || !hasMore) return;
    const io = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting) setLimit((n) => n + PAGE);
      },
      { root: el.closest(".main"), rootMargin: "1200px 0px" },
    );
    io.observe(el);
    return () => io.disconnect();
  }, [hasMore, limit, view]);

  const marks = (album: Album): Mark[] => {
    if (!onPc) {
      const { present, total } = albumOnPc(album, tracks, pc);
      return [{ key: PC, short: "PC", present, total, title: t("library.presencePc", { present, total }) }];
    }
    return players.flatMap((p) => {
      if (!p.presence) return [];
      const { present, total } = albumPresence(album, tracks, p.presence);
      return [{ key: p.device.id, short: p.short, present, total, title: t("library.presence", { device: p.name, present, total }) }];
    });
  };
  const actionFor = (ids: string[], close?: () => void): AlbumAction | null => {
    if (!onPc) {
      return {
        icon: "receive",
        label: t(ids.length === 1 ? "album.pull" : "library.pull"),
        run: () => {
          close?.();
          setPulling(ids);
        },
      };
    }
    if (players.length === 0) return null;
    return {
      icon: "send",
      label: t(ids.length === 1 ? "album.send" : "library.send"),
      run: () => {
        close?.();
        setSending(ids);
      },
    };
  };

  const album = openAlbum ? albums.find((a) => a.id === openAlbum) : undefined;
  const roots = lib.state?.roots ?? [];
  const job = lib.retag ?? lib.pull ?? lib.sync;
  const analysisAway = lib.analysis != null && !(view === "sensme" && source === (lib.analysis.deviceId ?? PC));
  const jobPlayer = job?.deviceId ? players.find((p) => p.device.id === job.deviceId) : undefined;
  const pullAlbums = useMemo(() => {
    if (!pulling) return [];
    const byId = new Map(albums.map((a) => [a.id, a]));
    return pulling.map((id) => byId.get(id)).filter((a) => a != null);
  }, [pulling, albums]);

  if (!lib.state) return <div className="page library" />;

  let hero: ReactNode;
  if (onPc) {
    hero = (
      <>
        <span className="eyebrow">{t("library.eyebrow")}</span>
        <h1 className="lib-title">{t("library.title")}</h1>
        <Stats albums={albums} tracks={index?.tracks.length ?? 0} />
        <div className="lib-hero-actions">
          <button className="btn" onClick={lib.addRoot}>
            <Icon name="plus" size={16} />
            {t("library.addFolder")}
          </button>
          {lib.scanning ? (
            <ScanMeter />
          ) : (
            roots.length > 0 && (
              <button className="icon-btn" onClick={lib.rescan} aria-label={t("library.rescan")} title={t("library.rescan")}>
                <Icon name="refresh" />
              </button>
            )
          )}
        </div>
        {roots.length > 0 && (
          <div className="lib-sources" aria-label={t("library.sources")}>
            {roots.map((r) => (
              <span className="source" key={r} title={r}>
                <Icon name="folder" size={14} />
                <span className="source-path">
                  <bdi>{r}</bdi>
                </span>
                <button className="icon-btn icon-btn--small" onClick={() => lib.removeRoot(r)} aria-label={t("library.removeFolder")} title={t("library.removeFolder")}>
                  <Icon name="close" size={13} />
                </button>
              </span>
            ))}
          </div>
        )}
      </>
    );
  } else {
    const perRoot = (index?.roots ?? []).map((r, i) => ({ root: r, n: index!.tracks.filter((tr) => tr.root === i).length }));
    hero = (
      <>
        <span className="eyebrow">{t("library.player.eyebrow")}</span>
        <h1 className="lib-title">{player?.name}</h1>
        <Stats albums={albums} tracks={index?.tracks.length ?? 0} />
        <div className="lib-hero-actions">
          <button
            className={`btn${shelf?.loading ? " is-spinning" : ""}`}
            onClick={() => player && loadShelf(player.device.id)}
            disabled={shelf?.loading}
          >
            <Icon name="refresh" size={16} />
            {t("library.player.reload")}
          </button>
          {shelf?.library && (
            <span className="lib-origin">
              <Icon name="info" size={14} />
              {t(shelf.library.source === "media-store" ? "library.player.mediaStore" : "library.player.folders")}
            </span>
          )}
        </div>
        {perRoot.length > 0 && (
          <div className="lib-sources">
            {perRoot.map(({ root, n }) => {
              const label = volumeLabel(root, player);
              return (
                <span className="source source--volume" key={root} title={root}>
                  <Icon name={label === "storage.card" ? "card" : "chip"} size={14} />
                  {t(label)}
                  <span className="num">{n.toLocaleString()}</span>
                </span>
              );
            })}
          </div>
        )}
      </>
    );
  }

  const trayPrimary: { icon: IconName; label: string; run(ids: string[]): void } | null = onPc
    ? players.length > 0
      ? { icon: "send", label: t("library.send"), run: setSending }
      : null
    : { icon: "receive", label: t("library.pull"), run: setPulling };

  return (
    <div className="page library">
      <header className="lib-hero">
        <div className="lib-hero-text">
          {players.length > 0 && <SourceSwitch source={source} players={players} onPick={pick} />}
          {hero}
        </div>
      </header>

      {lib.error && onPc && (
        <div className="notice notice--warn">
          <Icon name="alert" size={20} />
          <div className="notice-text">
            <h3>{t("error.title")}</h3>
            <p className="selectable">
              <ErrorText error={lib.error} />
            </p>
          </div>
        </div>
      )}
      {shelf?.error && !onPc && (
        <div className="notice notice--warn">
          <Icon name="alert" size={20} />
          <div className="notice-text">
            <h3>{t("error.title")}</h3>
            <p className="selectable">
              <ErrorText error={shelf.error} />
            </p>
          </div>
          <button className="btn" onClick={() => player && loadShelf(player.device.id)}>
            {t("error.retry")}
          </button>
        </div>
      )}

      {!onPc && !shelf?.library && shelf?.loading ? (
        <ShelfLoading />
      ) : !hasContent ? (
        onPc ? (
          !lib.scanning && <Empty players={players} onPick={pick} />
        ) : (
          shelf?.library && <p className="sec-note">{t("library.player.empty")}</p>
        )
      ) : (
        <>
          <div ref={sentinel} />
          <div className={`lib-toolbar${stuck ? " is-stuck" : ""}`}>
            <ViewSwitch view={view} onView={setView} />
            {view === "albums" && albums.length > 0 && <QualityTabs albums={albums} filter={quality} onFilter={setQuality} />}
            <span className="grow" />
            <label className="search">
              <Icon name="search" size={16} />
              <input type="search" value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t("library.search")} spellCheck={false} />
            </label>
            {view === "albums" && (
              <label className="sortpick" title={t("library.sort")}>
                <Icon name="sort" size={16} />
                <select value={sort} onChange={(e) => setSort(e.target.value as Sort)} aria-label={t("library.sort")}>
                  {(["artist", "recent", "title"] as const).map((s) => (
                    <option key={s} value={s}>
                      {t(`library.sort.${s}`)}
                    </option>
                  ))}
                </select>
              </label>
            )}
          </div>

          {view === "sensme" ? (
            <SensMeBoard
              tracks={index?.tracks ?? []}
              albums={albums}
              player={player}
              unread={shelf?.library?.sensmeUnread ?? 0}
              query={query}
              onOpenAlbum={setOpenAlbum}
            />
          ) : (
            <>
              {onPc && players.length > 0 && (
                <div className="lib-players">
                  {players.map((p) => (
                    <span className="lib-player" key={p.device.id}>
                      {p.music?.loading && !p.presence ? <span className="spinner" /> : <span className="pip pip--full mono">{p.short}</span>}
                      {t("library.onPlayer", { device: p.name })}
                    </span>
                  ))}
                </div>
              )}
              {!onPc && (
                <div className="lib-players">
                  <span className="lib-player">
                    <span className="pip pip--full mono">PC</span>
                    {t("library.onPc")}
                  </span>
                </div>
              )}

              {shown.length === 0 && albums.length > 0 && <p className="sec-note">{t("library.noResults")}</p>}
              <div className={`albums${lib.selected.size > 0 ? " is-selecting" : ""}`}>
                {shown.slice(0, limit).map((a) => (
                  <AlbumTile key={a.id} album={a} marks={marks(a)} action={actionFor([a.id])} open={() => setOpenAlbum(a.id)} />
                ))}
              </div>
              {hasMore && <div ref={more} className="albums-more" aria-hidden="true" />}

              {onPc && index && index.skipped.length > 0 && (
                <details className="lib-skipped">
                  <summary>{t("library.skipped", { n: index.skipped.length })}</summary>
                  <ul>
                    {index.skipped.map((s) => (
                      <li key={s.path} className="selectable">
                        <span>{s.path}</span>
                        <span className="mono">{s.reason}</span>
                      </li>
                    ))}
                  </ul>
                </details>
              )}
            </>
          )}
        </>
      )}

      {job && !sending && !pulling && !fixing && !jobOpen ? (
        <TransferTray reopen={() => setJobOpen(true)} />
      ) : analysisAway && lib.selected.size === 0 ? (
        <AnalysisTray
          open={() => {
            pick(lib.analysis?.deviceId ?? PC);
            setView("sensme");
          }}
        />
      ) : (
        !sending && !pulling && !fixing && <Tray albums={albums} shown={shown} primary={trayPrimary} />
      )}

      {album && (
        <AlbumSheet
          album={album}
          tracks={tracks}
          places={marks(album).map(({ key, short, present, total }): AlbumPlace => ({ key, short, present, total }))}
          marked={(tr) => (onPc ? players.some((p) => p.presence && trackOnPlayer(tr, p.presence)) : trackOnPc(tr, pc))}
          primary={actionFor([album.id], () => setOpenAlbum(null))}
          more={[
            {
              icon: "search",
              label: t("album.lookup"),
              run: () => {
                setOpenAlbum(null);
                setFixing(album);
              },
            },
          ]}
          onClose={() => setOpenAlbum(null)}
        />
      )}
      {(fixing || (jobOpen && lib.retag)) && (
        <MetadataSheet
          album={fixing ?? lib.retag!.album}
          tracks={tracks}
          player={lib.retag ? jobPlayer : player}
          onClose={() => {
            setFixing(null);
            setJobOpen(false);
          }}
        />
      )}
      {(sending || (jobOpen && lib.sync && !lib.pull && !lib.retag)) && (
        <SendSheet
          albumIds={sending ?? []}
          players={players}
          onClose={() => {
            setSending(null);
            setJobOpen(false);
          }}
        />
      )}
      {(pulling || (jobOpen && lib.pull && !lib.retag)) && (
        <PullSheet
          albums={pullAlbums}
          player={lib.pull ? jobPlayer : player}
          onClose={() => {
            setPulling(null);
            setJobOpen(false);
          }}
        />
      )}
    </div>
  );
}
