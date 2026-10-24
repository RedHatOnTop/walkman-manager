import { useEffect, useMemo, useState } from "react";
import { asError, bridge } from "../../api/bridge";
import type { Album, ErrorReport, PullPlan } from "../../api/types";
import { Cover } from "../../components/Cover";
import { Icon } from "../../components/Icon";
import { DeviceRender } from "../../components/render/DeviceRender";
import { bytes, percent } from "../../format";
import { usePrefs, useT } from "../../i18n";
import { artFor, screenFor } from "../../state/describe";
import { useLibrary, type MusicPlayer } from "../../state/library";
import { useEscape } from "./AlbumSheet";
import { ErrorText } from "../../components/ErrorText";

const RESERVE = 256 * 1024 * 1024;

function Transfer({ player, onClose }: { player: MusicPlayer | undefined; onClose(): void }) {
  const t = useT();
  const { lang } = usePrefs();
  const { pull, cancelPull, dismissPull, clear } = useLibrary();
  if (!pull) return null;
  const p = pull.progress;
  const r = pull.report;
  const pct = p && p.totalBytes > 0 ? Math.floor(percent(p.doneBytes, p.totalBytes)) : r ? 100 : 0;
  const art = artFor(player?.device.profileId);
  const finish = () => {
    if (r && !r.cancelled && r.failures.length === 0) clear();
    dismissPull();
    onClose();
  };
  return (
    <>
      <div className="sheet-body">
        <div className="xfer" aria-live="polite">
          <span className="eyebrow">
            {r ? (r.cancelled ? t("send.stopped") : t("pull.done")) : t("pull.copying")} · {player?.name ?? pull.deviceId}
          </span>
          <div className="xfer-top">
            <div className="xfer-figure num">
              {pct}
              <small>%</small>
            </div>
            {art && player && (
              <DeviceRender
                art={art}
                scale={0.95}
                turned={false}
                screen={r ? screenFor(player.device.mode, player.snapshot) : { kind: "busy" }}
                battery={player.snapshot?.power?.level}
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
                {r.verified > 0 && <p>{t("pull.verified", { n: r.verified })}</p>}
                {r.copied > 0 && (
                  <p>
                    {t("pull.added")} <bdi className="mono selectable">{pull.destRoot}</bdi>
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
            <button className="btn" onClick={cancelPull}>
              {t("send.stop")}
            </button>
          </>
        )}
      </footer>
    </>
  );
}

function PlanView({ plan }: { plan: PullPlan }) {
  const t = useT();
  const { lang } = usePrefs();
  const conflicts = plan.items.filter((i) => i.action === "conflict");
  const stat = (label: string, files: number, size: number) => (
    <div className={`plan-stat${files === 0 ? " is-zero" : ""}`}>
      <span className="plan-stat-label">{label}</span>
      <span className="plan-stat-value num">{files}</span>
      <span className="plan-stat-sub num">{bytes(size, lang)}</span>
    </div>
  );
  return (
    <>
      <div className="plan-stats">
        {stat(t("pull.plan.copy"), plan.copy.files, plan.copy.bytes)}
        {stat(t("pull.plan.present"), plan.present.files, plan.present.bytes)}
        {stat(t("pull.plan.conflict"), plan.conflict.files, plan.conflict.bytes)}
      </div>
      {conflicts.length > 0 && (
        <ul className="plan-list">
          {conflicts.map((i) => (
            <li key={i.local}>
              <span className="selectable">
                <bdi>{i.local.slice(plan.destRoot.length + 1)}</bdi>
              </span>
              <span>{t("pull.conflictNote")}</span>
            </li>
          ))}
        </ul>
      )}
      {plan.copy.files === 0 && (
        <p className="plan-note">
          <Icon name="check" size={16} />
          {t("pull.nothing")}
        </p>
      )}
      <p className="plan-note">
        <Icon name="shield" size={16} />
        {t("pull.never")}
      </p>
    </>
  );
}

export function PullSheet({ albums, player, onClose }: { albums: Album[]; player: MusicPlayer | undefined; onClose(): void }) {
  const t = useT();
  const { lang } = usePrefs();
  const lib = useLibrary();
  useEscape(onClose);

  const trackIds = useMemo(() => albums.flatMap((a) => a.tracks), [albums]);
  const trackKey = trackIds.join(",");
  const totalBytes = albums.reduce((n, a) => n + a.bytes, 0);
  const deviceId = player?.device.id ?? null;
  const [plan, setPlan] = useState<PullPlan | null>(null);
  const [planning, setPlanning] = useState(false);
  const [error, setError] = useState<ErrorReport | null>(null);
  const [starting, setStarting] = useState(false);

  useEffect(() => {
    if (!deviceId || !trackKey || lib.pull) return;
    let live = true;
    setPlanning(true);
    setPlan(null);
    setError(null);
    bridge()
      .then((b) => b.pullPlan(deviceId, trackKey.split(",")))
      .then(
        (p) => live && setPlan(p),
        (e) => live && setError(asError(e)),
      )
      .finally(() => live && setPlanning(false));
    return () => {
      live = false;
    };
  }, [deviceId, trackKey, lib.pull]);

  if (lib.pull) {
    return (
      <>
        <div className="scrim" onClick={onClose} />
        <aside className="sheet" role="dialog" aria-modal="true" aria-label={t("pull.title")}>
          <header className="sheet-head">
            <h2>{t("pull.title")}</h2>
            <button className="icon-btn icon-btn--small" onClick={onClose} aria-label={t("send.close")}>
              <Icon name="close" size={16} />
            </button>
          </header>
          <Transfer player={player} onClose={onClose} />
        </aside>
      </>
    );
  }

  const start = async () => {
    if (!plan || !deviceId) return;
    setStarting(true);
    try {
      await lib.startPull(plan.id, deviceId, plan.destRoot);
    } catch (e) {
      setError(asError(e));
    } finally {
      setStarting(false);
    }
  };

  const short = plan && plan.freeBytes != null && !plan.fits ? plan.copy.bytes + RESERVE - plan.freeBytes : 0;

  return (
    <>
      <div className="scrim" onClick={onClose} />
      <aside className="sheet" role="dialog" aria-modal="true" aria-label={t("pull.title")}>
        <header className="sheet-head send-head">
          <div className="send-what-covers" aria-hidden="true">
            {albums.slice(0, 5).map((a, i) => (
              <span key={a.id} style={{ zIndex: 10 - i }}>
                <Cover art={a.art} title={a.title} />
              </span>
            ))}
          </div>
          <div className="send-what">
            <h2>{t("pull.title")}</h2>
            <p className="num">{t("send.what", { albums: albums.length, tracks: trackIds.length, size: bytes(totalBytes, lang) })}</p>
          </div>
          <button className="icon-btn icon-btn--small" onClick={onClose} aria-label={t("send.cancel")}>
            <Icon name="close" size={16} />
          </button>
        </header>
        <div className="sheet-body">
          <section className="send-step">
            <h3 className="eyebrow">{t("pull.route")}</h3>
            <div className="pull-route">
              <span className="pull-end">
                {player && artFor(player.device.profileId) ? (
                  <DeviceRender art={artFor(player.device.profileId)!} scale={0.5} turned={false} screen={screenFor(player.device.mode, player.snapshot)} lang={lang} />
                ) : (
                  <Icon name="player" size={26} />
                )}
                <span className="pull-end-name">{player?.name}</span>
              </span>
              <span className="pull-arrow" aria-hidden="true">
                <span />
              </span>
              <span className="pull-end">
                <Icon name="computer" size={30} />
                <span className="pull-end-name">{t("library.source.pc")}</span>
              </span>
            </div>
            {plan && (
              <div className="pull-dest">
                <Icon name="folder" size={16} />
                <span className="pull-dest-path mono selectable">
                  <bdi>{plan.destRoot}</bdi>
                </span>
                {plan.freeBytes != null && (
                  <span className={`pull-dest-free num${plan.fits ? "" : " is-warn"}`}>
                    {plan.fits ? t("storage.free", { v: bytes(plan.freeBytes, lang) }) : t("pull.noSpace", { short: bytes(short, lang) })}
                  </span>
                )}
              </div>
            )}
          </section>

          <section className="send-step">
            {planning && (
              <div className="plan-reading">
                <span className="spinner" />
                {t("pull.reading")}
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
        </div>
        <footer className="sheet-foot">
          <button className="btn btn--ghost btn--large" onClick={onClose}>
            {t("send.cancel")}
          </button>
          <button className="btn btn--primary btn--large" disabled={!plan || !plan.fits || plan.copy.files === 0 || starting} onClick={start}>
            <Icon name="receive" size={17} />
            {t("pull.go", { n: plan?.copy.files ?? 0 })}
          </button>
        </footer>
      </aside>
    </>
  );
}
