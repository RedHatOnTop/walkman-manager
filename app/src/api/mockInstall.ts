// Development only: an install in a plain browser. `?player=stock|kit|fastboot` sets what
// the connected NW-ZX507 runs; `?job=<stage kind>|done|unverified|failed` opens on a job
// stopped at that point, for screenshots; `?mode=restore` adds a stock build and opens the
// job on it. The NW-A50's build installs onto its drive: `?drive=fresh|installed|full|none`
// sets what the drive holds, and `?kit=a50` opens the `job` preset on that build.
import { findBuild } from "./mockStudio";
import { flasher } from "./mockTools";
import type { DriveFacts, DriveVerdict, Facts, Gate, InstallJob, InstallMode, InstallPlan, InstallSet, PlanView, Stage, StorageSet } from "./types";

const params = typeof location === "undefined" ? new URLSearchParams() : new URLSearchParams(location.search);
const player = params.get("player") ?? "stock";
const SERIAL = "0000test0zx50700";
const drivePlayer = params.get("drive") ?? "fresh";
const A50 = "TESTA55SERIAL01";
const A50_BUILD = "2026-10-22-154109";

function driveFacts(set: StorageSet): DriveFacts {
  const none = drivePlayer === "none";
  const installed = drivePlayer === "installed";
  return {
    serial: A50,
    profile: "nw-a50",
    drive: none ? null : { disk: "/dev/sdb", block: "/dev/sdb", serial: A50, label: "WALKMAN", bytes: 12_855_712_768, mount: "/run/media/listener/WALKMAN" },
    free: none ? null : drivePlayer === "full" ? 1_400_000_000 : 9_400_000_000,
    present: installed ? Object.fromEntries(set.copy.map((p) => [p, set.files[p]?.bytes ?? 0])) : {},
    choice: installed ? "2\n" : null,
    backupOnDrive: none ? null : "ab".repeat(32),
    backupBuilt: "ab".repeat(32),
    elevate: true,
  };
}

// The same rules as flash::storage::plan.
function drivePlan(set: StorageSet, f: DriveFacts, mode: InstallMode): InstallPlan {
  const has = (p: string) => p in f.present;
  const kept = (p: string) => mode === "update" && set.keep.includes(p) && has(p);
  const bytesOf = (p: string) => set.files[p]?.bytes ?? 0;
  const stages: Stage[] = set.copy.map((p) => (kept(p) ? { kind: "keep", file: p } : { kind: "copy", file: p, bytes: bytesOf(p) }));
  const copied = set.copy.filter((p) => !kept(p));
  stages.push({ kind: "readBack", bytes: copied.reduce((a, p) => a + bytesOf(p), 0) });
  if (f.choice != null && f.choice.trim() !== "1") stages.push({ kind: "pickWalkman", file: "wmmod-android/boot-choice" });
  stages.push({ kind: "trigger" }, { kind: "waitUpdate" }, { kind: "checkLog" });
  let grown = 0;
  let peak = 0;
  for (const p of copied) {
    peak = Math.max(peak, grown + bytesOf(p));
    grown += bytesOf(p) - (f.present[p] ?? 0);
  }
  const need = peak + (16 << 20);
  const mounted = f.drive != null && f.drive.bytes > 0 && f.drive.mount != null;
  const gates: Gate[] = [
    { id: "model", ok: f.profile === "nw-a50", value: f.profile, want: "nw-a50" },
    { id: "drive", ok: mounted, value: f.drive?.block ?? null, want: null },
    { id: "room", ok: f.free == null ? null : f.free >= need, value: f.free == null ? null : String(f.free), want: String(need) },
    { id: "backup", ok: f.backupOnDrive == null ? null : f.backupOnDrive === f.backupBuilt, value: null, want: null },
    { id: "elevate", ok: f.elevate, value: null, want: null },
  ];
  if (mode === "update") gates.push({ id: "hasData", ok: set.keep.length > 0 && set.keep.every(has), value: null, want: null });
  const erased = mode === "clean" ? set.keep.find(has) : undefined;
  return {
    mode,
    stages,
    gates,
    erases: erased != null,
    confirm: erased ? erased.slice(erased.lastIndexOf("/") + 1) : "bootswitcher.sh",
    ready: mode !== "restore" && mounted && f.free != null && gates.every((g) => g.ok !== false),
  };
}

