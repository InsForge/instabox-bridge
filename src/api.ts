import { invoke as nativeInvoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AdeId, AppState, ErrorView } from "./types";

/** `bun run dev` in a browser: sample data from preview.ts, never the native app. */
export const isBrowserPreview = import.meta.env.DEV && !isTauri();
const preview = () => import("./preview");
const invoke = <T,>(command: string, args?: Record<string, unknown>): Promise<T> =>
  isBrowserPreview
    ? preview().then((p) => p.invokePreview(command, args) as Promise<T>)
    : args === undefined
      ? nativeInvoke<T>(command)
      : nativeInvoke<T>(command, args);

/** Thin wrappers over the Tauri commands. Rejections are ErrorView objects. */
export type Agent = "claude" | "codex";
export type McpClient = "claudeCode" | "codex";

export const getState = () => invoke<AppState>("get_state");
export const onState = (cb: (s: AppState) => void) =>
  isBrowserPreview ? preview().then((p) => p.onPreviewState(cb)) : listen<AppState>("state", (e) => cb(e.payload));
/** A notice the back end hands the popover after a tray action, such as which Orca host to connect. */
export const onNotice = (cb: (notice: string) => void) =>
  isBrowserPreview ? Promise.resolve(() => {}) : listen<string>("notice", (e) => cb(e.payload));

export const signIn = (provider: "github" | "google") => invoke<void>("sign_in", { provider });
export const signOut = () => invoke<void>("sign_out");
export const switchOrg = (orgId: string) => invoke<void>("switch_org", { orgId });
export const createSandbox = (a: {
  name: string;
  snapshotId?: string;
  vcpus: number;
  memMiB: number;
  autoSuspendAfterSeconds?: number;
  /** The ADE to set up; none: a plain sandbox. */
  ade?: AdeId;
}) => invoke<string | null>("create_sandbox", a);
/** Sets the ADE up on the sandbox, then opens it. Resolves to what the user does next in the ADE, if anything. */
export const connectSandbox = (id: string, ade: AdeId) => invoke<string | null>("connect_sandbox", { id, ade });
export const dismissProgress = () => invoke<void>("dismiss_progress");
/** Looks for every ADE's desktop app on this computer again. */
export const refreshAdes = () => invoke<void>("refresh_t3");
/** Opens the sandbox in the ADE; a sandbox without it gets it first. */
export const openInAde = (id: string, ade: AdeId) => invoke<string | null>("open_in_t3", { id, ade });
export const pairAgain = (id: string) => invoke<void>("pair_again", { id, ade: "t3" });
export const pause = (id: string) => invoke<void>("pause", { id });
export const resume = (id: string) => invoke<void>("resume", { id });
export const deleteSandbox = (id: string) => invoke<void>("delete", { id });
export const resize = (id: string, vcpus: number, memMiB: number) => invoke<void>("resize", { id, vcpus, memMiB });
export const setAutoSleep = (id: string, seconds: number) => invoke<void>("set_auto_sleep", { id, seconds });
/** Sets T3 up again at this computer's version: Reinstall and Update. */
export const reinstallT3 = (id: string) => invoke<void>("reinstall_t3", { id, ade: "t3" });
export const agentSignIn = (id: string, agent: Agent) => invoke<void>("agent_sign_in", { id, agent });
export const agentStatus = (id: string, agent: Agent) => invoke<boolean>("agent_status", { id, agent });
export const sshCommand = (id: string) => invoke<string>("ssh_command", { id });
export const openSsh = (id: string) => invoke<void>("open_ssh", { id });
export const addMcp = (client: McpClient) => invoke<void>("add_mcp", { client });
export const setStartAtLogin = (on: boolean) => invoke<void>("set_start_at_login", { on });
/** Moves the old app (Instabox.app) to the Trash. */
export const trashOldApp = () => invoke<void>("trash_old_app");
/** Stops offering the old app until the next launch. */
export const dismissOldApp = () => invoke<void>("dismiss_old_app");
/** Copies a new pairing URL of the sandbox's T3 proxy for T3 Code's Add route. It is cleared a minute later. */
export const copyT3SwitchUrl = (id: string) => invoke<void>("copy_t3_switch_url", { id });
/** Closes the T3 switch guide. done: finished or turned off, so it does not show again for the sandbox. */
export const closeT3Switch = (id: string, done: boolean) => invoke<void>("close_t3_switch", { id, done });
export const openUrl = (url: string) => invoke<void>("open_url", { url });
/** Reads the update manifest now. Never downloads. The result shows in state.update. */
export const checkForUpdates = () => invoke<void>("check_for_updates");
/** Downloads, installs and restarts the app. A failure shows in state.update. */
export const installUpdate = () => invoke<void>("install_update");

export const T3_DOWNLOAD_URL = "https://github.com/pingdotgg/t3code/releases/latest";

/** Turns whatever a rejected command threw into an ErrorView. */
export function toErrorView(e: unknown): ErrorView {
  if (e && typeof e === "object" && "kind" in e && "message" in e) return e as ErrorView;
  return { kind: "other", message: typeof e === "string" ? e : e instanceof Error ? e.message : "Something went wrong" };
}
