// Development only: the Firmware Studio in a plain browser. The known files mirror
// kits/files.toml; `?studio=empty|some|ready` picks what the vault starts with, and
// `?building` starts with a build halfway through.
import type { Lang } from "../i18n/strings";
import type {
  BuildDone,
  BuildOptions,
  BuildProgress,
  BuildRecord,
  FormatKind,
  HostCheck,
  InstallSet,
  KitInput,
  KnownFile,
  LicenceText,
  RepoState,
  StudioDone,
  StudioProgress,
  StorageSet,
  StudioView,
  VaultFile,
} from "./types";

const GUIDE = { en: "https://github.com/RedHatOnTop/walkman-manager/blob/main/docs/guides/firmware-files.md", ko: "https://github.com/RedHatOnTop/walkman-manager/blob/main/docs/guides/firmware-files.ko.md" };

const known: Record<string, KnownFile> = {
  tdXz: {
    id: "td-ci-20230905-arm64-ab-vanilla-xz",
    title: { en: "TrebleDroid ci-20230905, arm64 A/B vanilla", ko: "TrebleDroid ci-20230905, arm64 A/B vanilla" },
    format: "xz",
    bytes: 641469308,
    sha256: "e3bfcc02fd5cfdf358c6d4792d746f8cce49899382ba7ece1df7e3fe72de154e",
    url: "https://github.com/TrebleDroid/treble_experimentations/releases/download/ci-20230905/system-td-arm64-ab-vanilla.img.xz",
  },
  tdImg: {
    id: "td-ci-20230905-arm64-ab-vanilla",
    title: { en: "TrebleDroid ci-20230905, arm64 A/B vanilla (unpacked)", ko: "TrebleDroid ci-20230905, arm64 A/B vanilla (압축 푼 이미지)" },
    format: "ext4",
    bytes: 1931804672,
    sha256: "55541b9b263a35f5fa4bb9abd0a626c5b89139bb9a4454bb70ee95cf9246e7a5",
    from: "td-ci-20230905-arm64-ab-vanilla-xz",
  },
  zx406: {
    id: "nw-zx500-0000-4.06.00",
    title: { en: "NW-ZX500 4.06.00, series 0000", ko: "NW-ZX500 4.06.00, 시리즈 0000" },
    format: "sonyNwwm",
    bytes: 907457808,
    sha256: "1249a9baeda969b205d1db9fe335a133a131f4628d9d97e9120ea02a5be19dac",
    guide: { en: "sony-firmware-updates", ko: "sony-펌웨어-업데이트" },
  },
  kernel: {
    id: "zx507-kernel-4.14.78-1",
    title: { en: "NW-ZX500 kernel 4.14.78, walkman-zx507 build 1", ko: "NW-ZX500 커널 4.14.78, walkman-zx507 빌드 1" },
    format: "kernelImage",
    bytes: 28940800,
    sha256: "19c3af6fc902f74a47b73b7320821ba189021362d080e07dbe1e965b12afb336",
    url: "https://github.com/RedHatOnTop/walkman-zx507/releases/download/kernel-4.14.78-zx507-1/Image-zx507-4.14.78-1",
  },
  gpl: {
    id: "nw-zx500-gpl-4.06",
    title: { en: "NW-ZX500 GPL source", ko: "NW-ZX500 GPL 소스" },
    format: "gzip",
    bytes: 304730290,
    sha256: "cd8942393d154c3ff72dd0c85cd32b7b0a3c60e20afd89e7b40a7d2454319d3f",
    md5: "4ea368efdb62d167483d89e6ec60e2c3",
    url: "https://prodgpl.blob.core.windows.net/download/Audio/20211022/gpl_source.tgz",
  },
  ford: {
    id: "lineage-12.1-20200505-ford",
    title: { en: "LineageOS 12.1 for the Fire 7 (ford), 2020-05-05", ko: "Fire 7(ford)용 LineageOS 12.1, 2020-05-05" },
    format: "otaBlocks",
    md5: "5fa7b3fdc29ae0693afe80f439c69631",
    guide: { en: "lineageos-for-the-fire-7", ko: "fire-7용-lineageos" },
  },
  fire: {
    id: "fireos-5.3.6.4-ford",
    title: { en: "Fire OS 5.3.6.4 for the Fire 7 (ford)", ko: "Fire 7(ford)용 Fire OS 5.3.6.4" },
    format: "otaBlocks",
    bytes: 664431377,
    sha256: "4f719fa75e786283916505154cf9a8b9c8f96734421f2c3c7e7e4fbfe1ce9f2f",
    guide: { en: "fire-os-for-the-fire-7", ko: "fire-7용-fire-os" },
  },
};

