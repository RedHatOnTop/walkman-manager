import { useEffect, useMemo } from "react";
import type { DeviceSummary, VolumeMusic } from "../../api/types";
import { Icon } from "../../components/Icon";
import { bytes } from "../../format";
import { usePrefs, useT } from "../../i18n";
import { useLibrary } from "../../state/library";
import { destPath } from "../../state/music";
import { Section } from "./sections";
import { ErrorText } from "../../components/ErrorText";

const GROUPS: [string, string[]][] = [
  ["FLAC", ["flac"]],
  ["DSD", ["dsf", "dff"]],
  ["M4A", ["m4a", "mp4", "aac"]],
  ["MP3", ["mp3"]],
  ["WAV", ["wav"]],
  ["AIFF", ["aif", "aiff", "aifc"]],
  ["APE", ["ape"]],
];
const SHADES = ["var(--text-1)", "var(--text-2)", "var(--text-3)", "var(--line-3)", "var(--line-2)"];

function group(rel: string): string {
  const ext = rel.slice(rel.lastIndexOf(".") + 1).toLowerCase();
  return GROUPS.find(([, exts]) => exts.includes(ext))?.[0] ?? "OTHER";
}

function VolumeRow({ v, library }: { v: VolumeMusic; library: Set<string> }) {
  const t = useT();
  const { lang } = usePrefs();
  const audio = v.files.filter((f) => !f.rel.toLowerCase().endsWith(".lrc"));
  const lyrics = v.files.length - audio.length;
  const size = audio.reduce((n, f) => n + f.size, 0);
  const matched = audio.filter((f) => library.has(`${f.rel.toLowerCase()}\0${f.size}`)).length;
  const counts = new Map<string, number>();
  for (const f of audio) counts.set(group(f.rel), (counts.get(group(f.rel)) ?? 0) + 1);
  const mix = [...counts.entries()].sort((a, b) => b[1] - a[1]);
  const shade = (name: string, i: number) => (name === "DSD" ? undefined : SHADES[Math.min(i, SHADES.length - 1)]);

  return (
    <div className="music-vol">
      <div className="music-vol-head">
        <span className="vol-name">
          {t(v.kind === "card" ? "storage.card" : "storage.internal")}
        </span>
        <span className="music-vol-root mono">{v.root}</span>
      </div>
      {audio.length > 0 ? (
        <div className="music-vol-count num">
          <span className="music-vol-n">{audio.length.toLocaleString()}</span>
          <span className="music-vol-unit">{t("music.trackUnit")}</span>
          <span className="music-vol-size">{bytes(size, lang)}</span>
        </div>
      ) : (
        <p className="sec-note">{v.exists ? t("music.empty") : t("music.missing")}</p>
      )}
      {mix.length > 0 && (
        <>
          <span className="music-mix" aria-hidden="true">
            {mix.map(([name, n], i) => (
              <span key={name} className={name === "DSD" ? "q-dsd" : undefined} style={{ flexGrow: n, background: shade(name, i) }} />
            ))}
          </span>
          <span className="music-legend num">
            {mix.map(([name, n], i) => (
              <span key={name}>
                <i className={name === "DSD" ? "q-dsd" : undefined} style={{ background: shade(name, i) }} />
                <span className="mono">{name}</span> {n.toLocaleString()}
              </span>
            ))}
            {lyrics > 0 && <span>{t("music.lyrics", { n: lyrics })}</span>}
            {matched > 0 && <span className="music-matched">{t("music.inLibrary", { n: matched.toLocaleString() })}</span>}
          </span>
        </>
      )}
    </div>
  );
}

export function MusicSection({ device, browse }: { device: DeviceSummary; browse(): void }) {
  const t = useT();
  const { music, loadMusic, state } = useLibrary();
  const m = music[device.id];

  useEffect(() => {
    if (!m) loadMusic(device.id);
  }, [device.id, m, loadMusic]);

  const library = useMemo(
    () => new Set((state?.index.tracks ?? []).map((tr) => `${destPath(tr.rel).toLowerCase()}\0${tr.size}`)),
    [state],
  );

  return (
    <Section
      kind="music"
      title={t("section.music")}
      aside={
        <>
          <button className="btn btn--small" onClick={browse}>
            <Icon name="library" size={14} />
            {t("music.browse")}
          </button>
          <button className={`icon-btn icon-btn--small${m?.loading ? " is-spinning" : ""}`} onClick={() => loadMusic(device.id)} aria-label={t("device.refresh")} title={t("device.refresh")}>
            <Icon name="refresh" size={15} />
          </button>
        </>
      }
    >
      {m?.error && (
        <p className="sec-note selectable">
          <ErrorText error={m.error} />
        </p>
      )}
      {!m?.volumes && m?.loading && (
        <div className="plan-reading">
          <span className="spinner" />
          {t("music.reading")}
        </div>
      )}
      {m?.volumes?.length === 0 && <p className="sec-note">{t(device.links.includes("mass-storage") ? "storage.exported" : "storage.none")}</p>}
      {m?.volumes && (
        <div className="music-vols">
          {m.volumes.map((v) => (
            <VolumeRow key={v.root} v={v} library={library} />
          ))}
        </div>
      )}
    </Section>
  );
}
