// Mirrors the Rust `AppState` (src-tauri/src/app.rs). Both test suites read src/fixtures/state.json.

export type ViewState =
  | "creating"
  | "running"
  | "paused"
  | "stopped"
  | "waking"
  | "pausing"
  | "error"
  | "deleting";

/** An ADE's id. */
export type AdeId = "t3" | "cmux" | "orca" | "paradev" | "paseo" | "herdr";

export type ErrorKind =
  | "freeLimit"
  | "cooldown"
  | "t3Busy"
  | "setup"
  | "signedOut"
  | "timeout"
  | "api"
  | "other";

/** Every command error, state.error and progress.error. */
export interface ErrorView {
  kind: ErrorKind;
  message: string;
  /** cooldown: minutes left. */
  minutes?: number;
  /** api: the API's error code. */
  code?: string;
}

export interface OrgView {
  id: string;
  name: string;
  tier: string;
  personal: boolean;
}

export interface SandboxView {
  id: string;
  name: string;
  state: ViewState;
  size: string;
  /** T3 was paired with it from this computer. cmux, Orca, Paradev, Paseo and Herdr never set it. */
  connected: boolean;
  /** The ADEs set up on the sandbox (its mark), in the order they were added. */
  ades: AdeId[];
  t3Version: string | null;
  free: boolean;
  autoSleep: { seconds: number | null; label: string };
  runLeftMinutes: number | null;
  cooldownMinutes: number | null;
  /** The sandbox's T3 is behind the desktop's and the update was deferred or failed: offer [Update]. */
  updatePending: boolean;
  /** The sandbox's T3 is newer than the desktop's: offer "Update T3 Code on this computer", never [Update]. */
  desktopBehind: boolean;
}

export interface SizeChoice {
  vcpus: number;
  memMiB: number;
  label: string;
}

export interface SleepChoice {
  seconds: number;
  label: string;
}

/** An ADE whose desktop app runs on this computer (cmux and Paradev are macOS only). */
export interface AdeView {
  id: AdeId;
  name: string;
  shortName: string;
  /** The desktop app's version; null when it is not found on this computer. For Herdr, which
   * runs on the sandbox, the OpenSSH client's, such as "OpenSSH 10.3p1". */
  version: string | null;
  downloadUrl: string;
}

export interface SnapshotView {
  id: string;
  name: string;
}

export interface McpStatus {
  /** null: the claude CLI is not installed. */
  claudeCode: boolean | null;
  codex: boolean | null;
}

/** The old app (Instabox.app) left next to this one, offered to move to the Trash. */
export interface OldAppView {
  paths: string[];
}

/**
 * A sandbox T3 Code saved with its published port, which it should reach through the local T3
 * proxy instead (Rust: `app::T3SwitchView`). The pairing URL to paste is never here:
 * copyT3SwitchUrl copies a new one.
 */
export interface T3SwitchView {
  id: string;
  name: string;
  /** The route T3 Code saved: the sandbox's published T3 port. */
  oldAddress: string;
  /** The route T3 Code adds: the proxy, http://localhost:<port>. */
  newAddress: string;
}

/** step: connecting | creating | waking | installing | pairing | done */
export interface ProgressView {
  /** create: a new sandbox. connect: one that exists. */
  kind: "create" | "connect";
  step: string;
  /** Set once the sandbox exists; Retry calls connect_sandbox with it and ade. */
  machineId: string | null;
  /** The ADE being set up; null for a create without one. */
  ade: AdeId | null;
  error: ErrorView | null;
}

/**
 * idle: nothing to say yet. checking, upToDate, checkFailed: Check for updates. available: offer
 * Update. downloading, installing: the app restarts when done. failed: offer Retry.
 */
export type UpdateStep = "idle" | "checking" | "upToDate" | "checkFailed" | "available" | "downloading" | "installing" | "failed";

/** This build's version and its updates (Rust: `updater::UpdateView`). */
export interface UpdateView {
  current: string;
  /** False in dev and preview builds: they never update. */
  enabled: boolean;
  step: UpdateStep;
  /** The newer version on offer, from available on. */
  version: string | null;
  /** downloading: how far, once the size is known. */
  percent: number | null;
  /** checkFailed, failed: why. */
  error: ErrorView | null;
}

export interface AppState {
  signedIn: boolean;
  login: string | null;
  signInCode: string | null;
  orgs: OrgView[];
  orgId: string | null;
  tier: string;
  /** The desktop T3 Code version. The same as T3's entry in ades. */
  t3: string | null;
  ades: AdeView[];
  sandboxes: SandboxView[];
  sizes: SizeChoice[];
  sleepChoices: SleepChoice[];
  snapshots: SnapshotView[];
  mcp: McpStatus;
  progress: ProgressView | null;
  oldApp: OldAppView | null;
  /** The guide that moves a sandbox to the local T3 proxy, while it shows. */
  t3Switch: T3SwitchView | null;
  error: ErrorView | null;
  startAtLogin: boolean;
  update: UpdateView | null;
}