const RELEASE = "4.14.78-g69dd21e";

type InputSpec = { id: string; accepts: KnownFile[]; optional: boolean; formats?: { kind: FormatKind; release?: string }[]; guided?: boolean };
const kitInputs: Record<string, InputSpec[]> = {
  "nw-zx500-android13": [
    { id: "gsi", accepts: [known.tdXz!, known.tdImg!], optional: false },
    { id: "stock", accepts: [known.zx406!], optional: false },
    { id: "kernel", accepts: [known.kernel!], optional: false, formats: [{ kind: "kernelImage", release: RELEASE }], guided: true },
    { id: "gpl", accepts: [known.gpl!], optional: true },
  ],
  "nw-a50-android5": [
    { id: "backup", accepts: [], optional: false, formats: [{ kind: "wmBackup" }], guided: true },
    { id: "rom", accepts: [known.ford!], optional: false },
    { id: "fireos", accepts: [known.fire!], optional: false },
  ],
};
const kitProfiles: Record<string, string> = { "nw-zx500-android13": "nw-zx500", "nw-a50-android5": "nw-a50" };
const buildable = new Set(["nw-zx500-android13", "nw-a50-android5"]);
// Kits whose entry point makes one kind of build, copied onto the player's drive.
const storageKits = new Set(["nw-a50-android5"]);

const HOME = "/home/listener";
let seq = 0;
const now = 1_760_900_000;

function entry(k: KnownFile | null, path: string, bytes: number, extra: Partial<VaultFile> = {}): VaultFile {
  seq += 1;
  return {
    id: `v${seq.toString(16).padStart(15, "0")}`,
    added: now - seq * 3600,
    path,
    bytes,
    modified: now - 86400 * 30,
    format:
      k?.format === "sonyNwwm"
        ? { kind: "sonyNwwm", payloadBytes: bytes - 137 }
        : k?.format === "otaBlocks"
          ? { kind: "otaBlocks", device: "ford" }
          : k?.format === "ext4"
            ? { kind: "ext4", fsBytes: bytes, label: "/" }
            : { kind: (k?.format as "xz" | "gzip") ?? "unknown" },
    sha256: k?.sha256 ?? "9f2c4e1d0b7a65c3d8e2f1a0b9c8d7e6f5a4b3c2d1e0f9a8b7c6d5e4f3a2b1c0",
    md5: k?.md5 ?? "0c2b4e6d8f0a1c3e5b7d9f1a3c5e7b9d",
    known: k?.id ?? null,
    damage: null,
    presence: "here",
    ...extra,
  };
}

const scenario = typeof location === "undefined" ? "some" : (new URLSearchParams(location.search).get("studio") ?? "some");
let files: VaultFile[] = [];
if (scenario !== "empty") {
  files.push(entry(known.tdXz!, `${HOME}/Downloads/system-td-arm64-ab-vanilla.img.xz`, 641469308));
  files.push(
    entry(null, `${HOME}/Downloads/NW-A100_0000_V4_06_00_NW_WM_FW.UPG`, 906889168, {
      format: { kind: "sonyNwwm", payloadBytes: 906889030 },
      known: "nw-a100-0000-4.06.00",
    }),
  );
  files.push(
    entry(null, `${HOME}/Downloads/NW-ZX500_0000_V4_06_00_NW_WM_FW (1).UPG`, 512000000, {
      format: { kind: "sonyNwwm", payloadBytes: 907457670 },
      damage: { kind: "truncated", expected: 907457798, actual: 512000000 },
    }),
  );
}
if (scenario === "ready") {
  files.push(entry(known.zx406!, `${HOME}/Walkman/NW-ZX500_0000_V4_06_00_NW_WM_FW.UPG`, 907457808));
  files.push(entry(known.fire!, `${HOME}/Walkman/update-ford-5.3.6.4.bin`, 664431377));
  files.push(entry(null, `${HOME}/src/walkman-zx507/kernel-out/Image`, 27_634_176, { format: { kind: "kernelImage", release: RELEASE } }));
  files.push(entry(null, `${HOME}/Walkman/WMBACKUP/manifest.txt`, 1751, { format: { kind: "wmBackup", images: 27 } }));
}