function facts(): Facts {
  const booted = player !== "fastboot";
  const kit = player === "kit";
  return {
    serial: SERIAL,
    profile: "nw-zx500",
    booted,
    vendorFirmware: booted ? "4.06" : null,
    apiLevel: booted ? (kit ? 33 : 28) : null,
    slot: kit ? "b" : player === "fastboot" ? null : "a",
    unlocked: kit ? true : player === "fastboot" ? null : false,
    oemUnlockAllowed: booted ? true : null,
    running: kit ? "release" : null,
    battery: booted ? 82 : null,
    card: false,
    fastboot: flasher(),
  };
}

// The same rules as flash::plan.
function plan(set: InstallSet, f: Facts, mode: InstallMode): InstallPlan {
  const gates: Gate[] = [
    { id: "model", ok: f.profile === "nw-zx500", value: f.profile, want: "nw-zx500" },
    { id: "firmware", ok: f.booted ? f.vendorFirmware === "4.06" : null, value: f.vendorFirmware, want: "4.06" },
  ];
  const stages: Stage[] = f.booted ? [{ kind: "rebootBootloader" }] : [];
  const flash: Stage[] = set.flash.map((x) => ({ kind: "flash", partition: x.partition, file: x.file, flags: x.flags, bytes: set.files[x.file]?.bytes ?? 0 }));
  if (mode === "clean") {
    if (f.unlocked !== true) {
      gates.push({ id: "oemUnlock", ok: f.booted ? f.oemUnlockAllowed === true : null, value: f.oemUnlockAllowed == null ? null : String(f.oemUnlockAllowed), want: "true" });
      stages.push({ kind: "unlock" });
    }
    gates.push({ id: "battery", ok: f.battery == null ? null : f.battery >= 50, value: f.battery == null ? null : String(f.battery), want: "50" });
    gates.push({ id: "card", ok: f.booted ? !f.card : null, value: null, want: null });
    stages.push(flash[0]!, { kind: "setActive", slot: set.slot }, ...flash.slice(1), { kind: "wipe" });
  } else if (mode === "restore") {
    gates.push({ id: "unlocked", ok: f.unlocked, value: f.unlocked == null ? null : String(f.unlocked), want: "true" });
    gates.push({ id: "battery", ok: f.battery == null ? null : f.battery >= 50, value: f.battery == null ? null : String(f.battery), want: "50" });
    gates.push({ id: "card", ok: f.booted ? !f.card : null, value: null, want: null });
    stages.push(...flash, { kind: "setActive", slot: set.slot }, { kind: "wipe" });
  } else {
    const runsKit = f.booted ? f.running != null && f.unlocked === true && f.slot === set.slot : f.unlocked === false || (f.slot != null && f.slot !== set.slot) ? false : null;
    gates.push({ id: "runsKit", ok: runsKit, value: f.slot, want: set.slot });
    gates.push({ id: "sameVariant", ok: f.booted ? f.running === set.variant : null, value: f.running, want: set.variant });
    gates.push({ id: "battery", ok: f.battery == null ? null : f.battery >= 30, value: f.battery == null ? null : String(f.battery), want: "30" });
    stages.push(...flash);
  }
  const want = mode === "update" ? "writes" : "formats";
  gates.push({ id: "fastboot", ok: f.fastboot === "formats" || f.fastboot === want, value: f.fastboot, want });
  stages.push({ kind: "reboot" }, { kind: "waitBoot" }, { kind: "waitAdb" }, { kind: "verify" });
  if (mode === "update") stages.push({ kind: "dexopt" });
  const known = (ids: Gate["id"][]) => gates.filter((g) => ids.includes(g.id)).every((g) => g.ok === true);
  const ready = gates.every((g) => g.ok !== false) && (mode === "clean" || known(mode === "update" ? ["runsKit", "sameVariant"] : ["unlocked"]));
  return { mode, stages, gates, erases: mode !== "update", confirm: mode === "update" ? `system_${set.slot}` : "userdata", ready };
}

