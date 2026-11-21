import fixture from "./fixtures/state.json";
import type { Ade } from "./ade";
import type { SandboxView } from "./types";

export type PlaceholderAde = Exclude<Ade, "t3">;

// Reserved IDs keep these examples separate from every real sandbox command.
export const placeholderMessage = "This environment is a placeholder. Switch to T3 Code to manage real sandboxes.";
export const isPlaceholderId = (id: unknown) => typeof id === "string" && id.startsWith("placeholder:");

const names = ["dev-workspace", "website", "experiment", "api-server", "mobile-app", "design-system", "staging"];
function machines(ade: PlaceholderAde, count: number): SandboxView[] {
  return names.slice(0, count).map((name, index) => ({
    ...fixture.sandboxes[0],
    id: `placeholder:${ade}:${index}`,
    name,
    state: index === 1 || index === 4 || index === 6 ? "paused" : "running",
    connected: false,
    ade: null,
    t3Version: null,
    updatePending: false,
    desktopBehind: false,
  } as SandboxView));
}

export const placeholderSandboxes: Record<PlaceholderAde, SandboxView[]> = {
  cmux: machines("cmux", 7),
  paradev: machines("paradev", 4),
  orca: [],
};
