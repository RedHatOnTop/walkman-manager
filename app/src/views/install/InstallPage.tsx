import { useCallback, useEffect, useMemo, useState } from "react";
import { asError, bridge } from "../../api/bridge";
import type { BuildRecord, DeviceSummary, ErrorReport, Gate, InstallJob, InstallMode, PlanView, Stage } from "../../api/types";
import { ErrorText } from "../../components/ErrorText";
import { GuideStage, type StageScene } from "../../components/guide/GuideStage";
import { Icon } from "../../components/Icon";
import { GetTools, usePlatformTools, type Tools } from "../../components/PlatformTools";
import { bytes } from "../../format";
import { usePrefs, type StringKey } from "../../i18n";
import type { Route } from "../../routes";
import { useData } from "../../state/data";
import { artFor } from "../../state/describe";

const SCALE = 3.5;

/** Stages that must run to their end once started. */
const HOLD: Stage["kind"][] = ["unlock", "flash", "wipe", "waitUpdate"];

/** A clean install starts with the kit's first-run setup, whose last question turns adb on. */
const SETUP: { scene: StageScene; key: StringKey }[] = [
  { scene: "setupLanguage", key: "install.setup.pages" },
  { scene: "setupDebug", key: "install.setup.usb" },
  { scene: "allow", key: "install.back.allow" },
  { scene: "aboutKit", key: "install.setup.later" },
];

/** When adb is off on the installed system: the owner's way back to it, in order. */
const BACK: { scene: StageScene; key: StringKey }[] = [
  { scene: "settings", key: "install.back.settings" },
  { scene: "aboutKit", key: "install.back.about" },
  { scene: "usbDebugging", key: "install.back.usb" },
  { scene: "allow", key: "install.back.allow" },
];

/** The steps that bring a stock player to where the app can see it. */
const REACH: { scene: StageScene; key: StringKey }[] = [
  { scene: "connect", key: "install.reach.connect" },
  { scene: "about", key: "install.reach.about" },
  { scene: "usbDebugging", key: "install.reach.usb" },
  { scene: "allow", key: "install.reach.allow" },
];

const GATE_SCENE: Partial<Record<Gate["id"], StageScene>> = {
  oemUnlock: "oemUnlock",
  card: "card",
  battery: "charge",
  drive: "plug",
};

const AT_THE_PLAYER: Gate["id"][] = ["oemUnlock", "battery", "card"];

function useNow(on: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!on) return;
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, [on]);
  return now;
}