type L<T> = (e: T) => void;
const listeners = new Set<L<InstallJob>>();
let job: InstallJob | null = null;
let timer: number | null = null;

function emit() {
  if (job) for (const l of listeners) l({ ...job, log: [...job.log] });
}

function verdict(set: InstallSet) {
  return {
    slot: set.slot,
    slotOk: true,
    kernelOk: true,
    kernel: set.after.kernel,
    files: set.after.check.map((c) => ({ path: c.path, ok: true, got: c.sha256 })),
    absent: set.after.absent.map((a) => [a, true] as [string, boolean]),
    ok: true,
  };
}

const LENGTH: Partial<Record<Stage["kind"], number>> = { unlock: 5000, wipe: 5000, waitBoot: 4000, waitAdb: 3500 };

function step(set: InstallSet) {
  if (!job || job.state !== "running") return;
  const st = job.stages[job.index]!;
  const ms = st.kind === "flash" ? 900 + st.bytes / 1_500_000 : (LENGTH[st.kind] ?? 1200);
  const t0 = Date.now();
  const tick = () => {
    if (!job || job.state !== "running") return;
    const p = Math.min(1, (Date.now() - t0) / ms);
    job.progress = st.kind === "flash" ? p : null;
    job.seen =
      st.kind === "waitAdb"
        ? p < 0.5 || job.mode === "update" ? null : "unauthorized"
        : st.kind === "reboot" || st.kind === "waitBoot"
          ? "usb"
          : st.kind === "rebootBootloader"
            ? "adb"
            : "fastboot";
    job.waiting = st.kind === "waitAdb";
    if (["flash", "unlock", "wipe", "setActive"].includes(st.kind)) job.touched = true;
    if (p < 1) {
      emit();
      timer = window.setTimeout(tick, 200);
      return;
    }
    job.log.push(st.kind === "flash" ? `Sending '${st.partition}' (${Math.round(st.bytes / 1024)} KB) OKAY` : `${st.kind}: ok`);
    if (job.index + 1 >= job.stages.length) {
      job.state = "done";
      job.verdict = verdict(set);
      job.seen = "adb";
      job.waiting = false;
      emit();
      return;
    }
    job.index += 1;
    job.stageStarted = Date.now();
    emit();
    step(set);
  };
  tick();
}

function driveVerdict(set: StorageSet, stages: Stage[], log: boolean): DriveVerdict {
  return {
    files: stages.flatMap((st) => (st.kind === "copy" ? [{ path: st.file, ok: true, got: set.files[st.file]?.sha256 ?? null }] : [])),
    lines: log ? set.after.expect.map((l) => [l, true] as [string, boolean]) : [],
    upgDone: log ? true : null,
  };
}

const DRIVE_LENGTH: Partial<Record<Stage["kind"], number>> = { readBack: 3000, trigger: 2500, waitUpdate: 6000, checkLog: 900, keep: 300, pickWalkman: 400 };

function driveStep(set: StorageSet) {
  if (!job || job.state !== "running") return;
  const st = job.stages[job.index]!;
  const ms = st.kind === "copy" ? 300 + st.bytes / 1_500_000 : (DRIVE_LENGTH[st.kind] ?? 800);
  const t0 = Date.now();
  const tick = () => {
    if (!job || job.state !== "running") return;
    const p = Math.min(1, (Date.now() - t0) / ms);
    job.progress = st.kind === "copy" || st.kind === "readBack" ? p : null;
    job.seen = st.kind === "waitUpdate" && p < 0.85 ? "gone" : "drive";
    job.waiting = st.kind === "trigger" && p < 0.8;
    if (st.kind === "waitUpdate") job.touched = true;
    if (p < 1) {
      emit();
      timer = window.setTimeout(tick, 200);
      return;
    }
    job.log.push(st.kind === "copy" ? `${st.file}: ${st.bytes} bytes in ${(ms / 1000).toFixed(1)} s` : `${st.kind}: ok`);
    if (st.kind === "readBack") job.drive = driveVerdict(set, job.stages, false);
    if (job.index + 1 >= job.stages.length) {
      job.state = "done";
      job.drive = driveVerdict(set, job.stages, true);
      emit();
      return;
    }
    job.index += 1;
    job.stageStarted = Date.now();
    emit();
    driveStep(set);
  };
  tick();
}

