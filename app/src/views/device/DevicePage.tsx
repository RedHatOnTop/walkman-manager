import { useEffect, useState } from "react";
import type { DeviceSummary, KnownDevice, Mode, Snapshot } from "../../api/types";
import { DeviceDrawing, type Callout } from "../../components/drawing/DeviceDrawing";
import { Icon } from "../../components/Icon";
import { DeviceRender } from "../../components/render/DeviceRender";
import { Battery } from "../../components/Battery";
import { bytes, usbId } from "../../format";
import { usePrefs, useT } from "../../i18n";
import type { StringKey } from "../../i18n/strings";
import { useData, useProfile, useReading, useReadings } from "../../state/data";
import { artFor, displayName, freeBytes, isPlaying, modeTone, outputFigure, screenFor, shortSystem } from "../../state/describe";
import type { Route } from "../../routes";
import { MusicSection } from "./MusicSection";
import { ConnectionSection, HardwareSection, label, OutputSection, PowerSection, StorageSection, SystemSection, streamText } from "./sections";
import { ErrorText } from "../../components/ErrorText";

/** Pixels per millimetre for the hero render: a little larger than on the stage. */
const HERO_SCALE = 3.3;

function useCallouts(device: DeviceSummary | null, snapshot: Snapshot | null): Callout[] {
  const t = useT();
  const { lang } = usePrefs();
  if (!device) return [];
  if (!snapshot) {
    const first = device.links[0];
    return first ? [{ key: "usb", label: t("callout.usb"), value: t(`link.${first}`), tone: modeTone(device.mode) === "rescue" ? "warn" : "plain" }] : [];
  }
  const out: Callout[] = [];
  const playing = streamText(snapshot, t);
  out.push({ key: "output", label: t("callout.output"), value: playing ?? t("output.idle"), tone: playing ? "live" : "muted" });
  const card = snapshot.volumes.find((v) => v.kind === "card");
  out.push({
    key: "card",
    label: t("callout.card"),
    value: card ? t("storage.free", { v: bytes(card.freeBytes, lang) }) : t("storage.noCardShort"),
    tone: card ? "plain" : "muted",
  });
  const p = snapshot.power;
  if (p?.level != null) {
    const status = p.status ? label(t, `power.${p.status}`, p.status) : "";
    out.push({ key: "battery", label: t("callout.battery"), value: `${p.level}%  ${status}`, tone: p.level < 15 ? "warn" : "plain" });
  }
  out.push({
    key: "system",
    label: t("callout.screen"),
    value: shortSystem(snapshot),
  });
  const link = device.links.find((l) => l === "adb" || l === "adb-network") ?? device.links[0];
  if (link) {
    out.push({ key: "usb", label: t("callout.usb"), value: `${t(`link.${link}`)}${device.usb?.speed ? ` · ${device.usb.speed}` : ""}` });
  }
  return out;
}

function Ago({ at }: { at: number }) {
  const t = useT();
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);
  const sec = Math.max(0, Math.round((now - at) / 1000));
  return <>{sec < 2 ? t("unit.ago.now") : t("unit.ago.sec", { n: sec })}</>;
}

function ModeNotice({ mode, device }: { mode: Mode | "disconnected"; device: DeviceSummary | null }) {
  const t = useT();
  if (mode === "running") return null;
  const tone = modeTone(mode);
  const icon = tone === "rescue" ? "chip" : tone === "warn" ? "alert" : "plug";
  return (
    <div className={`notice notice--${tone}`}>
      <Icon name={icon} size={20} />
      <div className="notice-text">
        <h3>{t(`notice.${mode}.title` as StringKey)}</h3>
        <p>{t(`notice.${mode}.body` as StringKey)}</p>
        {device?.usb && (
          <p className="notice-meta mono">
            {device.links.map((l) => t(`link.${l}`)).join(" · ")} · USB {usbId(device.usb.vid, device.usb.pid)} · {device.usb.port}
          </p>
        )}
      </div>
    </div>
  );
}

/** The readings that matter first, as rows of a spec sheet. */
function HeroSpec({ snapshot }: { snapshot: Snapshot }) {
  const t = useT();
  const { lang } = usePrefs();
  const level = snapshot.power?.level;
  const charging = snapshot.power?.status === "Charging";
  const free = freeBytes(snapshot);
  const out = outputFigure(snapshot);
  return (
    <dl className="specs hero-spec">
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
      <div className="spec">
        <dt>{t("readout.system")}</dt>
        <dd>{shortSystem(snapshot)}</dd>
      </div>
    </dl>
  );
}

