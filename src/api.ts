import { invoke as nativeInvoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AppState, ErrorView } from "./types";
import { isPlaceholderId, placeholderMessage } from "./placeholders";

export const isBrowserPreview = import.meta.env.DEV && !isTauri();
const preview = () => import("./preview");
const invokeBackend = isBrowserPreview
  ? <T,>(command: string, args?: Record<string, unknown>): Promise<T> =>
      preview().then((p) => p.invokePreview(command, args) as Promise<T>)
  : nativeInvoke;

// Never send a sample machine ID to Tauri, even from a nested view or menu.
const invoke = <T,>(command: string, args?: Record<string, unknown>): Promise<T> =>
  isPlaceholderId(args?.id)
    ? Promise.reject({ kind: "other", message: placeholderMessage })
    : args === undefined ? invokeBackend<T>(command) : invokeBackend<T>(command, args);

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
}) => invoke<void>("create_sandbox", a);
export const connectSandbox = (id: string) => invoke<void>("connect_sandbox", { id });
export const dismissProgress = () => invoke<void>("dismiss_progress");
export const refreshT3 = () => invoke<void>("refresh_t3");
export const openInT3 = (id: string) => invoke<void>("open_in_t3", { id });
export const pairAgain = (id: string) => invoke<void>("pair_again", { id });
export const pause = (id: string) => invoke<void>("pause", { id });
export const resume = (id: string) => invoke<void>("resume", { id });
export const deleteSandbox = (id: string) => invoke<void>("delete", { id });
export const resize = (id: string, vcpus: number, memMiB: number) => invoke<void>("resize", { id, vcpus, memMiB });
export const setAutoSleep = (id: string, seconds: number) => invoke<void>("set_auto_sleep", { id, seconds });
export const reinstallT3 = (id: string) => invoke<void>("reinstall_t3", { id });
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