function newDriveJob(build: string, set: StorageSet, mode: InstallMode): InstallJob {
  const p = drivePlan(set, driveFacts(set), mode);
  return {
    build,
    device: A50,
    serial: A50,
    mode,
    variant: "release",
    stages: p.stages,
    index: 0,
    state: "running",
    started: Date.now(),
    stageStarted: Date.now(),
    progress: null,
    seen: "drive",
    waiting: false,
    touched: false,
    error: null,
    verdict: null,
    drive: null,
    log: [],
  };
}

function newJob(build: string, set: InstallSet, mode: InstallMode, at = 0): InstallJob {
  const p = plan(set, facts(), mode);
  return {
    build,
    device: SERIAL,
    serial: SERIAL,
    mode,
    variant: set.variant,
    stages: p.stages,
    index: Math.min(at, p.stages.length - 1),
    state: "running",
    started: Date.now() - 95_000,
    stageStarted: Date.now() - 21_000,
    progress: null,
    seen: "fastboot",
    waiting: false,
    touched: false,
    error: null,
    verdict: null,
    drive: null,
    log: [],
  };
}

const preset = params.get("job");
if (preset && typeof window !== "undefined" && params.get("kit") === "a50") {
  const b = findBuild(A50_BUILD);
  if (b?.storage) {
    const j = newDriveJob(b.id, b.storage, drivePlayer === "installed" ? "update" : "clean");
    j.started = Date.now() - 140_000;
    j.stageStarted = Date.now() - 12_000;
    if (preset === "done" || preset === "failed") {
      j.state = preset;
      j.index = preset === "done" ? j.stages.length - 1 : j.stages.findIndex((s) => s.kind === "checkLog");
      j.touched = true;
      j.drive = { ...driveVerdict(b.storage, j.stages, true), ...(preset === "failed" ? { lines: [[b.storage.after.expect[0]!, false], [b.storage.after.expect[1]!, false]] as [string, boolean][], upgDone: false } : {}) };
      j.error = preset === "failed" ? { kind: "unsupported", message: "the UPG's log does not hold the lines the kit expects", say: "upg-did-not-run" } : null;
      j.log = ["PICKERUPG.log: ====== PICKER UPG START ======", "PICKERUPG.log: mount /emmc@android failed; nothing written"];
    } else {
      j.index = Math.max(0, j.stages.findIndex((s) => s.kind === preset || (s.kind === "copy" && preset === s.file.slice(s.file.lastIndexOf("/") + 1))));
      const st = j.stages[j.index]!;
      j.progress = st.kind === "copy" || st.kind === "readBack" ? 0.46 : null;
      j.seen = st.kind === "waitUpdate" ? "gone" : "drive";
      j.waiting = st.kind === "trigger";
      j.touched = st.kind === "waitUpdate";
    }
    job = j;
  }
} else if (preset && typeof window !== "undefined") {
  const restore = params.get("mode") === "restore";
  const b = findBuild(restore ? "20261021-151702" : "20261020-193012");
  if (b?.set) {
    const j = newJob(b.id, b.set, restore ? "restore" : player === "kit" ? "update" : "clean");
    if (preset === "stuck") {
      j.state = "failed";
      j.index = j.stages.findIndex((s) => s.kind === "flash");
      j.error = { kind: "failed", message: "fastboot failed", say: "failed", vars: { tool: "fastboot" }, detail: "fastboot: error: Failed to find AVB_MAGIC at offset: 0" };
      j.log = ["fastboot flash vbmeta_b vbmeta.img", "fastboot: error: Failed to find AVB_MAGIC at offset: 0"];
    } else if (preset === "done" || preset === "unverified" || preset === "failed") {
      j.state = preset;
      j.index = preset === "failed" ? j.stages.findIndex((s) => s.kind === "waitBoot") : j.stages.length - 1;
      j.verdict = preset === "done" ? verdict(b.set) : null;
      j.seen = preset === "failed" ? "usb" : "adb";
      j.touched = true;
      j.error = preset === "failed" ? { kind: "no-return", message: "the player did not come back after the reboot", say: "no-return" } : null;
      j.log = ["Sending 'system_b' (2927440 KB) OKAY", "Writing 'system_b' OKAY"];
    } else {
      const at = j.stages.findIndex((s) => s.kind === preset || (s.kind === "flash" && preset === s.partition));
      j.index = Math.max(at, 0);
      const st = j.stages[j.index]!;
      j.progress = st.kind === "flash" ? 0.62 : null;
      j.seen = st.kind === "waitAdb" ? null : ["reboot", "waitBoot"].includes(st.kind) ? "usb" : "fastboot";
      j.waiting = st.kind === "waitAdb";
    }
    job = j;
  }
}