// What a finished build of the kit produced: the shape of its install.json.
export const mockSet = (variant: BuildOptions["variant"], root: boolean): InstallSet => ({
  kit: "nw-zx500-android13",
  commit: "da877795c709adf873f867f80e072075369de945",
  dirty: false,
  variant,
  root,
  model: "NW-ZX500",
  firmware: "4.06.00",
  series: "0000",
  slot: "b",
  files: {
    "system.img": { bytes: 2997698560, sha256: "0449aa53fa7446c99ea3b63cb3593ecd6a017238538f2e3b2efaebde38064045" },
    "boot.img": { bytes: 50331648, sha256: "3dcfedff6ee958f7a90f9801460a1f7ad6150b2f85056017e5b67ad1c8460735" },
    "vbmeta.img": { bytes: 4096, sha256: "3669ca5c0dc2aa28789dcfe6910536d47b15a27ffb1f7197e3b03c8362c45102" },
  },
  flash: [
    { partition: "vbmeta_b", file: "vbmeta.img", flags: ["--disable-verity", "--disable-verification"] },
    { partition: "boot_b", file: "boot.img", flags: [] },
    { partition: "system_b", file: "system.img", flags: [] },
  ],
  after: {
    kernel: `Linux version ${RELEASE} (builder@host)`,
    check: [
      { path: "/system/framework/services.jar", sha256: "5a793adc14edb3871a951fefeba785380f06654ef226fc9641ccd9dd76103fcb" },
      { path: "/system/framework/framework-res.apk", sha256: "a3abe037b8973c2fa85b2283ca563b04eb6102224e9f738f22ed7b5e892abdcd" },
      { path: "/system/system_ext/priv-app/SystemUI/SystemUI.apk", sha256: "48719ce884a135cd26e7acd70e44ef4cfdf8c020f9e1a19bd7489ee14082dc75" },
      { path: "/system/system_ext/etc/build.prop", sha256: "030aea4178a174675cb66c0e693602472b75d67f31d55fc3aedeb828758eee6b" },
    ],
    absent: root ? [] : ["/system/xbin/su"],
  },
});

// make-restore.sh's set: Sony's four partitions as the UPG has them.
const stockSet = (): InstallSet => ({
  kit: "nw-zx500-android13",
  commit: "62d4c1c6893ae37ceedf06794ad687d37a27752c",
  dirty: false,
  variant: "stock",
  root: false,
  model: "NW-ZX500",
  firmware: "4.06.00",
  series: "0000",
  slot: "b",
  files: {
    "vbmeta.img": { bytes: 4096, sha256: "3669ca5c0dc2aa28789dcfe6910536d47b15a27ffb1f7197e3b03c8362c45102" },
    "boot.img": { bytes: 50331648, sha256: "922d1e878ca6fd7ff61f99ca251fab0c944ba0b602e64c54c3dc84ecdef57ca2" },
    "system.img": { bytes: 3145728000, sha256: "2a3db28862e38e60f77119dd36e60654e652e55ccaa138933b44b79039f813f0" },
    "vendor.img": { bytes: 209715200, sha256: "be305cf1e263c1624f00ef757644c2aaa7e1341748978964760a10d788fb8c6d" },
  },
  flash: [
    { partition: "vbmeta_b", file: "vbmeta.img", flags: [] },
    { partition: "boot_b", file: "boot.img", flags: [] },
    { partition: "system_b", file: "system.img", flags: [] },
    { partition: "vendor_b", file: "vendor.img", flags: [] },
  ],
  after: {
    kernel: `Linux version ${RELEASE} (slave@99f7f681bda5)`,
    check: [
      { path: "/system/framework/services.jar", sha256: "1c16f5976318fadcf7add92c518a93896f99e2a78f742b7347d8539103264882" },
      { path: "/system/framework/framework-res.apk", sha256: "b1517b696cf4f9ae21cad6e106ebb59e4655e91367bad36641c8053d1028b01c" },
      { path: "/system/priv-app/SystemUI/SystemUI.apk", sha256: "f925718a593e630ab33529b329da4a129afb923939abab381c36b27d71ac7409" },
    ],
    absent: ["/system/etc/init/zx507-audio.rc"],
  },
});

