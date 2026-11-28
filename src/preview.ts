import fixture from "./fixtures/state.json";
import type { AdeId, AdeView, AppState, SandboxView } from "./types";

// In-memory sample data only. This module never calls the native or cloud APIs.
let state: AppState = {
  ...(structuredClone(fixture) as AppState),
  login: "preview",
  progress: null,
  oldApp: null,
  // `?guide=t3-switch` shows the T3 switch guide for dev-workspace.
  t3Switch: typeof window !== "undefined" && new URLSearchParams(window.location.search).get("guide") === "t3-switch"
    ? { ...fixture.t3Switch, id: fixture.sandboxes[0].id, name: "dev-workspace" }
    : null,
  error: null,
  // Like a dev build: Settings shows the version and says updates are only for released builds.
  update: { current: fixture.update.current, enabled: false, step: "idle", version: null, percent: null, error: null },
  startAtLogin: false,
  // Every ADE a Mac can have. The shared fixture has no Paradev, Paseo or Herdr.
  ades: [
    ...fixture.ades,
    { id: "paradev", name: "Paradev", shortName: "Paradev", version: "2.0.11", downloadUrl: "https://github.com/paradevai/paradev/releases/latest" },
    { id: "paseo", name: "Paseo", shortName: "Paseo", version: "0.17.4", downloadUrl: "https://github.com/getpaseo/paseo/releases/latest" },
    { id: "herdr", name: "Herdr", shortName: "Herdr", version: "OpenSSH 10.3p1", downloadUrl: "https://learn.microsoft.com/windows-server/administration/openssh/openssh_install_firstuse" },
  ] as AdeView[],
  sandboxes: [
    ...fixture.sandboxes.slice(0, 3).map((s, i) => ({
      ...s,
      name: ["dev-workspace", "website", "experiment"][i],
    })),
    ...[
      { id: "preview-api", name: "api-server", state: "running", connected: true, ades: ["t3", "cmux"] },
      { id: "preview-mobile", name: "mobile-app", state: "paused", connected: false, ades: ["orca", "paradev"] },
      { id: "preview-design", name: "design-system", state: "running", connected: false, ades: ["cmux", "paseo", "herdr"] },
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
  // What the command resolves to, after the state goes out.
  let result: unknown;
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
    case "connect_sandbox": {
      const sandbox = state.sandboxes.find((s) => s.id === args.id);
      const ade = args.ade;
      if (!sandbox || (ade !== "t3" && ade !== "cmux" && ade !== "orca" && ade !== "paradev" && ade !== "paseo" && ade !== "herdr")) throw { kind: "other", message: "Unknown sandbox or ADE in this preview." };
      // Paradev sets the host up itself, so the app hands the setup over and marks nothing yet.
      if (ade === "paradev") {
        result = "The SSH address is copied. In Paradev, click Set up a host over SSH and paste it.";
        break;
      }
      patchSandbox({ ades: [...new Set<AdeId>([...sandbox.ades, ade])],
        ...(ade === "t3" ? { connected: true, t3Version: state.t3 } : {}) });
      if (ade === "orca") result = `In Orca, open Settings, then SSH Hosts. Click Import if instabox-${sandbox.name} is not listed, then connect it and open /data/home.`;
      if (ade === "paseo") result = `In Paseo, finish or skip its setup if it shows one. Then add this sandbox once: open Settings, click Switch host, Add host, then Remote SSH, and enter ssh://instabox-${sandbox.name}.`;
      break;
    }
    case "reinstall_t3": patchSandbox({ updatePending: false, t3Version: state.t3 }); break;
    case "delete": state = { ...state, sandboxes: state.sandboxes.filter((s) => s.id !== args.id) }; break;
    case "resize": patchSandbox({ size: `${args.vcpus} vCPU / ${Number(args.memMiB) / 1024} GB` }); break;
    case "set_auto_sleep": {
      const choice = state.sleepChoices.find((c) => c.seconds === args.seconds);
      if (choice) patchSandbox({ autoSleep: choice });
      break;
    }
    case "set_start_at_login": state = { ...state, startAtLogin: Boolean(args.on) }; break;
    case "dismiss_progress": state = { ...state, progress: null }; break;
    case "dismiss_old_app": state = { ...state, oldApp: null }; break;
    // Nothing is copied: there is no T3 proxy here.
    case "copy_t3_switch_url": return undefined;
    case "close_t3_switch": state = { ...state, t3Switch: null }; break;
    default:
      throw { kind: "other", message: "This is a browser preview. Use the InstaADE menu bar app for this action." };
  }
  for (const cb of listeners) cb(structuredClone(state));
  return result;
}