export function DevicePage({ id, go }: { id: string; go(route: Route): void }) {
  const t = useT();
  const { lang } = usePrefs();
  const { scan, known } = useData();
  const { setFocus, refresh } = useReadings();
  const [view, setView] = useState<"object" | "drawing">("object");
  const device = scan.devices.find((d) => d.id === id) ?? null;
  const remembered: KnownDevice | null = device ? null : (known.find((k) => k.serial === id) ?? null);
  const { snapshot, error, loading } = useReading(device?.mode === "running" ? id : null);
  const profile = useProfile(device?.profileId ?? remembered?.profileId ?? snapshot?.identity.profileId);
  const callouts = useCallouts(device, device?.mode === "running" ? snapshot : null);

  useEffect(() => {
    setFocus(id);
    return () => setFocus(null);
  }, [id, setFocus]);

  const mode: Mode | "disconnected" = device?.mode ?? "disconnected";
  const tone = modeTone(mode);
  const name = device ? displayName(device, snapshot, t("device.unknownModel")) : (remembered?.model ?? remembered?.family ?? t("device.unknownModel"));
  const family = device?.family ?? remembered?.family ?? snapshot?.identity.family;
  const art = artFor(profile?.id);
  const running = mode === "running";
  const showSnapshot = running && snapshot;
  const live = running && isPlaying(snapshot);
  const charging = running && snapshot?.power?.status === "Charging";

  return (
    <div className="page device-page">
      <section className={`hero hero--${tone}${view === "drawing" ? " is-drawing" : ""}`}>
        <div className="hero-visual">
          {art && view === "object" && (
            <div className="hero-object">
              <DeviceRender
                art={art}
                scale={HERO_SCALE}
                screen={screenFor(mode, snapshot)}
                battery={snapshot?.power?.level}
                charging={charging}
                live={live}
                lang={lang}
              />
            </div>
          )}
          {art && view === "drawing" && (
            <div className="hero-drawing">
              <DeviceDrawing art={art} scale={3.05} callouts={callouts} awake={running} playing={live} charging={charging} dimension />
            </div>
          )}
          {!art && (
            <div className="hero-object hero-object--unknown">
              <Icon name="player" size={72} />
            </div>
          )}
          {art && (
            <div className="segmented segmented--small" role="tablist" aria-label={t("device.view")}>
              {(["object", "drawing"] as const).map((v) => (
                <button key={v} role="tab" aria-selected={view === v} className={view === v ? "is-on" : undefined} onClick={() => setView(v)}>
                  {t(`device.view.${v}`)}
                </button>
              ))}
            </div>
          )}
        </div>

        <div className="hero-info">
          {family && <span className="eyebrow">{family}</span>}
          <h1 className="hero-name">{name}</h1>
          <div className={`hero-state hero-state--${tone}`}>
            <span className="hero-mode">{t(`mode.${mode}`)}</span>
            {showSnapshot && snapshot.identity.modelBasis === "storage" && (
              <span className="hero-note" title={t("device.modelByStorage")}>
                <Icon name="info" size={14} />
                {t("device.modelByStorage")}
              </span>
            )}
          </div>

          {showSnapshot && <HeroSpec snapshot={snapshot} />}
          {running && !snapshot && !error && <div className="hero-spec hero-spec--loading" aria-busy="true" />}
          <ModeNotice mode={mode} device={device} />

          {running && (
            <div className="hero-actions">
              <button className="btn btn--primary btn--large" onClick={() => go({ kind: "library" })}>
                <Icon name="send" size={17} />
                {t("device.sendMusic")}
              </button>
              <button className={`btn btn--large${loading ? " is-spinning" : ""}`} onClick={() => refresh(id)}>
                <Icon name="refresh" size={16} />
                {t("device.refresh")}
              </button>
              <span className="hero-ago num" aria-live="polite">
                {loading && !snapshot ? t("device.reading") : snapshot ? <Ago at={snapshot.takenAtMs} /> : null}
              </span>
            </div>
          )}
        </div>
      </section>

      {error && running && (
        <div className="notice notice--warn">
          <Icon name="alert" size={20} />
          <div className="notice-text">
            <h3>{t("error.title")}</h3>
            <p className="selectable">
              <ErrorText error={error} />
            </p>
          </div>
          <button className="btn" onClick={() => refresh(id)}>
            {t("error.retry")}
          </button>
        </div>
      )}

      <div className={`bento${showSnapshot ? "" : " bento--bare"}`}>
        {showSnapshot && (
          <>
            <OutputSection snapshot={snapshot} />
            <PowerSection snapshot={snapshot} />
            <StorageSection snapshot={snapshot} deviceId={id} exported={!!device?.links.includes("mass-storage")} />
            {device && <MusicSection device={device} browse={() => go({ kind: "library", source: id })} />}
            <SystemSection snapshot={snapshot} profile={profile} />
          </>
        )}
        {running && !snapshot && !error && (
          <>
            <div className="tile skeleton skeleton--wide" aria-busy="true" />
            <div className="tile skeleton" aria-busy="true" />
          </>
        )}
        <div className="bento-stack">
          {profile && <HardwareSection profile={profile} />}
          {device && <ConnectionSection device={device} />}
        </div>
      </div>
    </div>
  );
}