// make-install.sh's steps, as its "== <i> <step>" lines name them.
const STEPS = ["tools", "stock", "gsi", "prepare-gsi", "add-sony-apps", "debloat-gsi", "compat-gsi", "wmusic-audio", "izm-audio", "theme-gsi", "gpu-gsi", "wifi-only-gsi", "lean-ui-gsi", "tag-charset-gsi", "microg-gsi", "seamless-scale-gsi", "identity-gsi", "wallpaper-gsi", "firefox-gsi", "qstiles-gsi", "drop-trebleapp-gsi", "release-gsi", "odex-gsi", "boot", "manifest", "install"];
const STOCK_STEPS = ["stock", "install"];
const BUILDS = `${HOME}/.local/share/dev.personnya.walkman-manager/studio/builds`;

/** The NW-A50 kit's files, by path and size as a real build wrote them. */
const A50_FILES: [string, number][] = [
  ["wmmod-android/boot/android-boot.sh", 41686],
  ["wmmod-android/boot/container.tgz", 351758],
  ["wmmod-android/boot/drop-sys-boot", 0],
  ["wmmod-android/boot/idc-overlay/himax-hx8526-icx.idc", 232],
  ["wmmod-android/boot/kl-overlay/icx_key.kl", 481],
  ["wmmod-android/boot/mali_r4p0.ko", 576764],
  ["wmmod-android/boot/picker/bootpick", 626708],
  ["wmmod-android/boot/picker/frames.raw", 6144076],
  ["wmmod-android/boot/pivotinit", 621896],
  ["wmmod-android/boot/swap_accounting_guard.ko", 9948],
  ["wmmod-android/boot/wifi/cfg80211_bp.ko", 657056],
  ["wmmod-android/boot/wifi/compat_bp.ko", 19836],
  ["wmmod-android/boot/wifi/mtk_wmt_wifi_soc.ko", 12364],
  ["wmmod-android/boot/wifi/wlan_mt_bp.ko", 1311592],
  ["wmmod-android/boot/MANIFEST", 637],
  ["wmmod-android/fire-system.img", 724381696],
  ["wmmod-android/android-data.img", 2097152000],
  ["NW_WM_FW.UPG", 12368],
];

export const mockStorage = (): StorageSet => ({
  kit: "nw-a50-android5",
  commit: "d4f0675a1c9e2b7f30e8a4d6b5c1f9e7a2d3c4b5",
  dirty: false,
  firmware: "bbdmp2-20190731",
  files: Object.fromEntries(A50_FILES.map(([p, bytes], i) => [p, { bytes, sha256: (i.toString(16).padStart(2, "0") + "7c4e19a0").repeat(8) }])),
  copy: A50_FILES.map(([p]) => p),
  keep: ["wmmod-android/android-data.img"],
  upg: { file: "NW_WM_FW.UPG", key: "w1", recovery: "1093dbee3723ad6f145d3ffa28b76679b5ac8cd957bf891c070eafdc3690c7e2" },
  after: { log: "PICKERUPG.log", expect: ["hook md5 9e86b9772b897b2c467685f427c0d474", "====== PICKER UPG DONE ======"] },
});

function record(id: string, started: number, options: BuildOptions, outcome: BuildRecord["outcome"], secs: number, kit = "nw-zx500-android13"): BuildRecord {
  const done = outcome.kind === "done";
  const storage = storageKits.has(kit);
  return {
    id,
    kit,
    commit: storage ? "d4f0675a1c9e2b7f30e8a4d6b5c1f9e7a2d3c4b5" : "da877795c709adf873f867f80e072075369de945",
    started,
    ended: started + secs,
    options,
    inputs: storage ? [{ input: "backup", path: `${HOME}/WMBACKUP/manifest.txt`, sha256: "ab".repeat(32) }] : [],
    outcome,
    total: storage ? 24 : options.variant === "stock" ? STOCK_STEPS.length : STEPS.length,
    steps: [],
    set: done && !storage ? (options.variant === "stock" ? stockSet() : mockSet(options.variant, options.root)) : null,
    ...(done && storage ? { storage: mockStorage() } : {}),
    dir: `${BUILDS}/${id}`,
  };
}

let repos: Record<string, RepoState> = { "nw-zx500-android13": { state: "published", have: scenario === "ready", bytes: 354757 } };
let keys: Record<string, boolean> = { "nw-zx500": scenario === "ready" };
let licences: Record<string, boolean> = { "nw-zx500-android13": scenario === "ready" };

