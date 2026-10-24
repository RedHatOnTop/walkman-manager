import { useState, type ReactNode } from "react";
import type { DeviceSummary, Profile, Snapshot } from "../../api/types";
import { Icon } from "../../components/Icon";
import { bytes, dsdLabel, kib, masked, percent, sampleRate, usbId } from "../../format";
import { usePrefs, useT } from "../../i18n";
import type { StringKey } from "../../i18n/strings";
import { activeStream, dacCard } from "../../state/describe";
import { useLibrary } from "../../state/library";

/** The translation for `key`, or `fallback` when the dictionary has none (raw kernel wording). */
export function label(t: ReturnType<typeof useT>, key: string, fallback: string): string {
  const out = t(key as StringKey);
  return out === key ? fallback : out;
}

export function Section({ kind, title, aside, children }: { kind: string; title: string; aside?: ReactNode; children: ReactNode }) {
  return (
    <section className={`tile sec sec--${kind}`}>
      <header className="sec-head">
        <h2 className="sec-title">{title}</h2>
        {aside && <div className="sec-aside">{aside}</div>}
      </header>
      {children}
    </section>
  );
}

export function streamText(snapshot: Snapshot | null, t: ReturnType<typeof useT>): string | null {
  const st = activeStream(snapshot);
  if (!st || st.state !== "running" || !st.encoding) return null;
  return st.encoding.type === "pcm"
    ? t("output.pcm", { rate: sampleRate(st.encoding.rate), bits: st.encoding.bits })
    : t("output.dsd", { label: dsdLabel(st.encoding.rate) });
}

export function OutputSection({ snapshot }: { snapshot: Snapshot }) {
  const t = useT();
  const card = dacCard(snapshot);
  const st = activeStream(snapshot);
  const playing = st?.state === "running";
  const detail = streamText(snapshot, t);
  const streams = card ? snapshot.audio.streams.filter((s) => s.card === card.index) : [];
  const nodes: { key: StringKey; sub: string }[] = [
    { key: "output.path.source", sub: snapshot.system.platform === "android" ? "AudioFlinger" : t("output.path.sonyAudio") },
    { key: "output.path.alsa", sub: playing && st?.alsa ? st.alsa : card ? t("output.path.card", { n: card.index }) : "—" },
    { key: "output.path.dac", sub: "CXD3778GF" },
    { key: "output.path.jack", sub: "S-Master HX" },
  ];

  return (
    <Section kind="output" title={t("section.output")}>
      <p className={`out-line${playing ? " is-live" : ""}`}>
        <span className="out-word">{playing ? t("output.playing") : t("output.idle")}</span>
        <span className="num">
          {playing ? `${detail ?? st?.format ?? ""}${st?.channels ? ` · ${t("output.channels", { n: st.channels })}` : ""}` : t("output.idleDetail")}
        </span>
      </p>

      <ol className={`chain${playing ? " is-live" : ""}`} aria-label={nodes.map((n) => t(n.key)).join(" → ")}>
        {nodes.map((n) => (
          <li key={n.key}>
            <span className="chain-name">{t(n.key)}</span>
            <span className="chain-sub mono">{n.sub}</span>
          </li>
        ))}
      </ol>

      {card ? (
        <p className="pcm">
          <span className="pcm-label">{t("output.pcmDevices")}</span>
          {streams.map((s) => (
            <span key={s.device} className={`pcm-token mono pcm-token--${s.state === "running" ? "live" : s.state === "closed" ? "off" : "open"}`} title={s.format ?? s.state}>
              pcm{s.device}
            </span>
          ))}
        </p>
      ) : snapshot.audio.outputs.length > 0 ? (
        <p className="pcm">
          <span className="pcm-label">{t("output.flingerOutputs")}</span>
          {snapshot.audio.outputs.map((o) => (
            <span key={o.name} className={`pcm-token mono pcm-token--${o.active ? "live" : "off"}`} title={`${o.name} · ${o.format ?? ""}`}>
              {o.kind}
            </span>
          ))}
        </p>
      ) : (
        <p className="sec-note">{t("output.noCard")}</p>
      )}
    </Section>
  );
}

