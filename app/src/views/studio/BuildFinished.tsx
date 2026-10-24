import { useEffect, useRef, useState } from "react";
import { bridge } from "../../api/bridge";
import type { BuildDone, BuildRecord, Kit } from "../../api/types";
import { ErrorText } from "../../components/ErrorText";
import { Icon } from "../../components/Icon";
import { DeviceRender } from "../../components/render/DeviceRender";
import { bytes } from "../../format";
import { usePrefs, type StringKey } from "../../i18n";
import type { Route } from "../../routes";
import { artFor } from "../../state/describe";
import { clock, stepName } from "./BuildDock";

function setBytes(r: BuildRecord): number | null {
  const files = r.set?.files ?? r.storage?.files;
  return files ? Object.values(files).reduce((a, f) => a + f.bytes, 0) : null;
}

/** A build ending, wherever in the app the owner is by then: what it made and the way on to installing
 * it, or the step it stopped at. Builds run for many minutes and the Studio lists them far down. */
export function BuildFinished({ route, go }: { route: Route; go(r: Route): void }) {
  const { t, lang } = usePrefs();
  const [shown, setShown] = useState<(BuildDone & { kit: Kit | null }) | null>(null);
  const here = useRef(route);
  useEffect(() => {
    here.current = route;
  }, [route]);

  useEffect(() => {
    let live = true;
    const offs: Promise<() => void>[] = [];
    void bridge().then((b) => {
      offs.push(
        b.onStudioBuildDone((d) => {
          const r = d.record;
          // The owner stopped it, or the Studio's own notice already says why it could not start.
          if (r ? r.outcome.kind !== "done" && r.outcome.kind !== "failed" : !d.error || here.current.kind === "studio") return;
          // A stock set is only ever made to go back with, so from the Studio the page goes on to that.
          if (r?.options.variant === "stock" && r.outcome.kind === "done" && here.current.kind === "studio") {
            go({ kind: "install", build: r.id });
            return;
          }
          void b.studioView().then(
            (v) => live && setShown({ ...d, kit: v.kits.find((k) => k.id === r?.kit) ?? null }),
            () => live && setShown({ ...d, kit: null }),
          );
        }),
      );
    });
    return () => {
      live = false;
      offs.forEach((o) => void o.then((f) => f()));
    };
  }, [go]);

  const close = () => setShown(null);
  useEffect(() => {
    if (!shown) return;
    const key = (e: KeyboardEvent) => e.key === "Escape" && setShown(null);
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [shown]);

  if (!shown) return null;
  const r = shown.record;
  const kit = shown.kit;
  const ok = r?.outcome.kind === "done" && !!(r.set || r.storage);
  const stock = r?.options.variant === "stock";
  const art = artFor(kit?.profile);
  const secs = r?.ended ? r.ended - r.started : null;
  const size = r ? setBytes(r) : null;
  const failedAt = r?.outcome.kind === "failed" && r.outcome.step ? stepName(r.outcome.step, t) : null;
  const tail = r?.outcome.kind === "failed" ? r.outcome.tail.slice(-8) : [];
  const variant = r && kit?.variants !== false ? t(`build.variant.${r.options.variant}` as StringKey) + (r.set?.root ? ` · ${t("install.root")}` : "") : null;

  const rows: [string, React.ReactNode | null][] = [
    [t("build.kit"), r ? t(`studio.kit.${r.kit}.name` as StringKey) : null],
    [t("build.variant"), variant],
    [t("build.key"), r?.storage ? t(`build.key.${r.storage.upg.key}` as StringKey) : null],
    [t("build.done.stoppedAt"), failedAt && <span className={`bdone-bad${failedAt.raw ? " mono" : ""}`}>{failedAt.text}</span>],
    [t("build.done.took"), secs != null ? <span className="num">{clock(secs)}</span> : null],
    [t("build.done.size"), ok && size != null ? <span className="num">{bytes(size, lang)}</span> : null],
  ];

  const install = () => {
    close();
    if (r) go({ kind: "install", build: r.id });
  };
  const toStudio = () => {
    close();
    go({ kind: "studio" });
  };

  return (
    <>
      <div className="scrim" onClick={close} />
      <div className={`sheet bdone${ok ? " is-done" : " is-failed"}`} role="dialog" aria-modal="true" aria-labelledby="bdone-title">
        <div className="bdone-object">{art && <DeviceRender art={art} scale={2.6} screen={{ kind: "off" }} />}</div>
        <div className="bdone-body">
          <h2 id="bdone-title" className="bdone-title">
            {ok ? (stock ? t("build.done.stock") : t("build.done.title")) : t("build.done.failed")}
          </h2>
          {r && r.total > 0 && (
            <div className="brun-strip bdone-strip" style={{ gridTemplateColumns: `repeat(${r.total}, 1fr)` }} aria-hidden="true">
              {Array.from({ length: r.total }, (_, i) => (
                <span key={i} className={ok || i < r.steps.length ? "is-done" : i === r.steps.length ? "is-bad" : ""} />
              ))}
            </div>
          )}
          <dl className="specs bdone-specs">
            {rows
              .filter(([, v]) => v != null && v !== "")
              .map(([k, v]) => (
                <div className="spec" key={k}>
                  <dt>{k}</dt>
                  <dd>{v}</dd>
                </div>
              ))}
          </dl>
          {shown.error && !r && (
            <p className="bdone-error selectable">
              <ErrorText error={shown.error} />
            </p>
          )}
          {tail.length > 0 && <pre className="brun-log mono selectable">{tail.join("\n")}</pre>}
          <p className="bdone-note">
            {ok
              ? `${t(stock ? "build.done.note.stock" : kit?.set === "storage" ? "build.done.note.storage" : "build.done.note.set")} ${t("build.done.later.note")}`
              : t("build.done.failed.note")}
          </p>
          <div className="bdone-act">
            {ok ? (
              <button className="btn btn--primary bdone-go" onClick={install} autoFocus>
                <Icon name={stock ? "back" : "send"} size={17} />
                {t(stock ? "build.back.go" : "build.install")}
              </button>
            ) : (
              route.kind !== "studio" && (
                <button className="btn bdone-go" onClick={toStudio} autoFocus>
                  {t("build.done.see")}
                </button>
              )
            )}
            <button className="btn btn--ghost" onClick={close} autoFocus={!ok && route.kind === "studio"}>
              {ok ? t("build.done.later") : t("build.done.close")}
            </button>
          </div>
        </div>
      </div>
    </>
  );
}