/** The opening of Google's text and its date; the app reads all of it from Google. */
const SDK_LICENCE = "Terms and Conditions\n\nThis is the Android Software Development Kit License Agreement\n\n1. Introduction\n\n1.1 The Android Software Development Kit (referred to in the License Agreement as the \"SDK\" and specifically including the Android system files, packaged APIs, and Google APIs add-ons) is licensed to you subject to the terms of the License Agreement. The License Agreement forms a legally binding contract between you and Google in relation to your use of the SDK.\n\n1.2 \"Android\" means the Android software stack for devices, as made available under the Android Open Source Project, which is located at the following URL: http://source.android.com/, as updated from time to time.\n\n1.3 A \"compatible implementation\" means any Android device that (i) complies with the Android Compatibility Definition document, which can be found at the Android compatibility website (http://source.android.com/compatibility) and which may be updated from time to time; and (ii) successfully passes the Android Compatibility Test Suite (CTS).\n\n1.4 \"Google\" means Google Inc., a Delaware corporation with principal place of business at 1600 Amphitheatre Parkway, Mountain View, CA 94043, United States.\n\n2. Accepting the License Agreement\n\n2.1 In order to use the SDK, you must first agree to the License Agreement. You may not use the SDK if you do not accept the License Agreement.\n\n2.2 By clicking to accept, you hereby agree to the terms of the License Agreement.\n\n2.3 You may not use the SDK and may not accept the License Agreement if you are a person barred from receiving the SDK under the laws of the United States or other countries, including the country in which you are resident or from which you use the SDK.\n\n2.4 If you are agreeing to be bound by the License Agreement on behalf of your employer or other entity, you represent and warrant that you have full legal authority to bind your employer or such entity to the License Agreement. If you do not have the requisite authority, you may not accept the License Agreement or use the SDK on behalf of your employer or other entity.\n\nJanuary 16, 2019";
let builds: BuildRecord[] =
  scenario === "empty"
    ? []
    : [
        ...(typeof location !== "undefined" && new URLSearchParams(location.search).get("mode") === "restore" ? [record("20261021-151702", now - 600, { variant: "stock", root: false }, { kind: "done" }, 4)] : []),
        record("2026-10-22-154109", now - 1800, { variant: "release", root: false }, { kind: "done" }, 15, "nw-a50-android5"),
        record("20261020-193012", now - 3600, { variant: "release", root: false }, { kind: "done" }, 412),
        record("20261020-181544", now - 8200, { variant: "debug", root: false }, { kind: "failed", step: "odex-gsi", tail: ["dex2oat: services.jar: 7 failures", "odex-gsi.sh: exit 1"], code: 1 }, 288),
      ];
let building: BuildProgress | null = null;

function view(): StudioView {
  const usable = files.filter((f) => f.presence === "here" && !f.damage);
  const kits = Object.entries(kitInputs).map(([id, inputs]) => {
    const filled: KitInput[] = inputs.map(({ formats = [], guided = false, ...i }) => ({
      ...i,
      formats,
      guided,
      filledBy:
        usable.find(
          (f) =>
            (f.known && i.accepts.some((a) => a.id === f.known)) ||
            formats.some((p) => p.kind === f.format.kind && (!p.release || ("release" in f.format && f.format.release === p.release))),
        )?.id ?? null,
    }));
    const restore = buildable.has(id) && !storageKits.has(id) ? filled.some((i) => i.id === "stock" && i.filledBy) : null;
    return {
      id,
      profile: kitProfiles[id]!,
      inputs: filled,
      ready: filled.every((i) => i.optional || i.filledBy),
      buildable: buildable.has(id),
      restore,
      variants: !storageKits.has(id),
      set: storageKits.has(id) ? ("storage" as const) : ("fastboot" as const),
    };
  });
  const a100: KnownFile = { id: "nw-a100-0000-4.06.00", title: { en: "NW-A100 4.06.00, series 0000", ko: "NW-A100 4.06.00, 시리즈 0000" }, format: "sonyNwwm", bytes: 906889168 };
  return {
    kits,
    files: files.map((f) => ({ ...f })),
    catalog: [...Object.values(known), a100],
    downloads: `${HOME}/.local/share/dev.personnya.walkman-manager/studio/files`,
    repos: { ...repos },
    keys: { ...keys },
    licences: { ...licences },
    builds: [...builds],
    building,
  };
}

