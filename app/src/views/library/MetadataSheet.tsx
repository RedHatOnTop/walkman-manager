import { useEffect, useMemo, useState, type FormEvent } from "react";
import { asError, bridge } from "../../api/bridge";
import type { Album, Candidate, ErrorReport, Proposal, SearchResult, TagField, Track, TrackProposal } from "../../api/types";
import { Cover } from "../../components/Cover";
import { Icon } from "../../components/Icon";
import { percent } from "../../format";
import { useT } from "../../i18n";
import { useLibrary, type MusicPlayer } from "../../state/library";
import { duration } from "../../state/music";
import { useEscape } from "./AlbumSheet";
import { ErrorText } from "../../components/ErrorText";

const SOURCE_NAME = { "music-brainz": "MusicBrainz", itunes: "iTunes" } as const;

function sourceLabel(c: Candidate): string {
  return c.source === "itunes" && c.country ? `${SOURCE_NAME.itunes} ${c.country}` : SOURCE_NAME[c.source];
}

function Tick({ on, label, disabled, onToggle }: { on: boolean; label: string; disabled?: boolean; onToggle(): void }) {
  return (
    <button type="button" className={`tick${on ? " is-on" : ""}`} role="checkbox" aria-checked={on} aria-label={label} disabled={disabled} onClick={onToggle}>
      {on && <Icon name="check" size={12} />}
    </button>
  );
}

/** A search result's cover, fetched when the row first shows. */
function CandidateCover({ id, title }: { id: string; title: string }) {
  const [art, setArt] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    bridge()
      .then((b) => b.lookupCover(id))
      .then(
        (key) => live && setArt(key),
        () => {},
      );
    return () => {
      live = false;
    };
  }, [id]);
  return <Cover art={art} title={title} />;
}

