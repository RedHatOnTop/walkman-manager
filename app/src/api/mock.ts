// Development only: lets the interface run in a plain browser. Values are taken
// from probe transcripts of the two test units (crates/uwm-core/tests/fixtures).
import type { Bridge } from "./bridge";
import * as install from "./mockInstall";
import * as lib from "./mockLibrary";
import * as studio from "./mockStudio";
import * as tools from "./mockTools";
import type { KnownDevice, Profile, Scan, Snapshot } from "./types";

// Sony Help Guide, "Supported formats"; the same table as profiles/*.toml.
const playback = {
  maxFileBytes: 4294967296,
  codecs: { mp3: 48000, wma: 48000, aac: 48000, flac: 384000, wav: 384000, alac: 384000, aiff: 384000, ape: 192000, dsf: 11289600, dff: 11289600 },
};

const profiles: Profile[] = [
  {
    id: "nw-a50",
    family: "NW-A50 Series",
    art: "a50",
    soc: "MediaTek MT8590",
    dac: "Sony CXD3778GF S-Master HX",
    ramMb: 512,
    bodyMm: [55.7, 97.3, 10.8],
    display: "3.1-inch WVGA 800 × 480",
    massG: 99,
    stockOs: "Walkman OS (Linux 3.10)",
    stockFirmwareLatest: "1.02",
    recovery: ["sony-upg", "mtk-brom"],
    variants: [
      { model: "NW-A55", storageGb: 16 },
      { model: "NW-A56", storageGb: 32 },
      { model: "NW-A57", storageGb: 64 },
    ],
    usb: [],
    playback,
  },
  {
    id: "nw-zx500",
    family: "NW-ZX500 Series",
    art: "zx500",
    soc: "NXP i.MX 8M Mini",
    dac: "Sony CXD3778GF S-Master HX",
    ramMb: 4096,
    bodyMm: [57.9, 122.6, 14.8],
    display: "3.6-inch HD 1280 × 720",
    massG: 164,
    stockOs: "Android 9",
    recovery: ["fastboot"],
    variants: [
      { model: "NW-ZX505", storageGb: 16 },
      { model: "NW-ZX507", storageGb: 64 },
    ],
    usb: [],
    playback,
  },
];

const params = new URLSearchParams(location.search);
const scenario = params.get("scenario") ?? "both";

function scan(): Scan {
  const zx = {
    id: "0000test0zx50700",
    serial: "0000test0zx50700",
    profileId: "nw-zx500",
    family: "NW-ZX500 Series",
    model: "NW-ZX507",
    modelBasis: "storage",
    mode: "running" as const,
    links: ["adb" as const],
    usb: { port: "3-1", vid: 0x054c, pid: 0x0cf2, speed: "480 Mb/s", product: "WALKMAN" },
    adbTarget: "0000test0zx50700",
  };
  const a55 = {
    id: "TESTA55SERIAL01",
    serial: "TESTA55SERIAL01",
    profileId: "nw-a50",
    family: "NW-A50 Series",
    model: "NW-A55",
    modelBasis: "storage",
    mode: "running" as const,
    links: ["adb" as const, "mass-storage" as const],
    usb: { port: "3-2", vid: 0x054c, pid: 0x0bc1, speed: "480 Mb/s", product: "WALKMAN" },
    adbTarget: "TESTA55SERIAL01",
  };
  switch (scenario) {
    case "none":
      return { devices: [], issues: [] };
    case "brom":
      return {
        devices: [
          {
            id: "usb:3-2",
            serial: null,
            profileId: "nw-a50",
            family: "NW-A50 Series",
            model: null,
            modelBasis: null,
            mode: "download",
            links: ["mtk-brom"],
            usb: { port: "3-2", vid: 0x0e8d, pid: 0x0003, speed: "12 Mb/s", product: "MT65xx Preloader" },
            adbTarget: null,
          },
        ],
        issues: [],
      };
    case "unauthorized":
      return { devices: [{ ...zx, mode: "unauthorized" }], issues: [] };
    case "zx":
      return { devices: [zx], issues: [] };
    default:
      return { devices: [zx, a55], issues: [] };
  }
}

