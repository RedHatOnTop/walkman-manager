export type Route =
  | { kind: "home" }
  | {
      kind: "library";
      /** A player's id to open on its music; the computer's otherwise. */
      source?: string;
      view?: "albums" | "sensme";
    }
  | { kind: "device"; id: string }
  | { kind: "studio" }
  | { kind: "install"; build: string }
  | { kind: "settings" };

export const HOME: Route = { kind: "home" };

export function sameRoute(a: Route, b: Route): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === "library") return b.kind === "library" && a.source === b.source && a.view === b.view;
  if (a.kind === "install") return b.kind === "install" && a.build === b.build;
  return a.kind !== "device" || (b.kind === "device" && a.id === b.id);
}