function Results({ found, opening, onPick }: { found: SearchResult; opening: string | null; onPick(c: Candidate): void }) {
  const t = useT();
  return (
    <>
      {found.failed.map(([source, error]) => (
        <p key={source} className="plan-note is-warn selectable">
          <Icon name="alert" size={16} />
          <span>
            {t("meta.failed", { source: SOURCE_NAME[source] })}
            <span className="err-detail">
              <ErrorText error={error} />
            </span>
          </span>
        </p>
      ))}
      {found.candidates.length === 0 ? (
        <p className="meta-none">{t("meta.none")}</p>
      ) : (
        <ul className="meta-results">
          {found.candidates.map((c) => {
            const facts = [c.year, c.source === "music-brainz" ? c.country : null, c.format, t("meta.trackCount", { n: c.trackCount }), c.label].filter(
              (x) => x != null && x !== "",
            );
            const tier = c.score >= 80 ? "high" : c.score >= 55 ? "mid" : "low";
            return (
              <li key={c.id}>
                <button className={`meta-result${opening === c.id ? " is-opening" : ""}`} onClick={() => onPick(c)} disabled={opening != null}>
                  <span className="meta-result-cover">
                    <CandidateCover id={c.id} title={c.title} />
                  </span>
                  <span className="meta-result-main">
                    <span className="meta-result-title">{c.title}</span>
                    <span className="meta-result-artist">{c.artist}</span>
                    <span className="meta-result-facts num">{facts.join(" · ")}</span>
                  </span>
                  <span className="meta-result-side">
                    <span className={`meta-source meta-source--${c.source} mono`}>{sourceLabel(c)}</span>
                    {opening === c.id ? (
                      <span className="spinner spinner--small" />
                    ) : (
                      <span className={`meta-score meta-score--${tier}`} title={`${t("meta.score")} ${c.score}%`}>
                        <span className="num">{c.score}</span>
                        <span className="meta-score-bar">
                          <span style={{ width: `${c.score}%` }} />
                        </span>
                      </span>
                    )}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </>
  );
}

function TrackRow({ tp, file, on, onToggle }: { tp: TrackProposal; file: Track | undefined; on: boolean; onToggle(): void }) {
  const t = useT();
  const title = tp.changes.find((c) => c.field === "title");
  const others = tp.changes.filter((c) => c.field !== "title");
  const current = file?.title ?? tp.name;
  const off = tp.matched && file && tp.matched.durationMs != null ? Math.round((file.durationMs - tp.matched.durationMs) / 1000) : null;
  const usable = tp.writable && tp.changes.length > 0;
  return (
    <li className={`meta-trk${usable && on ? " is-on" : ""}${tp.matched ? "" : " is-loose"}`}>
      {tp.changes.length > 0 ? <Tick on={usable && on} label={current} disabled={!usable} onToggle={onToggle} /> : <span aria-hidden="true" />}
      <span className="meta-trk-no mono num">{tp.matched ? tp.matched.number : "–"}</span>
      <span className="meta-trk-main">
        {title ? (
          <span className="meta-trk-title">
            <span className="meta-old selectable">{title.from ?? current}</span>
            <span className="meta-new selectable">{title.to}</span>
          </span>
        ) : (
          <span className="meta-trk-title">
            <span className="meta-same selectable">{current}</span>
          </span>
        )}
        <span className="meta-trk-notes">
          {others.map((c) => (
            <span key={c.field} className="meta-chip">
              {t(`meta.field.${c.field}`)} <strong>{c.to}</strong>
            </span>
          ))}
          {tp.repairs && <span className="meta-chip meta-chip--fix">{t("meta.repairs")}</span>}
          {!tp.matched && <span className="meta-trk-note">{t("meta.noMatch")}</span>}
          {!tp.writable && <span className="meta-trk-note is-warn">{t("meta.notWritable")}</span>}
        </span>
      </span>
      <span className="meta-trk-time num">
        {file && <span className="mono">{duration(file.durationMs)}</span>}
        {off != null && off !== 0 && <span className="meta-trk-off mono">{off > 0 ? `+${off}s` : `${off}s`}</span>}
      </span>
    </li>
  );
}

function ProposalView({
  album,
  proposal,
  tracks,
  player,
  fields,
  setFields,
  picked,
  setPicked,
  useCover,
  setUseCover,
}: {
  album: Album;
  proposal: Proposal;
  tracks: Map<string, Track>;
  player: MusicPlayer | undefined;
  fields: Set<TagField>;
  setFields(f: Set<TagField>): void;
  picked: Set<string>;
  setPicked(p: Set<string>): void;
  useCover: boolean;
  setUseCover(v: boolean): void;
}) {
  const t = useT();
  const c = proposal.candidate;
  const toggle = <T,>(set: Set<T>, v: T) => {
    const next = new Set(set);
    if (!next.delete(v)) next.add(v);
    return next;
  };
  const repairs = proposal.tracks.filter((tp) => tp.repairs).length;
  return (
    <>
      <section className="meta-compare">
        <figure className="meta-sleeve meta-sleeve--now">
          <Cover art={album.art} title={album.title} />
          <figcaption>{t("meta.coverNow")}</figcaption>
        </figure>
        <span className="meta-compare-arrow" aria-hidden="true">
          <Icon name="chevron" size={18} />
        </span>
        <figure className={`meta-sleeve meta-sleeve--new${proposal.cover && !useCover ? " is-off" : ""}`}>
          {proposal.cover ? <Cover art={proposal.cover.art} title={c.title} /> : <div className="cover-blank meta-sleeve-none">{t("meta.coverNone")}</div>}
          <figcaption className="num">
            {t("meta.coverNew")}
            {proposal.cover && <span className="mono"> {`${proposal.cover.width}×${proposal.cover.height}`}</span>}
          </figcaption>
        </figure>
        <div className="meta-compare-text">
          <span className="eyebrow">
            {t("meta.from")} · <span className="mono">{sourceLabel(c)}</span>
          </span>
          <h3 className="selectable">{c.title}</h3>
          <p className="selectable">{c.artist}</p>
          <p className="meta-compare-facts num">{[c.date ?? c.year, c.label].filter(Boolean).join(" · ")}</p>
          {proposal.cover ? (
            <label className="meta-switch">
              <Tick on={useCover} label={t("meta.coverUse")} onToggle={() => setUseCover(!useCover)} />
              <span onClick={() => setUseCover(!useCover)}>{t("meta.coverUse")}</span>
            </label>
          ) : (
            <p className="meta-compare-facts">{t("meta.coverMissing")}</p>
          )}
        </div>
      </section>

      {proposal.album.length > 0 && (
        <section className="send-step">
          <h3 className="eyebrow">{t("meta.fields")}</h3>
          <ul className="meta-fields">
            {proposal.album.map((ch) => {
              const on = fields.has(ch.field);
              return (
                <li key={ch.field} className={on ? "is-on" : ""}>
                  <Tick on={on} label={t(`meta.field.${ch.field}`)} onToggle={() => setFields(toggle(fields, ch.field))} />
                  <span className="meta-field-name">{t(`meta.field.${ch.field}`)}</span>
                  <span className={`meta-field-from selectable${ch.from == null ? " is-empty" : ""}`}>{ch.from ?? t("meta.empty")}</span>
                  <Icon name="chevron" size={13} />
                  <span className="meta-field-to selectable">{ch.to}</span>
                </li>
              );
            })}
          </ul>
        </section>
      )}

      <section className="send-step">
        <h3 className="eyebrow">{t("meta.tracks")}</h3>
        <ol className="meta-tracks">
          {proposal.tracks.map((tp) => (
            <TrackRow key={tp.trackId} tp={tp} file={tracks.get(tp.trackId)} on={picked.has(tp.trackId)} onToggle={() => setPicked(toggle(picked, tp.trackId))} />
          ))}
        </ol>
        {proposal.unmatched.length > 0 && (
          <details className="meta-unmatched">
            <summary>{t("meta.unmatched", { n: proposal.unmatched.length })}</summary>
            <ol>
              {proposal.unmatched.map((r) => (
                <li key={`${r.disc}-${r.number}`}>
                  <span className="mono num">{r.number}</span>
                  <span className="selectable">{r.title}</span>
                  {r.durationMs != null && <span className="mono num">{duration(r.durationMs)}</span>}
                </li>
              ))}
            </ol>
          </details>
        )}
        {repairs > 0 && (
          <p className="plan-note">
            <Icon name="info" size={16} />
            {t("album.garbled", { n: repairs })} · {t("meta.repairs")}
          </p>
        )}
      </section>

      <section className="send-step">
        <h3 className="eyebrow">{t("meta.where")}</h3>
        <div className="pull-dest">
          <Icon name={player ? "player" : "folder"} size={16} />
          <span className="meta-path selectable">
            {player && <strong>{player.name} </strong>}
            <bdi>{proposal.folder}</bdi>
          </span>
        </div>
        <p className="plan-note">
          <Icon name="note" size={16} />
          {t("meta.inPlace")}
        </p>
        {proposal.backupRoot && (
          <p className="plan-note">
            <Icon name="shield" size={16} />
            <span>
              {t("meta.backup")} <bdi className="meta-path selectable">{proposal.backupRoot}</bdi>
            </span>
          </p>
        )}
      </section>
    </>
  );
}

function JobView({ player, onClose }: { player: MusicPlayer | undefined; onClose(): void }) {
  const t = useT();
  const { retag, cancelRetag, dismissRetag } = useLibrary();
  if (!retag) return null;
  const p = retag.progress;
  const r = retag.report;
  const pct = p && p.totalFiles > 0 ? Math.floor(percent(p.doneFiles, p.totalFiles)) : r ? 100 : 0;
  const finish = () => {
    dismissRetag();
    onClose();
  };
  return (
    <>
      <div className="sheet-body">
        <div className="xfer" aria-live="polite">
          <span className="eyebrow">
            {r ? (r.cancelled ? t("send.stopped") : t("meta.done")) : t("meta.writing")} · {retag.album.title}
          </span>
          <div className="xfer-top">
            <div className="xfer-figure num">
              {pct}
              <small>%</small>
            </div>
            <span className="meta-job-cover">
              <Cover art={retag.album.art} title={retag.album.title} />
            </span>
          </div>
          <div className={`xfer-bar${r ? " is-done" : ""}`}>
            <span style={{ width: `${pct}%` }} />
          </div>
          {p && (
            <div className="xfer-meta num">
              <span>{t("send.files", { done: p.doneFiles, total: p.totalFiles })}</span>
              <span>{player?.name ?? t("library.source.pc")}</span>
            </div>
          )}
          {!r && p?.current && (
            <div className="xfer-current">
              <span className="status-dot status-dot--live" />
              <span className="selectable">
                <bdi>{p.current}</bdi>
              </span>
            </div>
          )}
          {r && (
            <div className={`xfer-result${r.failures.length > 0 || r.cancelled ? " is-warn" : ""}`}>
              <Icon name={r.failures.length > 0 ? "alert" : "check"} size={18} />
              <div>
                {r.copied > 0 && <p>{t(retag.deviceId ? "meta.writtenPlayer" : "meta.written", { n: r.copied })}</p>}
                {retag.backup && (
                  <p>
                    {t("meta.kept")} <bdi className="meta-path selectable">{retag.backup}</bdi>
                  </p>
                )}
                {r.failures.length > 0 && (
                  <>
                    <p>{t("send.failures", { n: r.failures.length })}</p>
                    <ul className="xfer-failures selectable">
                      {r.failures.map((f) => (
                        <li key={f.dest}>
                          {f.dest}
                          <span>
                            <ErrorText error={f.error} />
                          </span>
                        </li>
                      ))}
                    </ul>
                  </>
                )}
              </div>
            </div>
          )}
        </div>
      </div>
      <footer className="sheet-foot">
        {r ? (
          <button className="btn btn--primary" onClick={finish}>
            {t("send.close")}
          </button>
        ) : (
          <>
            <span className="sheet-foot-note">{t("send.background")}</span>
            <button className="btn" onClick={cancelRetag}>
              {t("send.stop")}
            </button>
          </>
        )}
      </footer>
    </>
  );
}

export function MetadataSheet({
  album,
  tracks,
  player,
  onClose,
}: {
  album: Album;
  tracks: Map<string, Track>;
  player: MusicPlayer | undefined;
  onClose(): void;
}) {
  const t = useT();
  const lib = useLibrary();
  useEscape(onClose);

  const [albumQ, setAlbumQ] = useState(album.title);
  const [artistQ, setArtistQ] = useState(album.artist ?? "");
  const [found, setFound] = useState<SearchResult | null>(null);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState<ErrorReport | null>(null);
  const [opening, setOpening] = useState<string | null>(null);
  const [proposal, setProposal] = useState<Proposal | null>(null);
  const [fields, setFields] = useState<Set<TagField>>(new Set());
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [useCover, setUseCover] = useState(true);
  const [starting, setStarting] = useState(false);
  const deviceId = player?.device.id ?? null;

  const search = async (e?: FormEvent) => {
    e?.preventDefault();
    if (!albumQ.trim()) return;
    setSearching(true);
    setError(null);
    try {
      const b = await bridge();
      setFound(await b.lookupSearch({ album: albumQ, artist: artistQ.trim() || null, tracks: album.tracks.length }));
    } catch (err) {
      setError(asError(err));
      setFound(null);
    } finally {
      setSearching(false);
    }
  };

  useEffect(() => {
    if (!lib.retag) void search();
    // Searches once with the album's own tags when the sheet opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const pick = async (c: Candidate) => {
    setOpening(c.id);
    setError(null);
    try {
      const p = await (await bridge()).lookupPropose(c.id, album.id, deviceId);
      setProposal(p);
      setFields(new Set(p.album.map((ch) => ch.field)));
      setPicked(new Set(p.tracks.filter((tp) => tp.writable && tp.changes.length > 0).map((tp) => tp.trackId)));
      setUseCover(p.cover != null);
    } catch (err) {
      setError(asError(err));
    } finally {
      setOpening(null);
    }
  };

  const back = () => {
    if (proposal) bridge().then((b) => b.retagForget(proposal.id));
    setProposal(null);
  };

  const close = () => {
    if (proposal && !lib.retag) bridge().then((b) => b.retagForget(proposal.id));
    onClose();
  };

  const count = useMemo(() => {
    if (!proposal) return 0;
    const all = fields.size > 0 || (useCover && proposal.cover != null);
    return proposal.tracks.filter((tp) => tp.writable && (all || tp.repairs || (picked.has(tp.trackId) && tp.changes.length > 0))).length;
  }, [proposal, fields, picked, useCover]);

  const write = async () => {
    if (!proposal || count === 0) return;
    setStarting(true);
    setError(null);
    try {
      const cover = useCover && proposal.cover != null;
      const shown = cover && proposal.cover ? { ...album, art: proposal.cover.art } : album;
      await lib.startRetag(proposal.id, deviceId, shown, [...fields], [...picked], cover);
    } catch (err) {
      setError(asError(err));
    } finally {
      setStarting(false);
    }
  };

  const head = (
    <header className="sheet-head meta-head">
      <span className="meta-head-cover" aria-hidden="true">
        <Cover art={album.art} title={album.title} />
      </span>
      <div className="meta-head-what">
        <span className="eyebrow">{t("meta.eyebrow")}</span>
        <h2>{album.title}</h2>
        <p>
          {album.artist ?? t(album.compilation ? "library.variousArtists" : "library.unknownArtist")}
          {" · "}
          <span className="num">{t("album.tracks", { n: album.tracks.length })}</span>
          {player && (
            <>
              {" · "}
              <span className="mono">{player.short}</span>
            </>
          )}
        </p>
      </div>
      <button className="icon-btn icon-btn--small" onClick={close} aria-label={t("send.close")}>
        <Icon name="close" size={16} />
      </button>
    </header>
  );

  if (lib.retag) {
    return (
      <>
        <div className="scrim" onClick={onClose} />
        <aside className="sheet meta-sheet" role="dialog" aria-modal="true" aria-label={t("meta.eyebrow")}>
          {head}
          <JobView player={player} onClose={onClose} />
        </aside>
      </>
    );
  }

  return (
    <>
      <div className="scrim" onClick={close} />
      <aside className="sheet meta-sheet" role="dialog" aria-modal="true" aria-label={t("meta.eyebrow")}>
        {head}
        <div className="sheet-body">
          {error && (
            <p className="plan-note is-warn selectable">
              <Icon name="alert" size={16} />
              <span>
                <ErrorText error={error} />
              </span>
            </p>
          )}
          {proposal ? (
            <ProposalView
              album={album}
              proposal={proposal}
              tracks={tracks}
              player={player}
              fields={fields}
              setFields={setFields}
              picked={picked}
              setPicked={setPicked}
              useCover={useCover}
              setUseCover={setUseCover}
            />
          ) : (
            <>
              <form className="meta-query" onSubmit={search}>
                <label className="meta-input">
                  <span>{t("meta.album")}</span>
                  <input value={albumQ} onChange={(e) => setAlbumQ(e.target.value)} spellCheck={false} />
                </label>
                <label className="meta-input">
                  <span>{t("meta.artist")}</span>
                  <input value={artistQ} onChange={(e) => setArtistQ(e.target.value)} spellCheck={false} />
                </label>
                <button className="btn btn--primary" type="submit" disabled={searching || !albumQ.trim()}>
                  <Icon name="search" size={16} />
                  {t("meta.search")}
                </button>
              </form>
              {searching ? (
                <div className="plan-reading">
                  <span className="spinner" />
                  {t("meta.searching")}
                </div>
              ) : (
                found && <Results found={found} opening={opening} onPick={pick} />
              )}
            </>
          )}
        </div>
        {proposal && (
          <footer className="sheet-foot">
            <button className="btn btn--ghost btn--large meta-back" onClick={back}>
              <Icon name="chevron" size={16} />
              {t("meta.back")}
            </button>
            <button className="btn btn--primary btn--large" disabled={count === 0 || starting} onClick={write}>
              <Icon name="check" size={17} />
              {count === 0 ? t("meta.nothing") : player ? t("meta.goPlayer", { device: player.short, n: count }) : t("meta.go", { n: count })}
            </button>
          </footer>
        )}
      </aside>
    </>
  );
}