function clock(secs: number): string {
  const s = Math.max(0, Math.round(secs));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

function expected(st: Stage): number | null {
  switch (st.kind) {
    case "rebootBootloader":
      return 15;
    case "unlock":
    case "wipe":
      return 600;
    case "flash":
      return 2 + st.bytes / 20_000_000;
    case "setActive":
      return 2;
    case "reboot":
      return 5;
    case "waitBoot":
      return 90;
    case "verify":
      return 10;
    case "dexopt":
      return 60;
    case "copy":
      return 1 + st.bytes / 15_000_000;
    case "keep":
    case "pickWalkman":
      return 0;
    case "readBack":
      return 5 + st.bytes / 30_000_000;
    case "waitUpdate":
      return 150;
    case "checkLog":
      return 5;
    default:
      return null;
  }
}

/** The last part of a path on the drive. */
function base(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

/** Stages as the page lists them: a storage set's copies, one per file, fold into one row. */
interface Row {
  stage: Stage;
  from: number;
  to: number;
}

const onDrive = (st: Stage): st is Extract<Stage, { kind: "copy" | "keep" }> => st.kind === "copy" || st.kind === "keep";

function rows(stages: Stage[]): Row[] {
  const out: Row[] = [];
  stages.forEach((stage, i) => {
    const last = out[out.length - 1];
    if (last && onDrive(stage) && onDrive(last.stage) && last.to === i - 1) last.to = i;
    else out.push({ stage, from: i, to: i });
  });
  return out;
}

function rowExpected(stages: Stage[], r: Row): number | null {
  const each = stages.slice(r.from, r.to + 1).map(expected);
  return each.every((e) => e == null) ? null : each.reduce<number>((a, e) => a + (e ?? 0), 0);
}

/** What going back to stock says its own way; everything else it shares with an install. */
const OWN = new Set(["install.stage.verify", "install.do.setActive", "install.do.waitBoot", "install.do.verify", "install.scene.boot", "install.scene.done", "install.end.done", "install.end.unverified", "install.end.failed", "install.rescue", "install.verdict.kernel"]);

/** What an install onto an NW-A50's drive says its own way. */
const DRIVE_OWN = new Set(["install.end.done"]);

function said(key: string, mode: InstallMode | undefined, drive = false): StringKey {
  if (drive && DRIVE_OWN.has(key)) return `${key}.drive` as StringKey;
  return (mode === "restore" && OWN.has(key) ? `${key}.restore` : key) as StringKey;
}

function stageTitle(st: Stage, mode: InstallMode | undefined, t: ReturnType<typeof usePrefs>["t"]): string {
  if (st.kind === "flash") return t("install.stage.flash", { partition: st.partition });
  if (st.kind === "setActive") return t("install.stage.setActive", { slot: st.slot });
  if (st.kind === "copy" || st.kind === "keep") return t(`install.stage.${st.kind}`, { file: base(st.file) });
  return t(said(`install.stage.${st.kind}`, mode));
}

function rowTitle(stages: Stage[], r: Row, mode: InstallMode | undefined, t: ReturnType<typeof usePrefs>["t"]): string {
  if (r.from === r.to) return stageTitle(r.stage, mode, t);
  return t("install.drive.copies", { n: stages.slice(r.from, r.to + 1).filter((st) => st.kind === "copy").length });
}

/** Cycles through a storyboard of guide steps until the owner picks one. */
function Storyboard({ steps, art }: { steps: { scene: StageScene; key: StringKey }[]; art: string }) {
  const { t, lang } = usePrefs();
  const [at, setAt] = useState(0);
  const [held, setHeld] = useState(false);
  useEffect(() => {
    if (held) return;
    const timer = window.setInterval(() => setAt((i) => (i + 1) % steps.length), 6500);
    return () => window.clearInterval(timer);
  }, [held, steps.length]);
  const step = steps[at]!;
  return (
    <div className="story">
      <GuideStage art={art} scene={step.scene} scale={SCALE} lang={lang} />
      <ol className="story-steps">
        {steps.map((s, i) => (
          <li key={s.key}>
            <button
              className={`story-step${i === at ? " is-on" : ""}`}
              onClick={() => {
                setAt(i);
                setHeld(true);
              }}
            >
              <span className="story-n num">{i + 1}</span>
              <span className="story-text">{t(s.key)}</span>
            </button>
          </li>
        ))}
      </ol>
    </div>
  );
}

function GateRow({ g, look, onLook, tools, drive }: { g: Gate; look: StageScene | null; onLook(s: StageScene | null): void; tools: Tools; drive: boolean }) {
  const { t, lang } = usePrefs();
  const scene = GATE_SCENE[g.id];
  const state = g.ok === true ? "ok" : g.ok === false ? "closed" : "unknown";
  const vars = g.id === "room" ? { value: g.value == null ? "—" : bytes(Number(g.value), lang), want: bytes(Number(g.want ?? 0), lang) } : { value: g.value ?? "—", want: g.want ?? "" };
  const getTools = g.id === "fastboot" && state === "closed";
  const key = drive ? "install.drive.gate" : "install.gate";
  return (
    <li className={`gate gate--${state}`}>
      <span className="gate-mark">
        <Icon name={state === "ok" ? "check" : state === "closed" ? "alert" : "info"} size={15} />
      </span>
      <span className="gate-text">
        <span className="gate-name">{t(`${key}.${g.id}` as StringKey)}</span>
        <span className="gate-why">{getTools && g.value === "missing" ? t("install.gate.fastboot.missing") : t(`${key}.${g.id}.${state}` as StringKey, vars)}</span>
      </span>
      {scene && state !== "ok" && (
        <button className={`btn btn--small btn--ghost${look === scene ? " is-on" : ""}`} onClick={() => onLook(look === scene ? null : scene)}>
          {t("install.show")}
        </button>
      )}
      {getTools && (
        <div className="gate-more">
          <GetTools tools={tools} />
        </div>
      )}
    </li>
  );
}

function RouteList({ job, now }: { job: InstallJob; now: number }) {
  const { t } = usePrefs();
  return (
    <ol className="route">
      {rows(job.stages).map((r) => {
        const here = job.index >= r.from && job.index <= r.to;
        const state = job.index > r.to || (here && job.state === "done") ? "done" : here ? (job.state === "running" ? "now" : job.state) : "next";
        const exp = rowExpected(job.stages, r);
        const st = job.stages[job.index];
        const name = state === "now" && r.from < r.to && st && onDrive(st) ? stageTitle(st, job.mode, t) : rowTitle(job.stages, r, job.mode, t);
        return (
          <li key={r.from} className={`route-stop route-stop--${state}`}>
            <span className="route-dot" />
            <span className="route-name">{name}</span>
            <span className="route-time num">
              {state === "now" ? clock((now - job.stageStarted) / 1000) : exp != null && state === "next" ? `~${clock(exp)}` : ""}
            </span>
          </li>
        );
      })}
    </ol>
  );
}

/** An install onto an NW-A50's drive: the drive while files go over, the updater while the
 * player is away, and the picker it ends with. */
function driveScene(job: InstallJob): StageScene {
  const st = job.stages[job.index];
  if (job.state === "done") return "picker";
  if (st?.kind === "waitUpdate" && job.seen !== "drive") return "updating";
  return "drive";
}

function sceneFor(job: InstallJob, story: boolean): StageScene | "story" {
  const st = job.stages[job.index];
  if (job.state === "done") return "done";
  if (job.state === "failed") return job.seen === "fastboot" ? "fastboot" : st?.kind === "waitBoot" ? "recovery" : "idle";
  if (job.state !== "running") return job.seen === "fastboot" ? "fastboot" : "idle";
  switch (st?.kind) {
    case "rebootBootloader":
    case "setActive":
      return "fastboot";
    case "unlock":
    case "wipe":
      return "hold";
    case "flash":
      return "flash";
    case "reboot":
    case "waitBoot":
      return "boot";
    case "waitAdb":
      if (job.seen === "unauthorized") return "allow";
      return story && job.waiting ? "story" : "boot";
    default:
      return "idle";
  }
}

export function InstallPage({ build, go }: { build: string; go(r: Route): void }) {
  const { t, lang } = usePrefs();
  const { scan } = useData();
  const [record, setRecord] = useState<BuildRecord | null>(null);
  const [profile, setProfile] = useState<string | null>(null);
  const [job, setJob] = useState<InstallJob | null>(null);
  const [mode, setMode] = useState<InstallMode | null>(null);
  const [view, setView] = useState<PlanView | null>(null);
  const [error, setError] = useState<ErrorReport | null>(null);
  const [confirm, setConfirm] = useState("");
  const [backedUp, setBackedUp] = useState(false);
  const [look, setLook] = useState<StageScene | null>(null);
  const [starting, setStarting] = useState(false);
  const [named, setNamed] = useState<string | null>(null);
  /** The player was sent back to Android from fastboot and is not up yet. */
  const [restarting, setRestarting] = useState(false);
  const now = useNow(job?.state === "running");

  useEffect(() => {
    void bridge()
      .then((b) => b.studioView())
      .then((v) => {
        const r = v.builds.find((x) => x.id === build) ?? null;
        setRecord(r);
        setProfile(v.kits.find((k) => k.id === r?.kit)?.profile ?? null);
      }, (e) => setError(asError(e)));
    const offs: Promise<() => void>[] = [];
    // The app keeps the last job; another build's would read as this one's result.
    const mine = (j: InstallJob | null) => j && j.build === build && setJob(j);
    void bridge().then((b) => {
      void b.installState().then(mine);
      offs.push(b.onInstall(mine));
    });
    return () => offs.forEach((o) => void o.then((f) => f()));
  }, [build]);

  const candidates = useMemo(() => scan.devices.filter((d) => d.profileId === profile), [scan.devices, profile]);
  const device: DeviceSummary | undefined = job ? scan.devices.find((d) => d.id === job.device) : candidates[0];
  /** The build goes onto an NW-A50's WALKMAN drive; the drive gate says whether it is there. */
  const drive = record?.storage != null;
  const reachable = device && (drive ? device.mode !== "download" && device.mode !== "no-permission" : device.mode === "running" || device.mode === "bootloader");
  // Off USB while it restarts, the player is still the one the page was opened for.
  useEffect(() => {
    if (device?.model) setNamed(device.model);
  }, [device?.model]);
  useEffect(() => {
    if (!restarting) return;
    if (device?.mode === "running") setRestarting(false);
    const t = window.setTimeout(() => setRestarting(false), 4 * 60_000);
    return () => window.clearTimeout(t);
  }, [restarting, device?.mode]);

  const restoring = record?.options.variant === "stock";
  const loadPlan = useCallback(
    (m: InstallMode | null) => {
      if (!device || !reachable || !record) {
        setView(null);
        return;
      }
      const ask = (wanted: InstallMode) => bridge().then((b) => b.installPlan(build, device.id, wanted));
      setError(null);
      (m ? ask(m) : restoring ? ask("restore") : ask("update").then((u) => (u.plan.ready ? u : ask("clean"))))
        .then((v) => {
          setView(v);
          setMode(v.plan.mode);
        })
        .catch((e) => setError(asError(e)));
    },
    [build, device, reachable, record, restoring],
  );

  const tools = usePlatformTools(() => loadPlan(mode));

  const deviceKey = device ? `${device.id}:${device.mode}` : "none";
  useEffect(() => {
    if (job && job.state === "running") return;
    loadPlan(mode);
    // The plan follows the player's state, not every re-render.
  }, [deviceKey, build, record?.id]);

  // The owner clears these at the player, which leaves its mode, and so the key above, as it was.
  const waiting = !job && view?.plan.gates.some((g) => g.ok === false && AT_THE_PLAYER.includes(g.id));
  useEffect(() => {
    if (!waiting) return;
    const t = window.setTimeout(() => loadPlan(mode), 4000);
    return () => window.clearTimeout(t);
  }, [waiting, view, error]);

  const switchMode = (m: InstallMode) => {
    setMode(m);
    setConfirm("");
    loadPlan(m);
  };

  const start = () => {
    if (!device || !view || !mode) return;
    setStarting(true);
    setError(null);
    void bridge()
      .then((b) => b.installStart(build, device.id, mode, confirm))
      .catch((e) => setError(asError(e)))
      .finally(() => setStarting(false));
  };

  const again = () => {
    void bridge()
      .then((b) => b.installClear())
      .then(() => {
        setJob(null);
        setConfirm("");
        loadPlan(mode);
      });
  };

  const leave = () => {
    setError(null);
    void bridge()
      .then((b) => b.installLeave())
      .then(() => {
        setJob(null);
        setConfirm("");
        setRestarting(true);
      })
      .catch((e) => setError(asError(e)));
  };

  const art = artFor(profile) ?? "zx500";
  const set = record?.set ?? view?.set ?? null;
  const files = set?.files ?? record?.storage?.files ?? null;
  const plan = view?.plan;
  const running = job?.state === "running";
  const current = job ? job.stages[job.index] : undefined;
  const driveOk = plan?.gates.find((g) => g.id === "drive")?.ok === true;
  const scene: StageScene | "story" = job
    ? drive
      ? driveScene(job)
      : sceneFor(job, job.mode !== "update")
    : restarting
      ? "boot"
      : look ?? (drive ? (driveOk ? "drive" : "plug") : !device || device.mode === "storage-only" ? "story" : device.mode === "unauthorized" ? "allow" : device.mode === "bootloader" ? "fastboot" : "idle");
  // Back on stock, USB debugging is found where a stock player keeps it.
  const storySteps = job?.mode === "clean" && !drive ? SETUP : job && job.mode !== "restore" ? BACK : REACH;
  const name = device?.model ?? named ?? device?.family ?? profile?.toUpperCase() ?? "NW-ZX500";
  const stuck = job != null && (job.state === "failed" || job.state === "cancelled") && job.seen === "fastboot";

  return (
    <div className="page install">
      <header className="install-hero">
        <div className="page-titles">
          <span className="eyebrow">{t(restoring ? "install.eyebrow.restore" : "install.eyebrow")}</span>
          <h1 className="page-title">{t(restoring ? "install.title.restore" : "install.title", { name })}</h1>
          {record && (
            <p className="install-build">
              <span className="chip">{t(`install.variant.${record.options.variant}` as StringKey)}</span>
              {set?.root && <span className="chip">{t("install.root")}</span>}
              <span className="mono">{record.id}</span>
              <span className="install-commit mono">{record.commit.slice(0, 7)}</span>
              {record.storage && <span className="chip">{t(`build.key.${record.storage.upg.key}` as StringKey)}</span>}
              {files && <span className="num">{bytes(Object.values(files).reduce((a, f) => a + f.bytes, 0), lang)}</span>}
            </p>
          )}
        </div>
        {!job && view && !restoring && (
          <div className="segmented install-modes" role="tablist">
            {(["clean", "update"] as const).map((m) => (
              <button key={m} className={mode === m ? "is-on" : ""} onClick={() => switchMode(m)} role="tab" aria-selected={mode === m}>
                {t(`install.mode.${m}` as StringKey)}
              </button>
            ))}
          </div>
        )}
      </header>

      <div className="install-grid">
        <section className="install-stage tile" aria-label={t("install.stageLabel")}>
          {scene === "story" ? (
            <Storyboard key={storySteps === SETUP ? "setup" : storySteps === BACK ? "back" : "reach"} steps={storySteps} art={art} />
          ) : (
            <GuideStage
              art={art}
              scene={scene}
              scale={SCALE}
              lang={lang}
              label={scene === "idle" ? undefined : t(restarting && !job ? "install.scene.restart" : said(`install.scene.${scene}`, job?.mode, drive))}
            />
          )}
        </section>

        <section className="install-side">
          {error && (
            <div className="notice notice--warn">
              <Icon name="alert" />
              <p className="selectable">
                <ErrorText error={error} />
              </p>
            </div>
          )}

          {!job && restarting && (
            <div className="tile install-card">
              <h2 className="install-card-title">{t("install.restarting")}</h2>
              <p className="install-card-text">{t("install.restarting.text")}</p>
            </div>
          )}

          {!job && !restarting && !device && (
            <div className="tile install-card">
              <h2 className="install-card-title">{t("install.none.title")}</h2>
              <p className="install-card-text">{t(drive ? "install.drive.none.text" : "install.none.text")}</p>
            </div>
          )}

          {!job && !restarting && device && !reachable && (
            <div className="tile install-card">
              <h2 className="install-card-title">{t(`install.mode-${device.mode}` as StringKey)}</h2>
              <p className="install-card-text">{t(`install.mode-${device.mode}.text` as StringKey)}</p>
            </div>
          )}

          {!job && !restarting && plan && view && (
            <>
              <div className="tile install-card">
                <p className="install-card-text">
                  {drive
                    ? t(`install.drive.mode.${plan.mode}.text` as StringKey)
                    : plan.mode === "clean" && !plan.stages.some((s) => s.kind === "unlock")
                      ? t("install.mode.clean.unlocked.text")
                      : t(`install.mode.${plan.mode}.text` as StringKey)}
                </p>
                <ul className="gates">
                  {plan.gates.map((g) => (
                    <GateRow key={g.id} g={g} look={look} onLook={setLook} tools={tools} drive={drive} />
                  ))}
                </ul>
              </div>

              <div className="tile install-card">
                <h2 className="install-card-title">{t("install.plan")}</h2>
                <ol className="route route--plan">
                  {rows(plan.stages).map((r) => {
                    const exp = rowExpected(plan.stages, r);
                    const yours = r.stage.kind === "trigger" || (r.stage.kind === "waitAdb" && plan.mode !== "update");
                    return (
                      <li key={r.from} className="route-stop route-stop--next">
                        <span className="route-dot" />
                        <span className="route-name">{rowTitle(plan.stages, r, plan.mode, t)}</span>
                        <span className="route-time num">{exp ? `~${clock(exp)}` : yours ? t("install.yourTurn") : ""}</span>
                        {r.from < r.to && (
                          <details className="route-files">
                            <summary>{t("install.drive.files")}</summary>
                            <ul>
                              {plan.stages.slice(r.from, r.to + 1).map(
                                (st) =>
                                  onDrive(st) && (
                                    <li key={st.file} className={st.kind === "keep" ? "is-kept" : ""}>
                                      <span className="mono build-path">
                                        <span className="build-path-dir">{st.file.slice(0, st.file.lastIndexOf("/") + 1)}</span>
                                        {base(st.file)}
                                      </span>
                                      <span className="num">{st.kind === "copy" ? bytes(st.bytes, lang) : t("install.drive.kept")}</span>
                                    </li>
                                  ),
                              )}
                            </ul>
                          </details>
                        )}
                      </li>
                    );
                  })}
                </ol>
                <p className="install-total num">
                  {t("install.total", { time: clock(plan.stages.reduce((a, st) => a + (expected(st) ?? 0), 0)) })}
                </p>
              </div>

              <div className={`tile install-card install-go${plan.erases ? " is-erasing" : ""}`}>
                {plan.erases && (
                  <label className="install-ack">
                    <input type="checkbox" checked={backedUp} onChange={(e) => setBackedUp(e.target.checked)} />
                    <span>{t(drive ? "install.drive.erases" : "install.erases")}</span>
                  </label>
                )}
                {view.stock && !restoring && <p className="install-stock">{t("install.stock")} <span className="mono selectable">{view.stock}</span></p>}
                <label className="install-confirm">
                  <span>{t("install.type", { word: plan.confirm })}</span>
                  <input
                    className="mono"
                    value={confirm}
                    onChange={(e) => setConfirm(e.target.value)}
                    placeholder={plan.confirm}
                    spellCheck={false}
                    autoComplete="off"
                  />
                </label>
                <button
                  className="btn btn--primary install-start"
                  disabled={!plan.ready || confirm.trim() !== plan.confirm || (plan.erases && !backedUp) || starting}
                  onClick={start}
                >
                  <Icon name={plan.mode === "restore" ? "back" : "firmware"} size={16} />
                  {t(`install.start.${plan.mode}` as StringKey)}
                </button>
              </div>
            </>
          )}

          {job && (
            <div className={`tile install-card install-now install-now--${job.state}`} aria-live="polite">
              {running && current && (
                <>
                  <span className="eyebrow">{t("install.stepOf", { n: job.index + 1, m: job.stages.length })}</span>
                  <h2 className="install-now-title">{stageTitle(current, job.mode, t)}</h2>
                  <p className="install-card-text">
                    {current.kind === "waitAdb" && job.seen === "unauthorized"
                      ? t("install.do.allow")
                      : t(current.kind === "waitAdb" ? (`install.do.waitAdb.${job.mode}` as StringKey) : said(`install.do.${current.kind}`, job.mode))}
                  </p>
                  <div className="install-meter">
                    <span className="install-meter-bar">
                      <span style={{ width: `${Math.round((job.progress ?? 0) * 100)}%` }} className={job.progress == null ? "is-idle" : ""} />
                    </span>
                    <span className="install-meter-time num">
                      {clock((now - job.stageStarted) / 1000)}
                      {expected(current) != null && <span> / ~{clock(expected(current)!)}</span>}
                    </span>
                  </div>
                  <div className="install-actions">
                    {current.kind === "waitAdb" && job.mode !== "update" && (
                      <button className="btn btn--ghost" onClick={() => void bridge().then((b) => b.installSkip())}>
                        {t("install.skip")}
                      </button>
                    )}
                    {!HOLD.includes(current.kind) ? (
                      <button className="btn btn--ghost" onClick={() => void bridge().then((b) => b.installCancel())}>
                        {t("install.cancel")}
                      </button>
                    ) : (
                      <span className="install-hold">
                        <Icon name="plug" size={15} />
                        {t("install.hold")}
                      </span>
                    )}
                  </div>
                </>
              )}
              {!running && (
                <>
                  <span className="eyebrow">{t(`install.end.${job.state}.eyebrow` as StringKey)}</span>
                  <h2 className="install-now-title">{t(said(`install.end.${job.state}`, job.mode, drive))}</h2>
                  {drive && job.state === "done" && <p className="install-card-text">{t("install.drive.next")}</p>}
                  {job.error && (
                    <p className="install-card-text selectable">
                      <ErrorText error={job.error} />
                    </p>
                  )}
                  {job.state === "failed" && current?.kind === "waitBoot" && !stuck && <p className="install-card-text">{t(said("install.rescue", job.mode))}</p>}
                  {stuck && <p className="install-card-text">{t(job.touched ? "install.stuck.touched" : "install.stuck")}</p>}
                  {job.drive && (
                    <ul className="verdict">
                      {job.drive.files.length > 0 && (
                        <li className={job.drive.files.every((f) => f.ok) ? "is-ok" : "is-bad"}>
                          <Icon name={job.drive.files.every((f) => f.ok) ? "check" : "alert"} size={14} />
                          {t("install.verdict.drive", { ok: job.drive.files.filter((f) => f.ok).length, of: job.drive.files.length })}
                        </li>
                      )}
                      {job.drive.lines.map(([line, ok]) => (
                        <li key={line} className={ok ? "is-ok" : "is-bad"}>
                          <Icon name={ok ? "check" : "alert"} size={14} />
                          {t("install.verdict.log")} <span className="mono">{line}</span>
                        </li>
                      ))}
                      {job.drive.upgDone != null && (
                        <li className={job.drive.upgDone ? "is-ok" : "is-bad"}>
                          <Icon name={job.drive.upgDone ? "check" : "alert"} size={14} />
                          {t(job.drive.upgDone ? "install.verdict.upgDone" : "install.verdict.upgNotDone")}
                        </li>
                      )}
                    </ul>
                  )}
                  {job.verdict && (
                    <ul className="verdict">
                      <li className={job.verdict.slotOk ? "is-ok" : "is-bad"}>
                        <Icon name={job.verdict.slotOk ? "check" : "alert"} size={14} />
                        {t("install.verdict.slot", { slot: job.verdict.slot ?? "?" })}
                      </li>
                      <li className={job.verdict.kernelOk ? "is-ok" : "is-bad"}>
                        <Icon name={job.verdict.kernelOk ? "check" : "alert"} size={14} />
                        {t(said("install.verdict.kernel", job.mode))}
                      </li>
                      <li className={job.verdict.files.every((f) => f.ok) ? "is-ok" : "is-bad"}>
                        <Icon name={job.verdict.files.every((f) => f.ok) ? "check" : "alert"} size={14} />
                        {t("install.verdict.files", { ok: job.verdict.files.filter((f) => f.ok).length, of: job.verdict.files.length })}
                      </li>
                      {job.verdict.absent.map(([path, gone]) => (
                        <li key={path} className={gone ? "is-ok" : "is-bad"}>
                          <Icon name={gone ? "check" : "alert"} size={14} />
                          {t("install.verdict.absent")} <span className="mono">{path}</span>
                        </li>
                      ))}
                    </ul>
                  )}
                  <div className="install-actions">
                    {job.state === "done" || job.state === "unverified" ? (
                      <button className="btn btn--primary" onClick={() => go({ kind: "device", id: job.device })}>
                        {t("install.toPlayer")}
                      </button>
                    ) : (
                      <button className="btn btn--primary" onClick={again}>
                        {t("install.again")}
                      </button>
                    )}
                    {stuck && !job.touched && (
                      <button className="btn btn--ghost" onClick={leave}>
                        {t("install.leave")}
                      </button>
                    )}
                    <button className="btn btn--ghost" onClick={() => go({ kind: "studio" })}>
                      {t("install.toStudio")}
                    </button>
                  </div>
                </>
              )}
            </div>
          )}

          {job && (
            <div className="tile install-card">
              <RouteList job={job} now={now} />
            </div>
          )}

          {job && job.log.length > 0 && (
            <details className="install-log tile">
              <summary>{t("install.log")}</summary>
              <pre className="mono selectable">{job.log.join("\n")}</pre>
            </details>
          )}
        </section>
      </div>
    </div>
  );
}
