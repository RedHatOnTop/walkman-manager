// Development only: Google's platform-tools in a plain browser. `?fastboot=missing|writes`
// sets what the computer's own fastboot does before the download; `?adb=none` leaves the
// computer without adb.
import { studioLicence } from "./mockStudio";
import type { ErrorReport, Flasher, ToolsDone, ToolsView } from "./types";

const params = typeof location === "undefined" ? new URLSearchParams() : new URLSearchParams(location.search);
const own = (params.get("fastboot") ?? "formats") as Flasher;
const ownAdb = params.get("adb") !== "none";
const BYTES = 9_054_187;
const DIR = "/home/listener/.local/share/dev.personnya.walkman-manager/platform-tools";

type L<T> = (e: T) => void;
const progressListeners = new Set<L<{ done: number; total: number }>>();
const doneListeners = new Set<L<ToolsDone>>();
let installed: string | null = null;
let agreed = false;
let fetching: { done: number; total: number } | null = null;
let timer: number | null = null;

export const flasher = (): Flasher => (installed ? "formats" : own);

function view(): ToolsView {
  return {
    installed,
    revision: "37.0.1",
    bytes: BYTES,
    agreed,
    fetching: fetching && { ...fetching },
    adb: ownAdb ? "/usr/bin/adb" : installed ? `${DIR}/adb` : null,
    fastboot: installed ? `${DIR}/fastboot` : own === "missing" ? null : "/usr/bin/fastboot",
    flasher: flasher(),
  };
}

function sub<T>(set: Set<T>, l: T) {
  set.add(l);
  return Promise.resolve(() => void set.delete(l));
}

const refused = (say: string, message: string): ErrorReport => ({ kind: "refused", message, say });

export const toolsView = () => Promise.resolve(view());
export const toolsLicence = studioLicence;
export const toolsAgree = () => {
  agreed = true;
  return Promise.resolve(view());
};
export const toolsFetch = () => {
  if (!agreed) return Promise.reject(refused("licence-not-agreed", "android-sdk-license is not agreed to"));
  if (fetching) return Promise.reject(refused("tools-busy", "the platform-tools are downloading"));
  const now = { done: 0, total: BYTES };
  fetching = now;
  timer = window.setInterval(() => {
    now.done = Math.min(BYTES, now.done + 420_000);
    for (const l of progressListeners) l({ ...now });
    if (now.done < BYTES) return;
    if (timer != null) window.clearInterval(timer);
    fetching = null;
    installed = "37.0.1";
    for (const l of doneListeners) l({ view: view(), error: null, cancelled: false });
  }, 120);
  return Promise.resolve();
};
export const toolsCancel = () => {
  if (timer != null) window.clearInterval(timer);
  if (fetching) {
    fetching = null;
    for (const l of doneListeners) l({ view: view(), error: null, cancelled: true });
  }
  return Promise.resolve();
};
export const onToolsProgress = (l: L<{ done: number; total: number }>) => sub(progressListeners, l);
export const onToolsDone = (l: L<ToolsDone>) => sub(doneListeners, l);
