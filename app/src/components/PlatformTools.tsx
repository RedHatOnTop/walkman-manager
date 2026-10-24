import { useEffect, useRef, useState } from "react";
import { asError, bridge } from "../api/bridge";
import type { ErrorReport, ToolsView } from "../api/types";
import { bytes, percent } from "../format";
import { usePrefs } from "../i18n";
import { LicenceSheet } from "../views/studio/LicenceSheet";
import { ErrorText } from "./ErrorText";
import { Icon } from "./Icon";

export interface Tools {
  view: ToolsView | null;
  setView(v: ToolsView): void;
  error: ErrorReport | null;
  setError(e: ErrorReport | null): void;
}

/** Google's platform-tools as the app keeps them, followed through a download. `onDone` hears of a
 * download that ended with the tools in place. */
export function usePlatformTools(onDone?: (v: ToolsView) => void): Tools {
  const [view, setView] = useState<ToolsView | null>(null);
  const [error, setError] = useState<ErrorReport | null>(null);
  const done = useRef(onDone);
  done.current = onDone;
  useEffect(() => {
    let live = true;
    const offs: Promise<() => void>[] = [];
    void bridge().then((b) => {
      void b.toolsView().then((v) => live && setView(v));
      offs.push(b.onToolsProgress((p) => live && setView((v) => v && { ...v, fetching: p })));
      offs.push(
        b.onToolsDone((d) => {
          if (!live) return;
          setView(d.view);
          setError(d.error);
          if (!d.error && !d.cancelled) done.current?.(d.view);
        }),
      );
    });
    return () => {
      live = false;
      offs.forEach((o) => void o.then((f) => f()));
    };
  }, []);
  return { view, setView, error, setError };
}

/** Downloads Google's platform-tools once the owner agrees to the licence they come under, and shows the
 * download while it runs. */
export function GetTools({ tools }: { tools: Tools }) {
  const { t, lang } = usePrefs();
  const [sheet, setSheet] = useState(false);
  const { view, setView, error, setError } = tools;
  if (!view) return null;
  const fetch = () => {
    setError(null);
    void bridge()
      .then((b) => b.toolsFetch())
      .catch((e) => setError(asError(e)));
  };
  const f = view.fetching;
  return (
    <div className="get-tools">
      {f ? (
        <div className="get-tools-run" aria-live="polite">
          <span className="get-tools-bar">
            <span style={{ width: `${percent(f.done, f.total)}%` }} />
          </span>
          <span className="get-tools-amount num">
            {bytes(f.done, lang)}
            <span> / {bytes(f.total, lang)}</span>
          </span>
          <button className="btn btn--small btn--ghost" onClick={() => void bridge().then((b) => b.toolsCancel())}>
            {t("tools.cancel")}
          </button>
        </div>
      ) : (
        <button className="btn btn--small" onClick={() => (view.agreed ? fetch() : setSheet(true))}>
          <Icon name="download" size={14} />
          {t("tools.get", { size: bytes(view.bytes, lang) })}
        </button>
      )}
      {error && (
        <p className="get-tools-error">
          <ErrorText error={error} />
        </p>
      )}
      {sheet && (
        <LicenceSheet
          what={t("tools.licence.what")}
          read={() => bridge().then((b) => b.toolsLicence())}
          agree={(sha256) =>
            bridge()
              .then((b) => b.toolsAgree(sha256))
              .then((v) => {
                setView(v);
                fetch();
              })
          }
          onClose={() => setSheet(false)}
          onError={setError}
        />
      )}
    </div>
  );
}
