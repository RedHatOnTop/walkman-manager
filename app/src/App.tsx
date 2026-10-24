import { useCallback, useEffect, useState } from "react";
import { Icon } from "./components/Icon";
import { HOME, sameRoute, type Route } from "./routes";
import { useData } from "./state/data";
import { DevicePage } from "./views/device/DevicePage";
import { Home } from "./views/Home";
import { LibraryPage } from "./views/library/LibraryPage";
import { InstallPage } from "./views/install/InstallPage";
import { StudioPage } from "./views/studio/StudioPage";
import { Settings } from "./views/Settings";
import { Sidebar } from "./views/Sidebar";
import { ErrorText } from "./components/ErrorText";

function initialRoute(): Route {
  const p = new URLSearchParams(location.search);
  const device = p.get("device");
  if (device) return { kind: "device", id: device };
  if (p.get("page") === "settings") return { kind: "settings" };
  if (p.get("page") === "studio") return { kind: "studio" };
  const build = p.get("install");
  if (build) return { kind: "install", build };
  if (p.get("page") === "library") {
    return { kind: "library", source: p.get("source") ?? undefined, view: p.get("view") === "sensme" ? "sensme" : undefined };
  }
  return HOME;
}

export function App() {
  const { ready, fatal, scan } = useData();
  const [route, setRoute] = useState<Route>(initialRoute);
  const go = useCallback((r: Route) => setRoute((cur) => (sameRoute(cur, r) ? cur : r)), []);

  // A single player opens straight to its page the first time it appears.
  const [autoOpened, setAutoOpened] = useState(false);
  useEffect(() => {
    if (!ready || autoOpened) return;
    setAutoOpened(true);
    const only = scan.devices.length === 1 ? scan.devices[0] : undefined;
    if (only && route.kind === "home") setRoute({ kind: "device", id: only.id });
  }, [ready, autoOpened, scan.devices, route.kind]);

  if (fatal) {
    return (
      <div className="fatal">
        <Icon name="alert" size={22} />
        <p className="selectable">
          <ErrorText error={fatal} />
        </p>
      </div>
    );
  }

  return (
    <div className="shell">
      <Sidebar route={route} go={go} />
      <main className="main" key={route.kind === "device" ? `d:${route.id}` : route.kind}>
        {!ready ? null : route.kind === "home" ? (
          <Home go={go} />
        ) : route.kind === "library" ? (
          <LibraryPage source={route.source} view={route.view} />
        ) : route.kind === "device" ? (
          <DevicePage id={route.id} go={go} />
        ) : route.kind === "studio" ? (
          <StudioPage go={go} />
        ) : route.kind === "install" ? (
          <InstallPage build={route.build} go={go} />
        ) : (
          <Settings />
        )}
      </main>
    </div>
  );
}
