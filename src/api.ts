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
}) => invoke<void>("create_sandbox", a);
/** Sets the ADE up on the sandbox, then opens it. */
export const connectSandbox = (id: string, ade: AdeId) => invoke<void>("connect_sandbox", { id, ade });
export const dismissProgress = () => invoke<void>("dismiss_progress");
/** Looks for every ADE's desktop app on this computer again. */
export const refreshAdes = () => invoke<void>("refresh_t3");
/** Opens the sandbox in the ADE; a sandbox without it gets it first. */
export const openInAde = (id: string, ade: AdeId) => invoke<void>("open_in_t3", { id, ade });
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
export const openUrl = (url: string) => invoke<void>("open_url", { url });

export const T3_DOWNLOAD_URL = "https://github.com/pingdotgg/t3code/releases/latest";

/** Turns whatever a rejected command threw into an ErrorView. */
export function toErrorView(e: unknown): ErrorView {
  if (e && typeof e === "object" && "kind" in e && "message" in e) return e as ErrorView;
  return { kind: "other", message: typeof e === "string" ? e : e instanceof Error ? e.message : "Something went wrong" };
}