export function PowerSection({ snapshot }: { snapshot: Snapshot }) {
  const t = useT();
  const p = snapshot.power;
  if (!p) return null;
  const level = p.level ?? 0;
  const charging = p.status === "Charging";
  const statusKey = (p.status ? `power.${p.status}` : "power.unknown") as StringKey;
  return (
    <Section kind="power" title={t("section.power")}>
      <div className="power">
        <div className={`meter${charging ? " is-charging" : level < 15 ? " is-low" : ""}`} role="img" aria-label={`${level} %`}>
          <span className="meter-figure num">{p.level ?? "—"} %</span>
          <span className="meter-cells" aria-hidden="true">
            {Array.from({ length: 20 }, (_, i) => (
              <i key={i} className={i < Math.round(level / 5) ? "is-on" : undefined} />
            ))}
          </span>
        </div>
        <Rows
          rows={[
            [t("power.status"), <>{(charging || p.externalPower) && <Icon name="plug" size={14} />}{label(t, statusKey, p.status ?? "")}</>],
            [t("power.voltage"), p.voltageMv != null ? `${(p.voltageMv / 1000).toFixed(2)} V` : null, "num"],
            [t("power.temperature"), p.temperatureC != null ? `${p.temperatureC.toFixed(1)} °C` : null, "num"],
            [t("power.healthLabel"), p.health ? label(t, `health.${p.health}`, p.health) : null],
          ]}
        />
      </div>
    </Section>
  );
}

/** Bytes in each volume's music folder, once the music listing has been read. */
function useMusicBytes(deviceId: string): Partial<Record<string, number>> {
  const { music } = useLibrary();
  const vols = music[deviceId]?.volumes;
  const out: Partial<Record<string, number>> = {};
  for (const v of vols ?? []) out[v.kind] = (out[v.kind] ?? 0) + v.files.reduce((n, f) => n + f.size, 0);
  return out;
}

export function StorageSection({ snapshot, deviceId, exported }: { snapshot: Snapshot; deviceId: string; exported: boolean }) {
  const t = useT();
  const { lang } = usePrefs();
  const musicBytes = useMusicBytes(deviceId);
  const vols = snapshot.volumes;
  const hasCard = vols.some((v) => v.kind === "card");
  return (
    <Section kind="storage" title={t("section.storage")}>
      {vols.length === 0 && <p className="sec-note">{t(exported ? "storage.exported" : "storage.none")}</p>}
      <div className="vols">
        {vols.map((v) => {
          const music = Math.min(musicBytes[v.kind] ?? 0, v.usedBytes);
          const other = v.usedBytes - music;
          return (
            <div className="vol" key={v.mount}>
              <div className="vol-head">
                <span className="vol-name">
                  <Icon name={v.kind === "card" ? "card" : "chip"} size={16} />
                  {t(v.kind === "card" ? "storage.card" : "storage.internal")}
                  <span className="tag mono">{v.filesystem === "vfat" ? "FAT32" : v.filesystem}</span>
                </span>
                <span className="vol-total num">{t("storage.of", { v: bytes(v.totalBytes, lang) })}</span>
              </div>
              <div className="vol-free num">
                <span className="vol-free-value">{bytes(v.freeBytes, lang)}</span>
                <span className="vol-free-label">{t("storage.freeLabel")}</span>
              </div>
              <div className="bar bar--split" role="img" aria-label={t("storage.free", { v: bytes(v.freeBytes, lang) })}>
                {music > 0 && <span className="bar-music" style={{ width: `${percent(music, v.totalBytes)}%` }} />}
                <span className="bar-other" style={{ width: `${percent(other, v.totalBytes)}%` }} />
              </div>
              <div className="vol-legend num">
                {music > 0 && (
                  <span>
                    <i className="key-music" />
                    {t("storage.music", { v: bytes(music, lang) })}
                  </span>
                )}
                <span>
                  <i className="key-other" />
                  {t(music > 0 ? "storage.other" : "storage.used", { v: bytes(other, lang) })}
                </span>
              </div>
            </div>
          );
        })}
        {!hasCard && vols.length > 0 && (
          <div className="vol vol--absent">
            <span className="vol-name">
              <Icon name="card" size={16} />
              {t("storage.cardMissing")}
            </span>
          </div>
        )}
      </div>
    </Section>
  );
}

function Rows({ rows }: { rows: [string, ReactNode | null | undefined, string?][] }) {
  return (
    <dl className="specs">
      {rows
        .filter(([, v]) => v != null && v !== "")
        .map(([k, v, cls]) => (
          <div className="spec" key={k}>
            <dt>{k}</dt>
            <dd className={cls}>{v}</dd>
          </div>
        ))}
    </dl>
  );
}