const misfit = (set: InstallSet, mode: InstallMode) =>
  (mode === "restore") !== (set.variant === "stock") ? { kind: "mode", message: `a ${set.variant} set does not install as ${mode}`, say: "mode-not-for-set" } : null;

export const installPlan = (build: string, _device: string, mode: InstallMode): Promise<PlanView> => {
  const b = findBuild(build);
  if (b?.storage) {
    if (mode === "restore") return Promise.reject({ kind: "mode", message: "a storage set does not install as Restore", say: "mode-not-for-set" });
    const f = driveFacts(b.storage);
    return Promise.resolve({ plan: drivePlan(b.storage, f, mode), facts: null, drive: f, set: null, storage: b.storage, build: b, stock: null });
  }
  if (!b?.set) return Promise.reject({ kind: "build-not-done", message: "build is not done", say: "build-not-done" });
  const wrong = misfit(b.set, mode);
  if (wrong) return Promise.reject(wrong);
  return Promise.resolve({ plan: plan(b.set, facts(), mode), facts: facts(), drive: null, set: b.set, storage: null, build: b, stock: "/home/listener/.local/share/dev.personnya.walkman-manager/studio/stock/nw-zx500-0000-4.06.00" });
};
export const installState = () => Promise.resolve(job && { ...job });
export const installStart = (build: string, _device: string, mode: InstallMode, confirm: string) => {
  const b = findBuild(build);
  if (b?.storage) {
    const p = drivePlan(b.storage, driveFacts(b.storage), mode);
    if (confirm !== p.confirm) return Promise.reject({ kind: "confirm-mismatch", message: "confirmation word differs", say: "confirm-mismatch" });
    job = newDriveJob(build, b.storage, mode);
    emit();
    driveStep(b.storage);
    return Promise.resolve();
  }
  if (!b?.set) return Promise.reject({ kind: "build-not-done", message: "build is not done", say: "build-not-done" });
  const wrong = misfit(b.set, mode);
  if (wrong) return Promise.reject(wrong);
  const p = plan(b.set, facts(), mode);
  if (confirm !== p.confirm) return Promise.reject({ kind: "confirm-mismatch", message: "confirmation word differs", say: "confirm-mismatch" });
  job = newJob(build, b.set, mode);
  job.started = Date.now();
  job.stageStarted = Date.now();
  job.seen = "adb";
  emit();
  step(b.set);
  return Promise.resolve();
};
export const installCancel = () => {
  if (timer != null) window.clearTimeout(timer);
  if (job) {
    job.state = "cancelled";
    emit();
  }
  return Promise.resolve();
};
export const installSkip = () => {
  if (timer != null) window.clearTimeout(timer);
  if (job) {
    job.state = "unverified";
    job.waiting = false;
    emit();
  }
  return Promise.resolve();
};
export const installClear = () => {
  job = null;
  return Promise.resolve();
};
export const installLeave = installClear;
export const onInstall = (l: L<InstallJob>) => {
  listeners.add(l);
  return Promise.resolve(() => void listeners.delete(l));
};