const known: KnownDevice[] = [
  { serial: "0000test0zx50700", profileId: "nw-zx500", family: "NW-ZX500 Series", model: "NW-ZX507", modelBasis: "storage" },
  { serial: "TESTA55SERIAL01", profileId: "nw-a50", family: "NW-A50 Series", model: "NW-A55", modelBasis: "storage" },
];

function snapshot(id: string): Snapshot {
  const now = Date.now();
  if (id === "TESTA55SERIAL01") {
    return {
      takenAtMs: now,
      identity: { serial: id, profileId: "nw-a50", family: "NW-A50 Series", model: "NW-A55", modelBasis: "storage" },
      system: {
        platform: "walkman-linux",
        os: "Walkman OS",
        distribution: "Sony firmware",
        firmware: "1.02",
        apiLevel: null,
        buildId: null,
        buildType: null,
        securityPatch: null,
        vendorFirmware: null,
        build: "01.20.E.1.02.00",
        kernel: "3.10.26",
        kernelDate: "Wed Jul 31 16:22:50 JST 2019",
        slot: null,
        bootloaderUnlocked: null,
        verifiedBoot: null,
        container: { root: "/tmp/aroot" },
      },
      power: { level: 81, status: "Charging", voltageMv: 3994, temperatureC: null, health: "Good", externalPower: true },
      memory: { totalKb: 467512, availableKb: null },
      volumes: [
        { kind: "internal", mount: "/tmp/c", filesystem: "vfat", totalBytes: 12855712 * 1024, usedBytes: 5066304 * 1024, freeBytes: 7789408 * 1024 },
      ],
      audio: {
        cards: [{ index: 0, id: "sonysoccard", name: "sony-soc-card", mainOutput: true }],
        streams: [0, 1, 2, 3, 4, 5].map((d) => ({ card: 0, device: d, state: "closed", format: null, rate: null, channels: null, encoding: null })),
        outputs: [],
      },
    };
  }
  const playing = params.get("playing") !== "0";
  return {
    takenAtMs: now,
    identity: { serial: id, profileId: "nw-zx500", family: "NW-ZX500 Series", model: "NW-ZX507", modelBasis: "storage" },
    system: {
      platform: "android",
      os: "Android 13",
      distribution: "TrebleDroid GSI",
      firmware: "treble_arm64_bvS-userdebug 13 TQ3C.230805.001.A4 230905 test-keys",
      apiLevel: 33,
      buildId: "TQ3C.230805.001.A4",
      buildType: "userdebug",
      securityPatch: "2023-08-05",
      vendorFirmware: "4.06",
      build: "google/treble_arm64_bvS/tdgsi_arm64_ab:13/TQ3C.230805.001.A4/230905:userdebug/test-keys",
      kernel: "4.14.78-g69dd21e",
      kernelDate: "Thu Oct 15 08:58:55 UTC 2026",
      slot: "b",
      bootloaderUnlocked: true,
      verifiedBoot: "orange",
      container: null,
    },
    power: { level: 100, status: "Full", voltageMv: 4128, temperatureC: 39.4, health: "Good", externalPower: true },
    memory: { totalKb: 3979552, availableKb: 2573368 },
    volumes: [
      { kind: "internal", mount: "/data", filesystem: "ext4", totalBytes: 52267164 * 1024, usedBytes: 872772 * 1024, freeBytes: 51394392 * 1024 },
      { kind: "card", mount: "/mnt/media_rw/3564-3463", filesystem: "exfat", totalBytes: 124835840 * 1024, usedBytes: 69369600 * 1024, freeBytes: 55466240 * 1024 },
    ],
    audio: {
      cards: [
        { index: 0, id: "imxaudiomicfil", name: "imx-audio-micfil", mainOutput: false },
        { index: 1, id: "imxaudiocxd3778", name: "imx-audio-cxd3778gf", mainOutput: true },
      ],
      streams: [0, 1, 2].map((d) => {
        const running = playing && d === 2;
        return {
          card: 1,
          device: d,
          state: running ? "running" : "closed",
          format: running ? "S32_LE" : null,
          rate: running ? 96000 : null,
          channels: running ? 2 : null,
          encoding: running ? { type: "pcm" as const, bits: 32, rate: 96000 } : null,
        };
      }),
      outputs: [],
    },
  };
}

