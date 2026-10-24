import { useEffect, useRef, useState } from "react";
import { asError } from "../../api/bridge";
import type { ErrorReport, LicenceText } from "../../api/types";
import { Icon } from "../../components/Icon";
import { usePrefs } from "../../i18n";

/** A paragraph of the licence: a numbered section's title, a numbered clause, or prose. */
function Paragraph({ text }: { text: string }) {
  const heading = /^(\d+)\.\s+(.{1,80})$/.exec(text);
  if (heading) {
    return (
      <h3 className="licence-section">
        <span className="num">{heading[1]}</span>
        {heading[2]}
      </h3>
    );
  }
  const clause = /^(\d+\.\d+)\s+([\s\S]+)$/.exec(text);
  if (clause) {
    return (
      <p className="licence-clause">
        <span className="licence-no num">{clause[1]}</span>
        <span>{clause[2]}</span>
      </p>
    );
  }
  return <p>{text}</p>;
}

/** The licence a download comes under, in its publisher's own words, and the owner's agreement. `what`
 * says what is downloaded once they agree; `agree` keeps the agreement to the text `read` returned. */
export function LicenceSheet({
  what,
  read: load,
  agree: keep,
  onClose,
  onError,
}: {
  what: string;
  read(): Promise<LicenceText>;
  agree(sha256: string): Promise<void>;
  onClose(): void;
  onError(e: ErrorReport): void;
}) {
  const { t } = usePrefs();
  const [text, setText] = useState<LicenceText | null>(null);
  const [read, setRead] = useState(0);
  const [agreeing, setAgreeing] = useState(false);
  const body = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let live = true;
    void load().then(
      (l) => live && setText(l),
      (e) => {
        if (!live) return;
        onError(asError(e));
        onClose();
      },
    );
    return () => {
      live = false;
    };
    // The text is read once per opening of the sheet.
  }, []);

  useEffect(() => {
    const key = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [onClose]);

  const onScroll = () => {
    const el = body.current;
    if (!el) return;
    const span = el.scrollHeight - el.clientHeight;
    setRead(span > 0 ? Math.min(1, el.scrollTop / span) : 1);
  };

  const agree = () => {
    if (!text) return;
    setAgreeing(true);
    void keep(text.sha256)
      .then(onClose, (e) => onError(asError(e)))
      .finally(() => setAgreeing(false));
  };

  const [title, ...rest] = text
    ? text.text
        .split(/\n\s*\n/)
        .map((p) => p.trim())
        .filter(Boolean)
    : [];

  return (
    <>
      <div className="scrim" onClick={onClose} />
      <aside className="sheet licence" role="dialog" aria-modal="true" aria-label={t("licence.title")}>
        <header className="sheet-head licence-head">
          <div>
            <h2>{t("licence.title")}</h2>
            <p className="licence-what">{what}</p>
          </div>
          <button className="icon-btn icon-btn--small" onClick={onClose} aria-label={t("licence.cancel")}>
            <Icon name="close" size={16} />
          </button>
          <span className="licence-read" style={{ transform: `scaleX(${read})` }} aria-hidden="true" />
        </header>
        <div className="sheet-body licence-body" ref={body} onScroll={onScroll}>
          {text ? (
            <article className="licence-text selectable" lang="en">
              <p className="licence-kicker">{title}</p>
              {rest.map((p, i) => (
                <Paragraph key={i} text={p.replace(/\s*\n\s*/g, " ")} />
              ))}
            </article>
          ) : (
            <p className="licence-loading">{t("licence.loading")}</p>
          )}
        </div>
        <footer className="sheet-foot">
          <span className="sheet-foot-note">{t("licence.note")}</span>
          <button className="btn btn--ghost" onClick={onClose}>
            {t("licence.cancel")}
          </button>
          <button className="btn btn--primary" onClick={agree} disabled={!text || agreeing}>
            <Icon name="check" size={15} />
            {t("licence.agree")}
          </button>
        </footer>
      </aside>
    </>
  );
}
