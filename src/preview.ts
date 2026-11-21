import fixture from "./fixtures/state.json";
import type { AppState, SandboxView } from "./types";

// In-memory sample data only. This module never calls the native or cloud APIs.
let state: AppState = {
  ...(structuredClone(fixture) as AppState),
  login: "preview",
  progress: null,
  error: null,
  update: null,
  startAtLogin: false,
  sandboxes: [
    ...fixture.sandboxes.slice(0, 3).map((s, i) => ({
      ...s,
      name: ["dev-workspace", "website", "experiment"][i],
    })),
    ...[
      { id: "preview-api", name: "api-server", state: "running", connected: true, ades: ["t3", "cmux"] },
      { id: "preview-mobile", name: "mobile-app", state: "paused", connected: false, ades: [] },
      { id: "preview-design", name: "design-system", state: "running", connected: false, ades: ["cmux"] },
      { id: "preview-staging", name: "staging", state: "paused", connected: true, ades: ["t3"] },
    ].map((machine) => ({
      ...fixture.sandboxes[2],
      ...machine,
      t3Version: machine.connected ? fixture.t3 : null,
    })),
  ] as SandboxView[],
};
const listeners = new Set<(s: AppState) => void>();

export function onPreviewState(cb: (s: AppState) => void) {
  // Each subscription owns its listener, including overlapping StrictMode mounts.
  const listener = (next: AppState) => cb(next);
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export async function invokePreview(command: string, args: Record<string, unknown> = {}): Promise<unknown> {
  const patchSandbox = (patch: Partial<SandboxView>) => {
    state = { ...state, sandboxes: state.sandboxes.map((s) => s.id === args.id ? { ...s, ...patch } : s) };
  };
  switch (command) {
    case "get_state": return structuredClone(state);
    case "agent_status": return false;
    case "ssh_command": return "ssh preview@example.invalid";
    case "sign_in": state = { ...state, signedIn: true }; break;
    case "sign_out": state = { ...state, signedIn: false }; break;
    case "switch_org": {
      const org = state.orgs.find((o) => o.id === args.orgId);
      if (org) state = { ...state, orgId: org.id, tier: org.tier };
      break;
    }
    case "pause": patchSandbox({ state: "paused" }); break;
    case "resume": patchSandbox({ state: "running" }); break;
    case "delete": state = { ...state, sandboxes: state.sandboxes.filter((s) => s.id !== args.id) }; break;
    case "resize": patchSandbox({ size: `${args.vcpus} vCPU / ${Number(args.memMiB) / 1024} GB` }); break;
    case "set_auto_sleep": {
      const choice = state.sleepChoices.find((c) => c.seconds === args.seconds);
      if (choice) patchSandbox({ autoSleep: choice });
      break;
    }
    case "set_start_at_login": state = { ...state, startAtLogin: Boolean(args.on) }; break;
    case "dismiss_progress": state = { ...state, progress: null }; break;
    default:
      throw { kind: "other", message: "This is a browser preview. Use the InstaBox menu bar app for this action." };
  }
  for (const cb of listeners) cb(structuredClone(state));
}