export function SystemSection({ snapshot, profile }: { snapshot: Snapshot; profile?: Profile }) {
  const t = useT();
  const { lang } = usePrefs();
  const s = snapshot.system;
  const android = s.platform === "android";
  const firmware =
    !android && s.firmware ? (
      <span className="inline">
        <span className="mono">{s.firmware}</span>
        {profile?.stockFirmwareLatest === s.firmware && <span className="tag">{t("system.latest")}</span>}
      </span>
    ) : null;
  const os = android && s.apiLevel ? (
    <span className="inline">
      {s.os}
      <span className="tag mono">{t("system.api", { n: s.apiLevel })}</span>
    </span>
  ) : (
    s.os
  );
  return (
    <Section kind="system" title={t("section.system")}>
      <Rows
        rows={[
          [t("system.os"), os],
          [t("system.distribution"), s.distribution === "Sony firmware" ? t("system.sonyFirmware") : s.distribution],
          [t("system.firmware"), firmware],
          [t("system.buildId"), android ? s.buildId : null, "mono selectable"],
          [t("system.buildType"), android ? s.buildType : null, "mono"],
          [t("system.securityPatch"), android ? s.securityPatch : null, "mono num"],
          [t("system.vendorFirmware"), s.vendorFirmware, "mono"],
          [t("system.kernel"), s.kernel, "mono selectable"],
          [t("system.kernelDate"), s.kernelDate, "mono"],
          [t("system.slot"), s.slot?.toUpperCase(), "mono"],
          [
            t("system.bootloader"),
            s.bootloaderUnlocked == null ? null : (
              <span className={s.bootloaderUnlocked ? "flag flag--warn" : "flag"}>
                <Icon name={s.bootloaderUnlocked ? "lockOpen" : "shield"} size={14} />
                {s.bootloaderUnlocked ? t("system.unlocked") : t("system.locked")}
              </span>
            ),
          ],
          [t("system.verifiedBoot"), s.verifiedBoot, "mono"],
          [
            t("system.container"),
            s.container ? (
              <span className="inline">
                <span className="tag">{t("system.containerMounted")}</span>
                <span className="mono">{s.container.root}</span>
              </span>
            ) : null,
          ],
          [
            t("system.memory"),
            snapshot.memory
              ? snapshot.memory.availableKb != null
                ? `${kib(snapshot.memory.availableKb, lang)} / ${kib(snapshot.memory.totalKb, lang)}`
                : kib(snapshot.memory.totalKb, lang)
              : null,
            "mono num",
          ],
        ]}
      />
    </Section>
  );
}

export function HardwareSection({ profile }: { profile: Profile }) {
  const t = useT();
  const [w, h, d] = profile.bodyMm;
  return (
    <Section kind="hardware" title={t("section.hardware")}>
      <Rows
        rows={[
          [t("hardware.soc"), profile.soc],
          [t("hardware.dac"), profile.dac],
          [t("hardware.display"), profile.display],
          [t("hardware.body"), `${w} × ${h} × ${d} mm`, "mono num"],
          [t("hardware.mass"), `${profile.massG} g`, "mono num"],
        ]}
      />
    </Section>
  );
}

export function ConnectionSection({ device }: { device: DeviceSummary }) {
  const t = useT();
  const [reveal, setReveal] = useState(false);
  return (
    <Section kind="connection" title={t("section.connection")}>
      <div className="links">
        {device.links.map((l) => (
          <span className="chip" key={l}>
            <Icon name={l === "mass-storage" || l === "mtp" ? "card" : l.startsWith("adb") ? "link" : "usb"} size={14} />
            {t(`link.${l}`)}
          </span>
        ))}
      </div>
      <Rows
        rows={[
          [t("connection.port"), device.usb?.port, "mono"],
          [t("connection.speed"), device.usb?.speed, "mono"],
          [t("connection.usbId"), device.usb ? usbId(device.usb.vid, device.usb.pid) : null, "mono"],
          [
            t("connection.serial"),
            device.serial ? (
              <span className="serial">
                <span className="mono selectable">{reveal ? device.serial : masked(device.serial)}</span>
                <button className="icon-btn icon-btn--small" onClick={() => setReveal((r) => !r)} aria-label={reveal ? t("connection.hide") : t("connection.show")}>
                  <Icon name={reveal ? "eyeOff" : "eye"} size={14} />
                </button>
              </span>
            ) : null,
          ],
        ]}
      />
    </Section>
  );
}