type L<T> = (e: T) => void;
const progressListeners = new Set<L<StudioProgress>>();
const changedListeners = new Set<() => void>();
const doneListeners = new Set<L<StudioDone>>();
let busy = false;
let cancelled = false;

function sub<T>(set: Set<T>, l: T) {
  set.add(l);
  return Promise.resolve(() => void set.delete(l));
}

function job(steps: { name: string; total: number; fetching: boolean; then: () => VaultFile | null }[]) {
  if (busy) return Promise.reject({ kind: "busy", message: "the Studio is still looking at other files", say: "studio-busy" });
  busy = true;
  cancelled = false;
  const added: string[] = [];
  let i = 0;
  let done = 0;
  const tick = () => {
    const s = steps[i];
    if (cancelled || !s) {
      busy = false;
      for (const l of doneListeners) l({ added, failed: [], cancelled });
      return;
    }
    done = Math.min(s.total, done + s.total / 14);
    for (const l of progressListeners) l({ name: s.name, fetching: s.fetching, done, total: s.total, index: i, count: steps.length });
    if (done >= s.total) {
      const e = s.then();
      if (e) {
        files = files.filter((f) => f.path !== e.path).concat(e);
        added.push(e.id);
        for (const l of changedListeners) l();
      }
      i += 1;
      done = 0;
    }
    setTimeout(tick, 110);
  };
  tick();
  return Promise.resolve();
}

function guess(path: string): VaultFile {
  const k = Object.values(known).find((x) => x.url && path.endsWith(x.url.split("/").pop()!)) ?? null;
  return entry(k, path, k?.bytes ?? 48_000_000);
}

export const studioView = () => Promise.resolve(view());
export const studioAdd = (paths: string[]) =>
  job(paths.map((p) => ({ name: p, total: guess(p).bytes, fetching: false, then: () => guess(p) })));
export const studioPick = () =>
  job([
    {
      name: `${HOME}/Downloads/update-ford-5.3.6.4.bin`,
      total: 664431377,
      fetching: false,
      then: () => entry(known.fire!, `${HOME}/Downloads/update-ford-5.3.6.4.bin`, 664431377),
    },
  ]).then(() => true);
export const studioFetch = (id: string) => {
  const k = Object.values(known).find((x) => x.id === id)!;
  const path = `${view().downloads}/${k.url!.split("/").pop()}`;
  return job([
    { name: k.id, total: k.bytes ?? 1, fetching: true, then: () => null },
    { name: path, total: k.bytes ?? 1, fetching: false, then: () => entry(k, path, k.bytes ?? 1) },
  ]);
};
export const studioRemove = (id: string) => {
  files = files.filter((f) => f.id !== id);
  return Promise.resolve(view());
};
export const studioCancel = () => {
  cancelled = true;
  return Promise.resolve();
};
export const studioOpenGuide = (id: string, lang: Lang) => {
  const k = Object.values(known).find((x) => x.id === id);
  if (k?.guide) window.open(`${GUIDE[lang]}#${k.guide[lang]}`, "_blank", "noopener");
  return Promise.resolve();
};
export const onStudioProgress = (l: L<StudioProgress>) => sub(progressListeners, l);
export const onStudioChanged = (l: () => void) => sub(changedListeners, l);
export const onStudioDone = (l: L<StudioDone>) => sub(doneListeners, l);
export const onFileDrop = () => Promise.resolve(() => {});

const buildListeners = new Set<L<BuildProgress | null>>();
const buildDoneListeners = new Set<L<BuildDone>>();
let buildTimer: number | null = null;

function emitBuild() {
  for (const l of buildListeners) l(building && { ...building, lines: [...building.lines] });
}

