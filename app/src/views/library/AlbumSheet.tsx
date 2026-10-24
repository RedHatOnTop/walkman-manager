import { useEffect } from "react";
import type { Album, Track } from "../../api/types";
import { Cover } from "../../components/Cover";
import { Icon, type IconName } from "../../components/Icon";
import { bytes } from "../../format";
import { usePrefs, useT } from "../../i18n";
import { useLibrary } from "../../state/library";
import { duration, formatLabel } from "../../state/music";
import { SensMeMarks } from "./SensMeBoard";

export function useEscape(onClose: () => void) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
}

/** Where else an album is: on a player for the computer's albums, on the computer for a player's. */
export interface AlbumPlace {
  key: string;
  short: string;
  present: number;
  total: number;
}

export interface AlbumAction {
  icon: IconName;
  label: string;
  run(): void;
}

export function AlbumSheet({
  album,
  tracks,
  places,
  marked,
  primary,
  more = [],
  onClose,
}: {
  album: Album;
  tracks: Map<string, Track>;
  places: AlbumPlace[];
  /** Tracks that are already where the primary action would put them. */
  marked(track: Track): boolean;
  primary: AlbumAction | null;
  more?: AlbumAction[];
  onClose(): void;
}) {
  const t = useT();
  const { lang } = usePrefs();
  const { selected, toggle } = useLibrary();
  useEscape(onClose);
  const list = album.tracks.map((id) => tracks.get(id)).filter((x) => x != null);
  const isSelected = selected.has(album.id);
  const discs = new Set(list.map((tr) => tr.disc ?? 1)).size > 1;
  const garbled = list.filter((tr) => tr.legacyText).length;
  const analysed = list.filter((tr) => tr.sensme).length;

  return (
    <>
      <div className="scrim scrim--deep" onClick={onClose} />
      <div className="album-modal" role="dialog" aria-modal="true" aria-label={album.title}>
        <span className="album-ambient" aria-hidden="true">
          <Cover art={album.art} title={album.title} />
        </span>
        <button className="icon-btn album-close" onClick={onClose} aria-label={t("album.close")} title={t("album.close")}>
          <Icon name="close" size={18} />
        </button>
        <div className="album-modal-scroll">
          <div className="album-top">
            <div className="album-cover">
              <Cover art={album.art} title={album.title} />
            </div>
            <div className="album-info">
              <span className="eyebrow">{album.genre ?? t("library.title")}</span>
              <h2 className="album-title selectable">{album.title}</h2>
              <p className="album-artist selectable">{album.artist ?? t(album.compilation ? "library.variousArtists" : "library.unknownArtist")}</p>
              <div className="album-facts num">
                {album.year && <span>{album.year}</span>}
                <span>{t("album.tracks", { n: album.tracks.length })}</span>
                <span>{duration(album.durationMs)}</span>
                <span>{bytes(album.bytes, lang)}</span>
                {analysed > 0 && <span>{t("album.sensme", { n: analysed })}</span>}
                {garbled > 0 && <span className="album-fact-warn">{t("album.garbled", { n: garbled })}</span>}
              </div>
              <div className="album-format">
                <span className={`qtab-swatch q-${album.quality}`} />
                <span className="mono">{album.mixedFormats ? t("album.mixed") : formatLabel(album.format)}</span>
                <span>{t(`quality.${album.quality}`)}</span>
              </div>
              <div className="album-actions">
                {primary && (
                  <button className="btn btn--primary btn--large" onClick={primary.run}>
                    <Icon name={primary.icon} size={17} />
                    {primary.label}
                  </button>
                )}
                <button className={`btn btn--large${isSelected ? " is-on" : ""}`} onClick={() => toggle(album.id)} aria-pressed={isSelected}>
                  {isSelected ? <Icon name="check" size={16} /> : <Icon name="plus" size={16} />}
                  {isSelected ? t("album.selected") : t("album.select")}
                </button>
                {more.map((a) => (
                  <button key={a.label} className="btn btn--large" onClick={a.run}>
                    <Icon name={a.icon} size={16} />
                    {a.label}
                  </button>
                ))}
              </div>
            </div>
          </div>

          {places.length > 0 && (
            <div className="album-players">
              {places.map(({ key, short, present, total }) => {
                const state = present === 0 ? "none" : present === total ? "full" : "part";
                return (
                  <span className={`album-player album-player--${state}`} key={key}>
                    <span className={`pip mono pip--${state}`}>{short}</span>
                    {present === 0 ? t("album.notOnPlayer") : present === total ? t("album.complete") : t("album.partial", { present, total })}
                  </span>
                );
              })}
            </div>
          )}

          <ol className="tracks">
            {list.map((tr) => {
              const onSome = marked(tr);
              return (
                <li key={tr.id} className={`trk${onSome ? " is-on-player" : ""}`}>
                  <span className="trk-no mono num">{discs ? `${tr.disc ?? 1}.${tr.track ?? ""}` : (tr.track ?? "")}</span>
                  <span className="trk-title">
                    <span className="selectable">{tr.title ?? tr.rel.split("/").pop()}</span>
                    {tr.artist && tr.artist !== album.artist && <span className="trk-artist">{tr.artist}</span>}
                  </span>
                  <span className="trk-side">
                    {tr.sensme && <SensMeMarks sensme={tr.sensme} />}
                    {tr.lyrics && (
                      <span title={t("album.lyrics")}>
                        <Icon name="lyrics" size={14} />
                      </span>
                    )}
                    {album.mixedFormats && <span className="mono">{formatLabel(tr.format)}</span>}
                  </span>
                  <span className="trk-time mono num">{duration(tr.durationMs)}</span>
                </li>
              );
            })}
          </ol>
        </div>
      </div>
    </>
  );
}
