import type { CSSProperties } from "react";
import type { DeviceSummary } from "../api/types";
import { Battery } from "../components/Battery";
import { Cover } from "../components/Cover";
import { Icon } from "../components/Icon";
import { GetTools, usePlatformTools } from "../components/PlatformTools";
import { DeviceRender } from "../components/render/DeviceRender";
import { bytes } from "../format";
import { usePrefs, useT } from "../i18n";
import type { Route } from "../routes";
import { useData, useReading } from "../state/data";
import { artFor, displayName, freeBytes, modeTone, outputFigure, screenFor, shortSystem } from "../state/describe";
import { useLibrary } from "../state/library";
import { streamText } from "./device/sections";

/** Pixels per millimetre on the stage: the ZX500 stands about 420 px tall. */
const STAGE_SCALE = 3.45;

function Readouts({ device }: { device: DeviceSummary }) {
  const t = useT();
  const { lang } = usePrefs();
  const { snapshot } = useReading(device.id);
  if (device.mode !== "running" || !snapshot) return null;
  const level = snapshot.power?.level;
  const charging = snapshot.power?.status === "Charging";
  const free = freeBytes(snapshot);
  const out = outputFigure(snapshot);
  return (
    <dl className="specs player-spec">
      <div className={`spec${out ? " is-live" : ""}`}>
        <dt>{t("readout.output")}</dt>
        <dd className="num">{out ? `${out.value} ${out.unit} · ${out.note}` : t("output.idle")}</dd>
      </div>
      {level != null && (
        <div className="spec">
          <dt>{charging ? t("readout.charging") : t("readout.battery")}</dt>
          <dd className="num">
            <Battery level={level} charging={charging} />
            {level} %
          </dd>
        </div>
      )}
      {free != null && (
        <div className="spec">
          <dt>{t("readout.free")}</dt>
          <dd className="num">{bytes(free, lang)}</dd>
        </div>
      )}
    </dl>
  );
}

function Player({ device, open, solo }: { device: DeviceSummary; open(): void; solo: boolean }) {
  const t = useT();
  const { lang } = usePrefs();
  const { snapshot } = useReading(device.id);
  const art = artFor(device.profileId ?? snapshot?.identity.profileId);
  const tone = modeTone(device.mode);
  const running = device.mode === "running";
  const stream = running ? streamText(snapshot, t) : null;
  return (
    <button className={`player${solo ? " is-solo" : ""}`} onClick={open}>
      <div className="player-stand">
        {art ? (
          <DeviceRender
            art={art}
            scale={STAGE_SCALE}
            screen={screenFor(device.mode, snapshot)}
            battery={snapshot?.power?.level}
            charging={running && snapshot?.power?.status === "Charging"}
            live={!!stream}
            lang={lang}
          />
        ) : (
          <Icon name="player" size={64} />
        )}
      </div>
      <div className="player-caption">
        {device.family && <span className="eyebrow">{device.family}</span>}
        <span className="player-name">{displayName(device, snapshot, t("device.unknownModel"))}</span>
        <span className={`player-state player-state--${tone}`}>
          <span className="player-mode">{t(`mode.${device.mode}`)}</span>
          {snapshot && running && <span className="player-os">{shortSystem(snapshot)}</span>}
        </span>
        <Readouts device={device} />
      </div>
    </button>
  );
}

function Shelf({ go }: { go(route: Route): void }) {
  const t = useT();
  const { state } = useLibrary();
  const albums = [...(state?.index.albums ?? [])].sort((a, b) => b.modified - a.modified).slice(0, 8);
  return (
    <section className="shelf">
      <header className="shelf-head">
        <div>
          <h2 className="shelf-title">{t("home.shelf")}</h2>
          {state && state.index.albums.length > 0 && (
            <span className="shelf-count num">
              {t("home.shelfCount", { albums: state.index.albums.length.toLocaleString(), tracks: state.index.tracks.length.toLocaleString() })}
            </span>
          )}
        </div>
        <button className="shelf-open" onClick={() => go({ kind: "library" })}>
          {t("home.shelfOpen")}
        </button>
      </header>
      {albums.length === 0 ? (
        <p className="shelf-empty">{t("home.shelfEmpty")}</p>
      ) : (
        <div className="shelf-row">
          {albums.map((a, i) => (
            <button key={a.id} className="shelf-item" style={{ "--i": i } as CSSProperties} onClick={() => go({ kind: "library" })} title={a.title}>
              <span className="shelf-cover">
                <Cover art={a.art} title={a.title} />
              </span>
              <span className="shelf-name">{a.title}</span>
              <span className="shelf-artist">{a.artist ?? t(a.compilation ? "library.variousArtists" : "library.unknownArtist")}</span>
            </button>
          ))}
        </div>
      )}
    </section>
  );
}

function Empty() {
  const t = useT();
  const { lang } = usePrefs();
  return (
    <div className="stage stage--empty">
      <div className="stage-ghosts" aria-hidden="true">
        <DeviceRender art="a50" scale={2.6} screen={{ kind: "off" }} lang={lang} />
        <DeviceRender art="zx500" scale={2.6} screen={{ kind: "off" }} lang={lang} />
      </div>
      <div className="stage-empty-text">
        <h2>{t("home.empty.title")}</h2>
        <p>{t("home.empty.body")}</p>
        <ul className="empty-hints">
          <li>
            <Icon name="player" size={16} />
            {t("home.empty.hintA50")}
          </li>
          <li>
            <Icon name="player" size={16} />
            {t("home.empty.hintZx")}
          </li>
          <li>
            <Icon name="chip" size={16} />
            {t("home.empty.hintRescue")}
          </li>
        </ul>
      </div>
    </div>
  );
}

export function Home({ go }: { go(route: Route): void }) {
  const t = useT();
  const { scan } = useData();
  const tools = usePlatformTools();
  const n = scan.devices.length;
  return (
    <div className="page home">
      <header className="page-head">
        <div className="page-titles">
          <h1 className="page-title">{n === 0 ? t("home.title") : n === 1 ? t("home.subtitle.one") : t("home.subtitle.many", { n })}</h1>
        </div>
      </header>

      {tools.view && !tools.view.adb && (
        <div className="notice notice--warn">
          <Icon name="alert" size={20} />
          <div className="notice-text">
            <h3>{t("home.noAdb.title")}</h3>
            <p>{t("home.noAdb.body")}</p>
            <GetTools tools={tools} />
          </div>
        </div>
      )}

      {scan.issues.length > 0 && (
        <div className="notice notice--warn">
          <Icon name="alert" size={20} />
          <div className="notice-text">
            <h3>{t("home.issues")}</h3>
            {scan.issues.map((i) => (
              <p key={i} className="mono selectable">
                {i}
              </p>
            ))}
          </div>
        </div>
      )}

      {n === 0 ? (
        <Empty />
      ) : (
        <div className={`stage stage--${Math.min(n, 3)}`}>
          {scan.devices.map((d) => (
            <Player key={d.id} device={d} solo={n === 1} open={() => go({ kind: "device", id: d.id })} />
          ))}
        </div>
      )}

      <Shelf go={go} />
    </div>
  );
}
