import { BrandMark } from "../components/BrandMark";
import { Icon } from "../components/Icon";
import { useT } from "../i18n";
import type { Route } from "../routes";
import { useData, useReading } from "../state/data";
import { useLibrary } from "../state/library";
import { percent } from "../format";
import { artFor, displayName, modeTone, screenFor } from "../state/describe";
import { inTauri } from "../api/bridge";
import { DeviceRender } from "../components/render/DeviceRender";
import { Battery } from "../components/Battery";
import type { DeviceSummary, KnownDevice } from "../api/types";

interface Props {
  route: Route;
  go(route: Route): void;
}

function ConnectedItem({ device, active, onClick }: { device: DeviceSummary; active: boolean; onClick(): void }) {
  const t = useT();
  const { snapshot } = useReading(device.id);
  const level = snapshot?.power?.level;
  const charging = snapshot?.power?.status === "Charging";
  const tone = modeTone(device.mode);
  const art = artFor(device.profileId ?? snapshot?.identity.profileId);
  return (
    <button className={`side-device${active ? " is-active" : ""}`} onClick={onClick} aria-current={active ? "page" : undefined}>
      <span className="side-device-art">
        {art ? <DeviceRender art={art} scale={0.36} turned={false} screen={screenFor(device.mode, snapshot)} /> : <Icon name="player" size={22} />}
      </span>
      <span className="side-device-text">
        <span className="side-device-name">{displayName(device, snapshot, t("device.unknownModel"))}</span>
        <span className={`side-device-meta side-device-meta--${tone}`}>{t(`mode.${device.mode}`)}</span>
      </span>
      {level != null && (
        <span className="side-device-level num">
          <Battery level={level} charging={charging} size={18} />
          {level}
        </span>
      )}
    </button>
  );
}

function RememberedItem({ device, active, onClick }: { device: KnownDevice; active: boolean; onClick(): void }) {
  const t = useT();
  const art = artFor(device.profileId);
  return (
    <button className={`side-device is-remembered${active ? " is-active" : ""}`} onClick={onClick} aria-current={active ? "page" : undefined}>
      <span className="side-device-art">
        {art ? <DeviceRender art={art} scale={0.36} turned={false} screen={{ kind: "off" }} /> : <Icon name="player" size={22} />}
      </span>
      <span className="side-device-text">
        <span className="side-device-name">{device.model ?? device.family}</span>
        <span className="side-device-meta">{t("mode.disconnected")}</span>
      </span>
    </button>
  );
}

function LibraryLink({ active, onClick }: { active: boolean; onClick(): void }) {
  const t = useT();
  const { state, sync, pull } = useLibrary();
  const job = pull ?? sync;
  const p = job?.progress;
  const pct = p && p.totalBytes > 0 ? percent(p.doneBytes, p.totalBytes) : 0;
  return (
    <>
      <button className={`side-link${active ? " is-active" : ""}`} onClick={onClick}>
        <Icon name="library" />
        <span>{t("nav.library")}</span>
        {state && state.index.albums.length > 0 && <span className="side-link-count mono num">{state.index.albums.length}</span>}
      </button>
      {job && !job.report && (
        <div className="side-xfer num" aria-live="polite">
          <span className="side-xfer-bar">
            <span style={{ width: `${pct}%` }} />
          </span>
          {Math.floor(pct)}%
        </div>
      )}
    </>
  );
}

export function Sidebar({ route, go }: Props) {
  const t = useT();
  const { scan, known, env } = useData();
  const connectedIds = new Set(scan.devices.flatMap((d) => [d.id, d.serial].filter(Boolean) as string[]));
  const remembered = known.filter((k) => !connectedIds.has(k.serial) && !k.serial.includes(":"));

  return (
    <aside className="sidebar">
      <div className="brand">
        <BrandMark />
        <div className="brand-text">
          <span className="brand-top">
            {t("brand.top")}
            <span className="brand-stage" title={t("brand.stageNote")}>
              {t("brand.stage")}
            </span>
          </span>
          <span className="brand-name">{t("brand.name")}</span>
        </div>
      </div>

      <nav className="side-nav" aria-label={t("nav.devices")}>
        <button className={`side-link${route.kind === "home" ? " is-active" : ""}`} onClick={() => go({ kind: "home" })}>
          <Icon name="player" size={19} />
          <span>{t("home.eyebrow")}</span>
          <span className="side-link-count mono num">{scan.devices.length}</span>
        </button>
        <LibraryLink active={route.kind === "library"} onClick={() => go({ kind: "library" })} />
        <button className={`side-link${route.kind === "studio" || route.kind === "install" ? " is-active" : ""}`} onClick={() => go({ kind: "studio" })}>
          <Icon name="firmware" />
          <span>{t("nav.studio")}</span>
        </button>

        <div className="side-group">
          <span className="side-group-label eyebrow">{t("nav.connected")}</span>
          {scan.devices.length === 0 && <span className="side-empty">{t("nav.noneConnected")}</span>}
          {scan.devices.map((d) => (
            <ConnectedItem
              key={d.id}
              device={d}
              active={route.kind === "device" && route.id === d.id}
              onClick={() => go({ kind: "device", id: d.id })}
            />
          ))}
        </div>

        {remembered.length > 0 && (
          <div className="side-group">
            <span className="side-group-label eyebrow">{t("nav.remembered")}</span>
            {remembered.map((k) => (
              <RememberedItem
                key={k.serial}
                device={k}
                active={route.kind === "device" && route.id === k.serial}
                onClick={() => go({ kind: "device", id: k.serial })}
              />
            ))}
          </div>
        )}
      </nav>

      {!inTauri && (
        <p className="preview-badge" title={t("preview.title")}>
          <Icon name="info" size={13} />
          {t("preview.badge")}
        </p>
      )}
      <div className="side-foot">
        <button className={`side-link${route.kind === "settings" ? " is-active" : ""}`} onClick={() => go({ kind: "settings" })}>
          <Icon name="dial" />
          <span>{t("nav.settings")}</span>
        </button>
        {env && <span className="side-version mono">v{env.version}</span>}
      </div>
    </aside>
  );
}
