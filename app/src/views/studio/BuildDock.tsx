import { useEffect, useState } from "react";
import { asError, bridge } from "../../api/bridge";
import type { BuildOptions, BuildProgress, BuildRecord, ErrorReport, HostCheck, Kit, StudioView } from "../../api/types";
import { Icon } from "../../components/Icon";
import { bytes } from "../../format";
import { usePrefs, type StringKey } from "../../i18n";
import type { Route } from "../../routes";
import { useData } from "../../state/data";
import { LicenceSheet } from "./LicenceSheet";

export function clock(secs: number): string {
  const s = Math.max(0, Math.round(secs));
  const m = Math.floor(s / 60);
  return `${m}:${String(s % 60).padStart(2, "0")}`;
}

function when(secs: number, lang: ReturnType<typeof usePrefs>["lang"]): string {
  return new Date(secs * 1000).toLocaleString(lang === "ko" ? "ko-KR" : "en-GB", { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

/** A step's own name when the app has a sentence for it, else the kit's word for it. */
export function stepName(name: string, t: ReturnType<typeof usePrefs>["t"]): { text: string; raw: boolean } {
  const key = `build.step.${name}` as StringKey;
  const said = t(key);
  return said === key ? { text: name, raw: true } : { text: said, raw: false };
}

function Row({ label, state, children, action }: { label: string; state: "ok" | "todo" | "bad"; children: React.ReactNode; action?: React.ReactNode }) {
  return (
    <div className={`dock-row dock-row--${state}`}>
      <span className="dock-mark">
        <Icon name={state === "ok" ? "check" : state === "bad" ? "alert" : "info"} size={14} />
      </span>
      <span className="dock-label">{label}</span>
      <span className="dock-value">{children}</span>
      {action && <span className="dock-act">{action}</span>}
    </div>
  );
}

/** What a kit's build needs besides the files: the kit itself, the player's key, the licence of
 * what the kit downloads, this computer. */
export function BuildDock({ kit, view, go, onView, onError }: { kit: Kit; view: StudioView; go(r: Route): void; onView(v: StudioView): void; onError(e: ErrorReport): void }) {
  const { t, lang } = usePrefs();
  const { scan } = useData();
  const [host, setHost] = useState<{ checks: HostCheck[]; ok: boolean } | null>(null);
  const [showHost, setShowHost] = useState(false);
  const [options, setOptions] = useState<BuildOptions>({ variant: "release", root: false });
  const [reading, setReading] = useState(false);
  const [licence, setLicence] = useState(false);
  const repo = view.repos[kit.id];
  const licenceNeeded = kit.id in view.licences;
  const agreed = view.licences[kit.id] ?? false;
  const keyNeeded = kit.profile in view.keys;
  const key = view.keys[kit.profile] ?? false;
  const player = scan.devices.find((d) => d.profileId === kit.profile && d.mode === "running");

  const checkHost = () =>
    void bridge()
      .then((b) => b.studioHost(kit.id))
      .then(setHost, (e) => onError(asError(e)));
  useEffect(checkHost, [kit.id]);

  const pickRepo = () =>
    void bridge()
      .then((b) => b.studioPickRepo(kit.id))
      .then(onView, (e) => onError(asError(e)));
  const readKey = () => {
    if (!player) return;
    setReading(true);
    void bridge()
      .then((b) => b.studioReadKey(player.id))
      .then(onView, (e) => onError(asError(e)))
      .finally(() => setReading(false));
  };
  const build = (o: BuildOptions) =>
    void bridge()
      .then((b) => b.studioBuild(kit.id, o))
      .catch((e) => onError(asError(e)));

  const adbKey = host?.checks.find((c) => c.kind === "adbKey");
  const missing = host?.checks.filter((c) => !c.ok && c.kind !== "adbKey") ?? [];
  const repoOk = repo?.state === "ready" || repo?.state === "published";
  const ready = kit.ready && repoOk && (!keyNeeded || key) && (!licenceNeeded || agreed) && !!host?.ok && (options.variant === "release" || !!adbKey?.ok);
  // Going back takes the UPG and the key, not the licence: it downloads nothing.
  const backReady = kit.restore === true && repoOk && (!keyNeeded || key) && !!host?.ok;
  // The newest set this kit made with these options, so the dock goes on to installing it.
  const made = view.builds.find(
    (r) => r.kit === kit.id && r.outcome.kind === "done" && !!(r.set || r.storage) && r.options.variant === options.variant && (options.variant !== "release" || r.options.root === options.root),
  );
  const madeBack = view.builds.find((r) => r.kit === kit.id && r.outcome.kind === "done" && !!r.set && r.options.variant === "stock");

  return (
    <div className="dock">
      <div className="dock-rows">
        {repo?.state === "published" ? (
          <Row label={t("build.kit")} state={repo.have ? "ok" : "todo"}>
            {repo.have ? t("build.kit.have") : t("build.kit.fetch", { size: bytes(repo.bytes, lang) })}
          </Row>
        ) : (
          <Row
            label={t("build.repo")}
            state={repoOk ? "ok" : repo?.state === "unset" ? "todo" : "bad"}
            action={
              <button className="btn btn--small btn--ghost" onClick={pickRepo}>
                <Icon name="folder" size={14} />
                {repo?.state === "unset" ? t("build.repo.pick") : t("build.repo.change")}
              </button>
            }
          >
            {repo && repo.state !== "unset" ? (
              <>
                <span className="mono selectable" title={repo.path}>
                  {repo.path.split(/[\\/]/).pop()}
                </span>{" "}
                <span>{t(`build.repo.${repo.state}` as StringKey)}</span>
              </>
            ) : (
              t("build.repo.unset")
            )}
          </Row>
        )}
        {keyNeeded && (
          <Row
            label={t("build.key")}
            state={key ? "ok" : "todo"}
            action={
              <button className="btn btn--small btn--ghost" onClick={readKey} disabled={!player || reading}>
                <Icon name="player" size={14} />
                {key ? t("build.key.again") : t("build.key.read")}
              </button>
            }
          >
            {key ? t("build.key.have") : player ? t("build.key.ready") : t("build.key.connect")}
          </Row>
        )}
        {licenceNeeded && (
          <Row
            label={t("build.licence")}
            state={agreed ? "ok" : "todo"}
            action={
              <button className="btn btn--small btn--ghost" onClick={() => setLicence(true)}>
                <Icon name="info" size={14} />
                {agreed ? t("build.licence.again") : t("build.licence.read")}
              </button>
            }
          >
            {agreed ? t("build.licence.agreed") : t("build.licence.todo")}
          </Row>
        )}
        <Row
          label={t("build.host")}
          state={!host ? "todo" : host.ok ? "ok" : "bad"}
          action={
            <button className="btn btn--small btn--ghost" onClick={() => setShowHost((v) => !v)}>
              {showHost ? t("build.host.hide") : t("build.host.show")}
            </button>
          }
        >
          {!host ? t("build.host.checking") : host.ok ? t("build.host.ok") : t("build.host.missing", { n: missing.length })}
        </Row>
        {showHost && host && (
          <ul className="dock-host">
            {host.checks.map((c) => (
              <li key={`${c.kind}:${c.name}`} className={c.ok ? "is-ok" : "is-bad"}>
                <Icon name={c.ok ? "check" : "alert"} size={13} />
                <span className="dock-host-name">{t(`build.check.${c.kind}` as StringKey, { name: c.name, gb: c.name })}</span>
                <span className="dock-host-found mono selectable">
                  {c.kind === "space" && c.found ? bytes(Number(c.found), lang) : c.found ?? "—"}
                </span>
              </li>
            ))}
            <li className="dock-host-again">
              <button className="btn-link" onClick={checkHost}>
                {t("build.host.again")}
              </button>
            </li>
          </ul>
        )}
      </div>

      <div className="dock-options">
        {kit.variants && (
          <div className="segmented" role="tablist" aria-label={t("build.variant")}>
            {(["release", "debug"] as const).map((v) => (
              <button key={v} className={options.variant === v ? "is-on" : ""} onClick={() => setOptions((o) => ({ ...o, variant: v }))}>
                {t(`build.variant.${v}` as StringKey)}
              </button>
            ))}
          </div>
        )}
        {!kit.variants ? null : options.variant === "release" ? (
          <label className="dock-switch">
            <input type="checkbox" checked={options.root} onChange={(e) => setOptions((o) => ({ ...o, root: e.target.checked }))} />
            <span>{t("build.root")}</span>
          </label>
        ) : (
          <span className="dock-note">{adbKey?.ok ? t("build.debug.note") : t("build.debug.noKey")}</span>
        )}
        <div className="dock-gos">
          {made && <span className="dock-made num">{t("build.made", { when: when(made.started, lang) })}</span>}
          {made ? (
            <>
              <button className="btn btn--ghost" onClick={() => build(options)} disabled={!ready}>
                {t("build.again")}
              </button>
              <button className="btn btn--primary dock-go" onClick={() => go({ kind: "install", build: made.id })}>
                <Icon name="send" size={16} />
                {t("build.install")}
              </button>
            </>
          ) : (
            <button className={`btn dock-go${kit.ready ? " btn--primary" : ""}`} onClick={() => build(options)} disabled={!ready}>
              <Icon name="firmware" size={16} />
              {t("build.start")}
            </button>
          )}
        </div>
      </div>
      <p className="dock-what">{kit.set === "storage" ? t("build.storage.what") : t(`build.variant.${options.variant}.what` as StringKey)}</p>
      {kit.restore !== null && (
        <div className="dock-back">
          <span className="dock-back-mark">
            <Icon name="back" size={16} />
          </span>
          <div className="dock-back-text">
            <span className="dock-back-title">{t("build.back")}</span>
            <span className="dock-back-what">{backReady ? t("build.back.what") : t("build.back.needs")}</span>
          </div>
          <span className="dock-back-act">
            <button className="btn btn--small btn--ghost" onClick={() => build({ variant: "stock", root: false })} disabled={!backReady}>
              {madeBack ? t("build.back.again") : t("build.back.start")}
            </button>
            {madeBack && (
              <button className="btn btn--small" onClick={() => go({ kind: "install", build: madeBack.id })}>
                <Icon name="back" size={14} />
                {t("build.back.go")}
              </button>
            )}
          </span>
        </div>
      )}
      {licence && (
        <LicenceSheet
          what={t("licence.what")}
          read={() => bridge().then((b) => b.studioLicence(kit.id))}
          agree={(sha256) => bridge().then((b) => b.studioAgree(kit.id, sha256)).then(onView)}
          onClose={() => setLicence(false)}
          onError={onError}
        />
      )}
    </div>
  );
}

/** The build running now: its steps as a strip, the current one, the newest log lines. */
export function BuildRun({ progress }: { progress: BuildProgress }) {
  const { t } = usePrefs();
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);
  const total = Math.max(progress.total, 1);
  const step = progress.step ? stepName(progress.step, t) : null;
  return (
    <div className="brun" aria-live="polite">
      <div className="brun-head">
        <div className="brun-titles">
          <span className="eyebrow">
            {progress.phase === "fetch"
              ? t("build.fetching")
              : progress.phase === "copy"
                ? t("build.copying")
                : t("build.stepOf", { n: progress.index, m: progress.total || "…" })}
          </span>
          <span className={`brun-step${step?.raw ? " mono" : ""}`}>{step?.text ?? t("build.starting")}</span>
        </div>
        <span className="brun-time num">{clock((now - progress.started * 1000) / 1000)}</span>
        <button className="btn btn--ghost" onClick={() => void bridge().then((b) => b.studioBuildCancel())}>
          {t("studio.stop")}
        </button>
      </div>
      <div className="brun-strip" style={{ gridTemplateColumns: `repeat(${total}, 1fr)` }}>
        {Array.from({ length: total }, (_, i) => (
          <span key={i} className={i + 1 < progress.index ? "is-done" : i + 1 === progress.index ? "is-now" : ""} />
        ))}
      </div>
      {progress.lines.length > 0 && <pre className="brun-log mono">{progress.lines.slice(-6).join("\n")}</pre>}
    </div>
  );
}

function outcomeText(r: BuildRecord, t: ReturnType<typeof usePrefs>["t"]): string {
  if (r.outcome.kind === "failed") {
    const step = r.outcome.step ? stepName(r.outcome.step, t).text : null;
    return step ? t("build.outcome.failedAt", { step }) : t("build.outcome.failed");
  }
  return t(`build.outcome.${r.outcome.kind}` as StringKey);
}

/** Every build, newest first; a finished one goes on to the install. */
export function Builds({ view, go, onView, onError }: { view: StudioView; go(r: Route): void; onView(v: StudioView): void; onError(e: ErrorReport): void }) {
  const { t, lang } = usePrefs();
  const [open, setOpen] = useState<string | null>(null);
  const forget = (id: string) =>
    void bridge()
      .then((b) => b.studioForgetBuild(id))
      .then(onView, (e) => onError(asError(e)));
  return (
    <section className="studio-builds">
      <div className="studio-section-head">
        <h2 className="studio-section-title">{t("build.list")}</h2>
        <p className="studio-section-note">{t("build.listNote")}</p>
      </div>
      <div className="build-list">
        {view.builds.map((r) => {
          const secs = r.ended ? r.ended - r.started : null;
          const files = r.set?.files ?? r.storage?.files;
          const size = files ? Object.values(files).reduce((a, f) => a + f.bytes, 0) : 0;
          const date = when(r.started, lang);
          return (
            <article key={r.id} className={`tile build build--${r.outcome.kind}`}>
              <div className="build-main">
                <span className="build-date num">{date}</span>
                <span className="build-tags">
                  {view.kits.find((k) => k.id === r.kit)?.variants !== false && (
                    <span className="chip">{t(`build.variant.${r.options.variant}` as StringKey)}</span>
                  )}
                  {r.storage && <span className="chip">{t(`build.key.${r.storage.upg.key}` as StringKey)}</span>}
                  {r.set?.root && <span className="chip">{t("install.root")}</span>}
                  <span className="mono build-commit">{r.commit.slice(0, 7)}</span>
                </span>
                <span className={`build-outcome build-outcome--${r.outcome.kind}`}>
                  {r.outcome.kind === "done" ? <Icon name="check" size={14} /> : r.outcome.kind === "failed" ? <Icon name="alert" size={14} /> : null}
                  {outcomeText(r, t)}
                  {secs != null && <span className="num"> · {clock(secs)}</span>}
                </span>
              </div>
              <div className="build-side">
                {files && <span className="build-size num">{bytes(size, lang)}</span>}
                <button className="icon-btn icon-btn--small" title={t("build.folder")} aria-label={t("build.folder")} onClick={() => void bridge().then((b) => b.studioOpenBuild(r.id))}>
                  <Icon name="folder" size={15} />
                </button>
                <button className="icon-btn icon-btn--small" title={t("build.details")} aria-label={t("build.details")} onClick={() => setOpen(open === r.id ? null : r.id)}>
                  <Icon name="info" size={15} />
                </button>
                <button className="icon-btn icon-btn--small" title={t("build.forget")} aria-label={t("build.forget")} onClick={() => forget(r.id)}>
                  <Icon name="close" size={15} />
                </button>
                {r.outcome.kind === "done" && (r.set || r.storage) && (
                  <button className="btn btn--small" onClick={() => go({ kind: "install", build: r.id })}>
                    <Icon name={r.options.variant === "stock" ? "back" : "send"} size={14} />
                    {t(r.options.variant === "stock" ? "build.back.go" : "build.install")}
                  </button>
                )}
              </div>
              {open === r.id && (
                <div className="build-more">
                  {r.set && (
                    <ul className="build-files">
                      {r.set.flash.map((f) => (
                        <li key={f.partition}>
                          <span className="mono">{f.partition}</span>
                          <span className="mono">{f.file}</span>
                          <span className="num">{bytes(r.set!.files[f.file]?.bytes ?? 0, lang)}</span>
                          <span className="mono build-hash selectable">{r.set!.files[f.file]?.sha256.slice(0, 16)}</span>
                        </li>
                      ))}
                    </ul>
                  )}
                  {r.storage && (
                    <ul className="build-files build-files--drive">
                      {r.storage.copy.map((path) => (
                        <li key={path}>
                          <span className="mono build-path">
                            <span className="build-path-dir">{path.slice(0, path.lastIndexOf("/") + 1)}</span>
                            {path.slice(path.lastIndexOf("/") + 1)}
                          </span>
                          <span className="num">{bytes(r.storage!.files[path]?.bytes ?? 0, lang)}</span>
                          <span className="mono build-hash selectable">{r.storage!.files[path]?.sha256.slice(0, 16)}</span>
                        </li>
                      ))}
                    </ul>
                  )}
                  {r.outcome.kind === "failed" && <pre className="brun-log mono selectable">{r.outcome.tail.join("\n")}</pre>}
                  <p className="build-dir">
                    <span className="mono selectable">{r.dir}</span>
                  </p>
                </div>
              )}
            </article>
          );
        })}
      </div>
    </section>
  );
}
