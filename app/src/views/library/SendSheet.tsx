import { useEffect, useMemo, useState } from "react";
import { asError, bridge } from "../../api/bridge";
import type { ErrorReport, Plan, Volume } from "../../api/types";
import { Cover } from "../../components/Cover";
import { Icon } from "../../components/Icon";
import { DeviceRender } from "../../components/render/DeviceRender";
import { bytes, percent } from "../../format";
import { usePrefs, useT } from "../../i18n";
import { artFor, screenFor, shortSystem } from "../../state/describe";
import { useLibrary, type MusicPlayer } from "../../state/library";
import { codecName } from "../../state/music";
import { useEscape } from "./AlbumSheet";
import { ErrorText } from "../../components/ErrorText";

const RESERVE = 256 * 1024 * 1024;

function Transfer({ players, onClose }: { players: MusicPlayer[]; onClose(): void }) {
  const t = useT();
  const { lang } = usePrefs();
  const { sync, cancelSync, dismissSync, clear } = useLibrary();
  if (!sync) return null;
  const p = sync.progress;
  const r = sync.report;
  const pct = p && p.totalBytes > 0 ? Math.floor(percent(p.doneBytes, p.totalBytes)) : r ? 100 : 0;
  const target = players.find((x) => x.device.id === sync.deviceId);
  const name = target?.name ?? sync.deviceId;
  const art = artFor(target?.device.profileId);
  const finish = () => {
    if (r && !r.cancelled && r.failures.length === 0) clear();
    dismissSync();
    onClose();
  };
  return (
    <>
      <div className="sheet-body">
        <div className="xfer" aria-live="polite">
          <span className="eyebrow">
            {r ? (r.cancelled ? t("send.stopped") : t("send.done")) : t("send.sending")} · {name}
          </span>
          <div className="xfer-top">
            <div className="xfer-figure num">
              {pct}
              <small>%</small>
            </div>
            {art && (
              <DeviceRender
                art={art}
                scale={0.95}
                turned={false}
                screen={r ? screenFor(target!.device.mode, target!.snapshot) : { kind: "busy" }}
                battery={target?.snapshot?.power?.level}
                lang={lang}
              />
            )}
          </div>
          <div className={`xfer-bar${r ? " is-done" : ""}`}>
            <span style={{ width: `${pct}%` }} />
          </div>
          {p && (
            <div className="xfer-meta num">
              <span>{t("send.files", { done: p.doneFiles, total: p.totalFiles })}</span>
              <span>
                {bytes(p.doneBytes, lang)} / {bytes(p.totalBytes, lang)}
              </span>
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
                {r.verified > 0 && <p>{t("send.verified", { n: r.verified })}</p>}
                {r.scanned.length > 0 && <p>{t("send.scanned")}</p>}
                {r.scanError && (
                  <p className="selectable">
                    {t("send.scanFailed")}
                    <span className="err-detail">
                      <ErrorText error={r.scanError} />
                    </span>
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
            <button className="btn" onClick={cancelSync}>
              {t("send.stop")}
            </button>
          </>
        )}
      </footer>
    </>
  );
}

function VolumeChoice({ v, on, plan, onPick }: { v: Volume; on: boolean; plan: Plan | null; onPick(): void }) {
  const t = useT();
  const { lang } = usePrefs();
  const add = on && plan ? plan.neededBytes : 0;
  const over = on && plan ? !plan.fits : false;
  return (
    <button className={`choice choice--volume${on ? " is-on" : ""}`} onClick={onPick} aria-pressed={on}>
      <Icon name={v.kind === "card" ? "card" : "chip"} size={18} />
      <span className="choice-text">
        <span className="choice-name">{t(v.kind === "card" ? "storage.card" : "storage.internal")}</span>
        <span className="choice-sub num">
          {on && plan && plan.neededBytes > 0
            ? plan.fits
              ? t("send.after", { free: bytes(Math.max(0, v.freeBytes - plan.neededBytes), lang) })
              : t("send.noSpace", { short: bytes(plan.neededBytes + RESERVE - v.freeBytes, lang) })
            : t("storage.free", { v: bytes(v.freeBytes, lang) })}
        </span>
        <span className="capacity" aria-hidden="true">
          <span className="capacity-used" style={{ width: `${percent(v.usedBytes, v.totalBytes)}%` }} />
          {add > 0 && <span className={`capacity-add${over ? " is-over" : ""}`} style={{ width: `${Math.max(0.6, percent(add, v.totalBytes))}%` }} />}
        </span>
      </span>
      <span className="radio" />
    </button>
  );
}

function PlanView({ plan }: { plan: Plan }) {
  const t = useT();
  const { lang } = usePrefs();
  const { tracks } = useLibrary();
  const replace = plan.items.filter((i) => i.action === "replace").length;
  const unplayable = plan.items.filter((i) => i.action === "unplayable");
  const stat = (label: string, files: number, size: number, sub?: string) => (
    <div className={`plan-stat${files === 0 ? " is-zero" : ""}`}>
      <span className="plan-stat-label">{label}</span>
      <span className="plan-stat-value num">{files}</span>
      <span className="plan-stat-sub num">{sub ?? bytes(size, lang)}</span>
    </div>
  );
  return (
    <>
      <div className="plan-stats">
        {stat(
          t("send.plan.copy"),
          plan.copy.files,
          plan.copy.bytes,
          replace > 0 ? `${bytes(plan.copy.bytes, lang)} · ${t("send.plan.copyDetail", { copy: plan.copy.files - replace, replace })}` : undefined,
        )}
        {stat(t("send.plan.skip"), plan.skip.files, plan.skip.bytes)}
        {stat(t("send.plan.unplayable"), plan.unplayable.files, plan.unplayable.bytes)}
      </div>
      {unplayable.length > 0 && (
        <ul className="plan-list">
          {unplayable.map((i) => {
            const tr = i.trackId ? tracks.get(i.trackId) : undefined;
            return (
              <li key={i.dest}>
                <span>{tr?.title ?? i.dest}</span>
                <span>
                  {i.reason === "codec" && tr
                    ? t("send.reason.codec", { codec: codecName(tr.format.codec) })
                    : t(`send.reason.${i.reason ?? "codec"}`, { codec: "" })}
                </span>
              </li>
            );
          })}
        </ul>
      )}
      {plan.copy.files === 0 && (
        <p className="plan-note">
          <Icon name="check" size={16} />
          {t("send.nothing")}
        </p>
      )}
      <p className="plan-note">
        <Icon name="shield" size={16} />
        {t("send.never")}
      </p>
    </>
  );
}

export function SendSheet({ albumIds, players, onClose }: { albumIds: string[]; players: MusicPlayer[]; onClose(): void }) {
  const t = useT();
  const { lang } = usePrefs();
  const lib = useLibrary();
  useEscape(onClose);

  const albums = useMemo(
    () => albumIds.map((id) => lib.state?.index.albums.find((a) => a.id === id)).filter((a) => a != null),
    [albumIds, lib.state],
  );
  const trackIds = useMemo(() => albums.flatMap((a) => a.tracks), [albums]);
  const trackKey = trackIds.join(",");
  const totalBytes = albums.reduce((n, a) => n + a.bytes, 0);

  const [deviceId, setDeviceId] = useState<string | null>(players[0]?.device.id ?? null);
  const player = players.find((p) => p.device.id === deviceId) ?? null;
  const volumes = player?.snapshot?.volumes ?? [];
  const [volume, setVolume] = useState<Volume["kind"] | null>(null);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [planning, setPlanning] = useState(false);
  const [error, setError] = useState<ErrorReport | null>(null);
  const [starting, setStarting] = useState(false);

  useEffect(() => {
    if (!deviceId && players[0]) setDeviceId(players[0].device.id);
  }, [deviceId, players]);

  // Each player starts on its card when it has one: that is where most libraries live.
  const volumeKinds = volumes.map((v) => v.kind).join("|");
  useEffect(() => {
    const kinds = volumeKinds ? (volumeKinds.split("|") as Volume["kind"][]) : [];
    setVolume(kinds.includes("card") ? "card" : (kinds[0] ?? null));
  }, [deviceId, volumeKinds]);

  useEffect(() => {
    if (!deviceId || !volume || !trackKey) return;
    let live = true;
    setPlanning(true);
    setPlan(null);
    setError(null);
    bridge()
      .then((b) => b.syncPlan(deviceId, volume, trackKey.split(",")))
      .then(
        (p) => live && setPlan(p),
        (e) => live && setError(asError(e)),
      )
      .finally(() => live && setPlanning(false));
    return () => {
      live = false;
    };
  }, [deviceId, volume, trackKey]);

  if (lib.sync) {
    return (
      <>
        <div className="scrim" onClick={onClose} />
        <aside className="sheet" role="dialog" aria-modal="true" aria-label={t("send.title")}>
          <header className="sheet-head">
            <h2>{t("send.title")}</h2>
            <button className="icon-btn icon-btn--small" onClick={onClose} aria-label={t("send.close")}>
              <Icon name="close" size={16} />
            </button>
          </header>
          <Transfer players={players} onClose={onClose} />
        </aside>
      </>
    );
  }

  const start = async () => {
    if (!plan || !deviceId) return;
    setStarting(true);
    try {
      await lib.startSync(plan.id, deviceId);
    } catch (e) {
      setError(asError(e));
    } finally {
      setStarting(false);
    }
  };

  return (
    <>
      <div className="scrim" onClick={onClose} />
      <aside className="sheet" role="dialog" aria-modal="true" aria-label={t("send.title")}>
        <header className="sheet-head send-head">
          <div className="send-what-covers" aria-hidden="true">
            {albums.slice(0, 5).map((a, i) => (
              <span key={a.id} style={{ zIndex: 10 - i }}>
                <Cover art={a.art} title={a.title} />
              </span>
            ))}
          </div>
          <div className="send-what">
            <h2>{t("send.title")}</h2>
            <p className="num">{t("send.what", { albums: albums.length, tracks: trackIds.length, size: bytes(totalBytes, lang) })}</p>
          </div>
          <button className="icon-btn icon-btn--small" onClick={onClose} aria-label={t("send.cancel")}>
            <Icon name="close" size={16} />
          </button>
        </header>
        <div className="sheet-body">
          {players.length === 0 ? (
            <p className="plan-note is-warn">
              <Icon name="alert" size={16} />
              {t("send.noPlayers")}
            </p>
          ) : (
            <>
              <section className="send-step">
                <h3 className="eyebrow">{t("send.player")}</h3>
                <div className="choices">
                  {players.map((p) => {
                    const art = artFor(p.device.profileId);
                    const on = p.device.id === deviceId;
                    return (
                      <button key={p.device.id} className={`choice${on ? " is-on" : ""}`} onClick={() => setDeviceId(p.device.id)} aria-pressed={on}>
                        <span className="choice-art">
                          {art ? (
                            <DeviceRender art={art} scale={0.78} turned={false} screen={on ? screenFor(p.device.mode, p.snapshot) : { kind: "off" }} battery={p.snapshot?.power?.level} lang={lang} />
                          ) : (
                            <Icon name="player" size={28} />
                          )}
                        </span>
                        <span className="choice-text">
                          <span className="choice-name">{p.name}</span>
                          <span className="choice-sub">{p.snapshot ? shortSystem(p.snapshot) : t("device.reading")}</span>
                        </span>
                        <span className="radio" />
                      </button>
                    );
                  })}
                </div>
              </section>

              {player?.snapshot && volumes.length === 0 && (
                <p className="plan-note is-warn">
                  <Icon name="alert" size={16} />
                  {t(player.device.links.includes("mass-storage") ? "storage.exported" : "storage.none")}
                </p>
              )}
              {volumes.length > 0 && (
                <section className="send-step">
                  <h3 className="eyebrow">{t("send.volume")}</h3>
                  <div className="choices">
                    {volumes.map((v) => (
                      <VolumeChoice key={v.mount} v={v} on={v.kind === volume} plan={v.kind === volume ? plan : null} onPick={() => setVolume(v.kind)} />
                    ))}
                  </div>
                </section>
              )}

              <section className="send-step">
                {planning && (
                  <div className="plan-reading">
                    <span className="spinner" />
                    {t("send.reading")}
                  </div>
                )}
                {error && (
                  <p className="plan-note is-warn selectable">
                    <Icon name="alert" size={16} />
                    <span>
                      <ErrorText error={error} />
                    </span>
                  </p>
                )}
                {plan && <PlanView plan={plan} />}
              </section>
            </>
          )}
        </div>
        <footer className="sheet-foot">
          <button className="btn btn--ghost btn--large" onClick={onClose}>
            {t("send.cancel")}
          </button>
          <button className="btn btn--primary btn--large" disabled={!plan || !plan.fits || plan.copy.files === 0 || starting} onClick={start}>
            <Icon name="send" size={17} />
            {t("send.go", { n: plan?.copy.files ?? 0 })}
          </button>
        </footer>
      </aside>
    </>
  );
}