function runBuild(kit: string, options: BuildOptions, from = 0) {
  const steps = options.variant === "stock" ? [...STOCK_STEPS] : STEPS.filter((s) => s !== "release-gsi" || options.variant === "release");
  if (options.variant === "debug") steps.splice(4, 0, "debug-gsi");
  const started = Math.floor(Date.now() / 1000) - from * 9;
  const repo = repos[kit];
  const fetching = !from && repo?.state === "published" && !repo.have;
  const phase = from ? "run" : fetching ? "fetch" : "copy";
  const fetchTotal = repo?.state === "published" ? repo.bytes : 0;
  building = { id: null, kit, options, started, phase, fetched: 0, fetchTotal, index: from, total: steps.length, step: from ? steps[from - 1]! : null, stepStarted: Date.now(), lines: [] };
  if (fetching) repos = { ...repos, [kit]: { ...repo, have: true } };
  emitBuild();
  let i = from;
  const tick = () => {
    if (!building) return;
    if (i >= steps.length) {
      const id = new Date().toISOString().slice(0, 19).replace(/[-:]/g, "").replace("T", "-");
      const r = record(id, started, options, { kind: "done" }, Math.floor(Date.now() / 1000) - started, kit);
      builds = [r, ...builds];
      building = null;
      emitBuild();
      for (const l of buildDoneListeners) l({ record: r, error: null });
      return;
    }
    const name = steps[i]!;
    i += 1;
    building = {
      ...building,
      id: building.id ?? "pending",
      phase: "run",
      index: i,
      step: name,
      stepStarted: Date.now(),
      lines: [...building.lines, `== ${i} ${name}`, `${name}: ok`].slice(-40),
    };
    emitBuild();
    buildTimer = window.setTimeout(tick, name === "odex-gsi" || name === "stock" ? 2600 : 900);
  };
  buildTimer = window.setTimeout(tick, from ? 900 : 1200);
}

if (typeof location !== "undefined" && new URLSearchParams(location.search).has("building")) runBuild("nw-zx500-android13", { variant: "release", root: false }, 11);

const hostChecks: HostCheck[] = [
  ...["bash", "tar", "debugfs", "dumpe2fs", "e2fsck", "resize2fs", "patchelf", "python3", "java", "curl", "unzip", "zip", "xz", "ar", "openssl"].map((name) => ({ kind: "tool" as const, name, ok: true, found: `/usr/bin/${name}` })),
  { kind: "java", name: "17", ok: true, found: "21.0.8" },
  { kind: "space", name: "16", ok: true, found: String(212 * 1024 ** 3) },
  { kind: "adbKey", name: "adbkey.pub", ok: true, found: `${HOME}/.android/adbkey.pub` },
];

export const studioOpenInputGuide = (kit: string, input: string, lang: Lang) => {
  const spec = kitInputs[kit]?.find((i) => i.id === input);
  if (spec?.guided) window.open(`${GUIDE[lang]}#${lang === "ko" ? "nw-zx500용-커널" : "a-kernel-for-the-nw-zx500"}`, "_blank", "noopener");
  return Promise.resolve();
};
export const studioPickRepo = (kit: string) => {
  repos = { ...repos, [kit]: { state: "ready", path: `${HOME}/src/walkman-zx507` } };
  return Promise.resolve(view());
};
export const studioReadKey = () => {
  keys = { ...keys, "nw-zx500": true };
  return new Promise<StudioView>((r) => setTimeout(() => r(view()), 600));
};
export const studioHost = () => Promise.resolve({ checks: hostChecks, ok: true });
export const studioLicence = () =>
  new Promise<LicenceText>((r) => setTimeout(() => r({ id: "android-sdk-license", text: SDK_LICENCE, sha256: "mock-sdk-licence" }), 500));
export const studioAgree = (kit: string) => {
  licences = { ...licences, [kit]: true };
  return Promise.resolve(view());
};
export const studioBuild = (kit: string, options: BuildOptions) => {
  if (building) return Promise.reject({ kind: "busy", message: "a build is running", say: "studio-busy" });
  runBuild(kit, options);
  return Promise.resolve();
};
export const studioBuildCancel = () => {
  if (buildTimer != null) window.clearTimeout(buildTimer);
  if (building) {
    const r = record(new Date().toISOString().slice(0, 19).replace(/[-:]/g, "").replace("T", "-"), building.started, building.options, { kind: "cancelled" }, Math.floor(Date.now() / 1000) - building.started);
    builds = [r, ...builds];
    building = null;
    emitBuild();
    for (const l of buildDoneListeners) l({ record: r, error: null });
  }
  return Promise.resolve();
};
export const studioOpenBuild = () => Promise.resolve();
export const studioForgetBuild = (id: string) => {
  builds = builds.filter((b) => b.id !== id);
  return Promise.resolve(view());
};
export const onStudioBuild = (l: L<BuildProgress | null>) => sub(buildListeners, l);
export const onStudioBuildDone = (l: L<BuildDone>) => sub(buildDoneListeners, l);
export const findBuild = (id: string) => builds.find((b) => b.id === id) ?? null;
