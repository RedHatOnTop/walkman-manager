import { useCallback, useEffect, useState } from "react";
import { asError, bridge } from "../../api/bridge";
import type { ErrorReport, FormatKind, Kit, KitInput, KnownFile, StudioDone, StudioProgress, StudioView, VaultFile } from "../../api/types";
import type { Route } from "../../routes";
import { BuildDock, BuildRun, Builds } from "./BuildDock";
import { Icon } from "../../components/Icon";
import { DeviceRender } from "../../components/render/DeviceRender";
import { bytes, percent } from "../../format";
import { usePrefs, type StringKey } from "../../i18n";
import { useData } from "../../state/data";
import { artFor } from "../../state/describe";
import { ErrorText } from "../../components/ErrorText";

const FORMAT: Record<FormatKind, string> = {
  sonyNwwm: "UPG",
  sonyUpg: "UPG",
  otaPayload: "OTA",
  otaBlocks: "OTA",
  ext4: "IMG",
  sparse: "IMG",
  xz: "XZ",
  gzip: "TGZ",
  bootImage: "BOOT",
  vbMeta: "AVB",
  kernelImage: "KRNL",
  zip: "ZIP",
  wmBackup: "BAK",
  unknown: "?",
};

function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

function host(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

/** The file's format as the short word a spec sheet prints beside it. */
function FormatWord({ kind, state }: { kind: FormatKind; state: "empty" | "filled" | "bad" }) {
  return (
    <span className={`fmt fmt--${state} mono`} aria-hidden="true">
      {FORMAT[kind]}
    </span>
  );
}

/** What fills an input that takes the owner's own file by its structure, such as a kernel. */
function formatTitle(input: KitInput, t: ReturnType<typeof usePrefs>["t"]): string {
  const f = input.formats[0];
  if (f?.kind === "wmBackup") return t("studio.format.wmBackup.bay");
  return f ? t(`studio.format.${f.kind}` as StringKey, { release: f.release ?? "" }) : input.id;
}

function Bay({ input, file, busy, onFetch, onOpen, onOpenInput }: { input: KitInput; file: VaultFile | null; busy: boolean; onFetch(id: string): void; onOpen(id: string): void; onOpenInput(input: string): void }) {
  const { t, lang } = usePrefs();
  const first = input.accepts[0];
  const fetchable = input.accepts.find((a) => a.url);
  const guided = input.accepts.find((a) => a.guide);
  const filledBy = file ? input.accepts.find((a) => a.id === file.known) : null;
  const kind = filledBy?.format ?? first?.format ?? input.formats[0]?.kind ?? "unknown";
  return (
    <div className={`bay${file ? " is-filled" : ""}${input.optional ? " is-optional" : ""}`}>
      <FormatWord kind={kind} state={file ? "filled" : "empty"} />
      <div className="bay-text">
        <span className="bay-role">
          {t(`studio.input.${input.id}` as StringKey)}
          {input.optional && <span className="tag">{t("studio.optional")}</span>}
        </span>
        <span className="bay-file">{(filledBy ?? first)?.title[lang] ?? formatTitle(input, t)}</span>
        {file ? (
          <span className="bay-where mono selectable" title={file.path}>
            {baseName(file.path)}
          </span>
        ) : (
          <span className="bay-why">{t(`studio.why.${input.id}` as StringKey)}</span>
        )}
      </div>
      <div className="bay-act">
        {file ? (
          <span className="bay-ok num">
            <Icon name="check" size={15} />
            {bytes(file.bytes, lang)}
          </span>
        ) : fetchable ? (
          <button className="btn btn--small" onClick={() => onFetch(fetchable.id)} disabled={busy}>
            <Icon name="download" size={15} />
            {t("studio.fetch")}
            <span className="bay-host mono">{host(fetchable.url!)}</span>
          </button>
        ) : guided ? (
          <button className="btn btn--small btn--ghost" onClick={() => onOpen(guided.id)}>
            <Icon name="external" size={15} />
            {t("studio.howToGet")}
          </button>
        ) : input.guided ? (
          <button className="btn btn--small btn--ghost" onClick={() => onOpenInput(input.id)}>
            <Icon name="external" size={15} />
            {t("studio.howToMake")}
          </button>
        ) : (
          <span className="bay-own">{t("studio.bringIt")}</span>
        )}
      </div>
    </div>
  );
}

function KitBench({
  kit,
  view,
  busy,
  go,
  onFetch,
  onOpen,
  onView,
  onError,
}: {
  kit: Kit;
  view: StudioView;
  busy: boolean;
  go(r: Route): void;
  onFetch(id: string): void;
  onOpen(id: string): void;
  onView(v: StudioView): void;
  onError(e: ErrorReport): void;
}) {
  const { t, lang } = usePrefs();
  const files = view.files;
  const openInput = (input: string) => void bridge().then((b) => b.studioOpenInputGuide(kit.id, input, lang));
  const { profiles } = useData();
  const profile = profiles.find((p) => p.id === kit.profile);
  const art = artFor(kit.profile);
  const required = kit.inputs.filter((i) => !i.optional);
  const have = required.filter((i) => i.filledBy).length;
  return (
    <article className={`kit${kit.ready ? " is-ready" : ""}`}>
      <div className="kit-object">
        {art && <DeviceRender art={art} scale={2.3} screen={{ kind: "off" }} />}
      </div>
      <div className="kit-body">
        <div className="kit-head">
          <div className="kit-titles">
            <span className="eyebrow">{profile?.family ?? kit.profile}</span>
            <h2 className="kit-name">{t(`studio.kit.${kit.id}.name` as StringKey)}</h2>
            <p className="kit-what">{t(`studio.kit.${kit.id}.what` as StringKey)}</p>
          </div>
          <span className="kit-count num">{kit.ready ? t("studio.allHere") : t("studio.haveOf", { have, of: required.length })}</span>
        </div>
        <div className="kit-bays">
          {kit.inputs.map((i) => (
            <Bay key={i.id} input={i} file={files.find((f) => f.id === i.filledBy) ?? null} busy={busy} onFetch={onFetch} onOpen={onOpen} onOpenInput={openInput} />
          ))}
        </div>
        {kit.buildable && view.building?.kit === kit.id && <BuildRun progress={view.building} />}
        {kit.buildable && !view.building && <BuildDock kit={kit} view={view} go={go} onView={onView} onError={onError} />}
      </div>
    </article>
  );
}

function fileNote(f: VaultFile, catalog: KnownFile[], kits: Kit[], t: ReturnType<typeof usePrefs>["t"], lang: ReturnType<typeof usePrefs>["lang"]): { tone: "ok" | "plain" | "warn"; text: string } {
  if (f.presence === "gone") return { tone: "warn", text: t("studio.file.gone") };
  if (f.presence === "changed") return { tone: "warn", text: t("studio.file.changed") };
  if (f.damage?.kind === "truncated") {
    return { tone: "warn", text: t("studio.file.truncated", { have: bytes(f.damage.actual, lang), of: bytes(f.damage.expected, lang) }) };
  }
  if (f.damage?.kind === "changed") {
    const id = f.damage.known;
    return { tone: "warn", text: t("studio.file.differs", { title: catalog.find((c) => c.id === id)?.title[lang] ?? id }) };
  }
  if (f.damage?.kind === "unlike") return { tone: "warn", text: t("studio.file.unlike", { file: f.damage.file }) };
  const takes = (i: KitInput) =>
    i.accepts.some((a) => a.id === f.known) ||
    i.formats.some((p) => p.kind === f.format.kind && (!p.release || ("release" in f.format && f.format.release === p.release)));
  if (!f.known && !kits.some((k) => k.inputs.some(takes))) return { tone: "plain", text: t("studio.file.unknown") };
  const users = kits.filter((k) => k.inputs.some(takes));
  if (users.length === 0) return { tone: "plain", text: t("studio.file.unused") };
  return { tone: "ok", text: users.map((k) => t(`studio.kit.${k.id}.name` as StringKey)).join(" · ") };
}

function VaultTile({ f, view, busy, onRemove, onLook }: { f: VaultFile; view: StudioView; busy: boolean; onRemove(id: string): void; onLook(path: string): void }) {
  const { t, lang } = usePrefs();
  const known = view.catalog.find((c) => c.id === f.known);
  const note = fileNote(f, view.catalog, view.kits, t, lang);
  const bad = note.tone === "warn";
  return (
    <div className={`vfile${bad ? " is-bad" : ""}`}>
      <FormatWord kind={f.format.kind} state={bad ? "bad" : known || note.tone === "ok" ? "filled" : "empty"} />
      <div className="vfile-text">
        <span className="vfile-title">
          {known?.title[lang] ??
            (f.format.kind === "kernelImage"
              ? t("studio.format.kernelImage", { release: f.format.release ?? "?" })
              : f.format.kind === "wmBackup"
                ? t("studio.format.wmBackup", { n: f.format.images })
                : baseName(f.path))}
        </span>
        <span className="vfile-path mono selectable" title={f.path}>
          {f.path}
        </span>
        <span className={`vfile-note vfile-note--${note.tone}`}>
          {note.tone === "ok" && <Icon name="check" size={13} />}
          {note.tone === "warn" && <Icon name="alert" size={13} />}
          {note.text}
        </span>
      </div>
      <div className="vfile-side">
        <span className="vfile-size num">{bytes(f.bytes, lang)}</span>
        <span className="vfile-hash mono selectable" title={`SHA-256 ${f.sha256}`}>
          {f.sha256.slice(0, 12)}
        </span>
      </div>
      <div className="vfile-act">
        {f.presence === "changed" && (
          <button className="btn btn--small" onClick={() => onLook(f.path)} disabled={busy}>
            {t("studio.lookAgain")}
          </button>
        )}
        <button className="icon-btn icon-btn--small" onClick={() => onRemove(f.id)} title={t("studio.forget")} aria-label={t("studio.forget")}>
          <Icon name="close" size={15} />
        </button>
      </div>
    </div>
  );
}

/** `title` names the known file a download is for. */
function Working({ p, title, onStop }: { p: StudioProgress; title: string | undefined; onStop(): void }) {
  const { t, lang } = usePrefs();
  const pct = p.total > 0 ? percent(p.done, p.total) : 0;
  return (
    <div className="studio-work" aria-live="polite">
      <Icon name={p.fetching ? "download" : "firmware"} size={20} />
      <div className="studio-work-text">
        <span className="studio-work-title">
          {p.fetching ? t("studio.fetching") : t("studio.looking")}
          {p.count > 1 && <span className="num"> {t("studio.nOfM", { n: p.index + 1, m: p.count })}</span>}
        </span>
        <span className={`studio-work-name${p.fetching ? "" : " mono"}`}>{p.fetching ? (title ?? p.name) : baseName(p.name)}</span>
        <span className="studio-work-bar">
          <span style={{ width: `${pct}%` }} />
        </span>
      </div>
      <span className="studio-work-amount num">
        {bytes(p.done, lang)}
        {p.total > 0 && <span> / {bytes(p.total, lang)}</span>}
      </span>
      <button className="btn btn--ghost" onClick={onStop}>
        {t("studio.stop")}
      </button>
    </div>
  );
}

export function StudioPage({ go }: { go(r: Route): void }) {
  const { t, lang } = usePrefs();
  const [view, setView] = useState<StudioView | null>(null);
  const [progress, setProgress] = useState<StudioProgress | null>(null);
  const [busy, setBusy] = useState(false);
  const [last, setLast] = useState<StudioDone | null>(null);
  const [error, setError] = useState<ErrorReport | null>(null);
  const [over, setOver] = useState(false);

  const reload = useCallback(() => {
    bridge()
      .then((b) => b.studioView())
      .then(setView, (e) => setError(asError(e)));
  }, []);

  const begin = useCallback((run: () => Promise<unknown>) => {
    setError(null);
    setLast(null);
    setBusy(true);
    run().catch((e) => {
      setBusy(false);
      setError(asError(e));
    });
  }, []);

  const add = useCallback((paths: string[]) => begin(() => bridge().then((b) => b.studioAdd(paths))), [begin]);

  useEffect(() => {
    reload();
    const offs: Promise<() => void>[] = [];
    void bridge().then((b) => {
      offs.push(
        b.onStudioProgress((p) => {
          setBusy(true);
          setProgress(p);
        }),
        b.onStudioChanged(reload),
        b.onStudioDone((d) => {
          setBusy(false);
          setProgress(null);
          setLast(d);
          reload();
        }),
        b.onStudioBuild((p) => setView((v) => (v ? { ...v, building: p } : v))),
        b.onStudioBuildDone((d) => {
          if (d.error) setError(d.error);
          reload();
        }),
        b.onFileDrop((e) => {
          setOver(e.over);
          if (!e.over && e.paths.length > 0) add(e.paths);
        }),
      );
    });
    return () => offs.forEach((o) => void o.then((f) => f()));
  }, [reload, add]);

  const pick = () =>
    begin(() =>
      bridge()
        .then((b) => b.studioPick())
        .then((picked) => {
          if (!picked) setBusy(false);
        }),
    );
  const fetchFile = (id: string) => begin(() => bridge().then((b) => b.studioFetch(id)));
  const open = (id: string) => void bridge().then((b) => b.studioOpenGuide(id, lang));
  const stop = () => void bridge().then((b) => b.studioCancel());
  const remove = (id: string) =>
    void bridge()
      .then((b) => b.studioRemove(id))
      .then(setView, (e) => setError(asError(e)));

  const titleOf = (id: string) => view?.catalog.find((c) => c.id === id)?.title[lang];
  const files = view?.files ?? [];
  const kits = view?.kits ?? [];

  return (
    <div className="page studio">
      <header className="studio-hero">
        <div className="page-titles">
          <h1 className="page-title">{t("studio.title")}</h1>
          <p className="studio-lead">{t("studio.lead")}</p>
        </div>
          <button className={kits.some((k) => k.buildable && k.ready) ? "btn" : "btn btn--primary"} onClick={pick} disabled={busy}>
            <Icon name="plus" size={16} />
            {t("studio.add")}
          </button>
      </header>

      {progress && <Working p={progress} title={titleOf(progress.name)} onStop={stop} />}
      {error && (
        <div className="notice notice--warn studio-notice">
          <Icon name="alert" />
          <p className="selectable">
            <ErrorText error={error} />
          </p>
        </div>
      )}
      {last && (last.failed.length > 0 || last.cancelled) && (
        <div className="notice notice--warn studio-notice">
          <Icon name="alert" />
          <div>
            {last.cancelled && <p>{t("studio.stopped")}</p>}
            {last.failed.map((f) => (
              <p key={f.name} className="selectable">
                {f.file ? (titleOf(f.file) ?? f.file) : <span className="mono">{baseName(f.name)}</span>} — <ErrorText error={f.error} />
              </p>
            ))}
          </div>
        </div>
      )}

      <section className="studio-kits" aria-label={t("studio.kits")}>
        {view &&
          kits.map((k) => (
            <KitBench key={k.id} kit={k} view={view} busy={busy} go={go} onFetch={fetchFile} onOpen={open} onView={setView} onError={setError} />
          ))}
      </section>

      {view && view.builds.length > 0 && <Builds view={view} go={go} onView={setView} onError={setError} />}

      <section className="studio-vault">
        <div className="studio-section-head">
          <h2 className="studio-section-title">{t("studio.vault")}</h2>
          <p className="studio-section-note">{t("studio.vaultNote")}</p>
        </div>
        {files.length === 0 ? (
          <button className="studio-empty" onClick={pick} disabled={busy}>
            <Icon name="firmware" size={28} />
            <span className="studio-empty-title">{t("studio.empty")}</span>
            <span className="studio-empty-note">{t("studio.emptyNote")}</span>
          </button>
        ) : (
          <div className="vault-list">
            {[...files]
              .sort((a, b) => b.added - a.added)
              .map((f) => (
                <VaultTile key={f.id} f={f} view={view!} busy={busy} onRemove={remove} onLook={(p) => add([p])} />
              ))}
          </div>
        )}
        {view && (
          <p className="studio-foot">
            {t("studio.downloads")} <span className="mono selectable">{view.downloads}</span>
          </p>
        )}
      </section>

      {over && (
        <div className="studio-drop" aria-hidden="true">
          <div className="studio-drop-frame">
            <Icon name="firmware" size={40} />
            <span>{t("studio.drop")}</span>
          </div>
        </div>
      )}
    </div>
  );
}