export function mockBridge(): Bridge {
  return {
    environment: async () => ({ version: "0.1.0-dev", os: "linux", adbPath: "/usr/bin/adb", adbVersion: "Android Debug Bridge version 1.0.41" }),
    profiles: async () => profiles,
    scan: async () => scan(),
    rescan: async () => scan(),
    known: async () => known,
    snapshot: async (id) => {
      await new Promise((r) => setTimeout(r, 220));
      return snapshot(id);
    },
    onDevices: async () => () => {},

    library: lib.libraryState,
    addRoot: lib.libraryState,
    removeRoot: lib.libraryState,
    rescanLibrary: lib.rescanLibrary,
    cancelLibraryScan: async () => {},
    onLibraryScan: lib.onLibraryScan,
    onLibrary: lib.onLibrary,
    artUrl: lib.mockArtUrl,

    deviceMusic: (id) => lib.deviceMusic(id, snapshot(id).volumes),
    syncPlan: (id, volume, tracks) => {
      const snap = snapshot(id);
      const v = snap.volumes.find((x) => x.kind === volume)!;
      return lib.syncPlan(id, v, tracks, playback.codecs, playback.maxFileBytes);
    },
    syncStart: lib.syncStart,
    syncCancel: lib.syncCancel,
    onSyncProgress: lib.onSyncProgress,
    onSyncDone: lib.onSyncDone,

    playerLibrary: lib.playerLibrary,
    pullPlan: lib.pullPlan,
    pullStart: lib.pullStart,
    pullCancel: lib.syncCancel,
    onPullProgress: lib.onPullProgress,
    onPullDone: lib.onPullDone,

    lookupSearch: lib.lookupSearch,
    lookupCover: lib.lookupCover,
    lookupPropose: lib.lookupPropose,
    retagStart: lib.retagStart,
    retagForget: lib.retagForget,
    onRetagProgress: lib.onRetagProgress,
    onRetagDone: lib.onRetagDone,

    sensmeImport: lib.sensmeImport,
    sensmeRead: lib.sensmeReadPlayer,
    onSensmeProgress: lib.onSensmeProgress,
    onSensmeDone: lib.onSensmeDone,

    analysisEngine: lib.analysisEngine,
    analysisPickEngine: lib.analysisPickEngine,
    analysisPlan: lib.analysisPlan,
    analysisStart: lib.analysisStart,
    onAnalysisProgress: lib.onAnalysisProgress,
    onAnalysisDone: lib.onAnalysisDone,

    studioView: studio.studioView,
    studioAdd: studio.studioAdd,
    studioPick: studio.studioPick,
    studioFetch: studio.studioFetch,
    studioRemove: studio.studioRemove,
    studioCancel: studio.studioCancel,
    studioOpenGuide: studio.studioOpenGuide,
    onStudioProgress: studio.onStudioProgress,
    onStudioChanged: studio.onStudioChanged,
    onStudioDone: studio.onStudioDone,
    onFileDrop: studio.onFileDrop,
    studioOpenInputGuide: studio.studioOpenInputGuide,
    studioPickRepo: studio.studioPickRepo,
    studioReadKey: studio.studioReadKey,
    studioHost: studio.studioHost,
    studioLicence: studio.studioLicence,
    studioAgree: studio.studioAgree,
    studioBuild: studio.studioBuild,
    studioBuildCancel: studio.studioBuildCancel,
    studioOpenBuild: studio.studioOpenBuild,
    studioForgetBuild: studio.studioForgetBuild,
    onStudioBuild: studio.onStudioBuild,
    onStudioBuildDone: studio.onStudioBuildDone,

    installPlan: install.installPlan,
    installState: install.installState,
    installStart: install.installStart,
    installCancel: install.installCancel,
    installSkip: install.installSkip,
    installClear: install.installClear,
    installLeave: install.installLeave,
    onInstall: install.onInstall,

    toolsView: tools.toolsView,
    toolsLicence: tools.toolsLicence,
    toolsAgree: tools.toolsAgree,
    toolsFetch: tools.toolsFetch,
    toolsCancel: tools.toolsCancel,
    onToolsProgress: tools.onToolsProgress,
    onToolsDone: tools.onToolsDone,
  };
}
