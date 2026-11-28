import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { T3_DOWNLOAD_URL } from "./api";
import fixture from "./fixtures/state.json";
import { modeKey } from "./mode";
import type { AdeView, AppState, SandboxView, UpdateView } from "./types";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a), isTauri: () => true }));
vi.mock("./hero/HeroCube", () => ({ HeroCube: () => null }));
let emit: (s: AppState) => void = () => {};
/** Sends a notice the way the back end does after a tray action. */
let emitNotice: (notice: string) => void = () => {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, cb: (e: { payload: unknown }) => void) => {
    if (name === "state") emit = (s) => cb({ payload: s });
    if (name === "notice") emitNotice = (n) => cb({ payload: n });
    return Promise.resolve(() => {});
  },
}));

import App from "./App";

/** Typed literal: tsc checks every nested field against AppState. */
const sample: AppState = {
  signedIn: true,
  login: "alex",
  signInCode: null,
  orgs: [{ id: "org-me", name: "org-me", tier: "free", personal: true }],
  orgId: "org-me",
  tier: "free",
  t3: "0.1.10",
  ades: [{ id: "t3", name: "T3 Code", shortName: "T3", version: "0.1.10", downloadUrl: T3_DOWNLOAD_URL }],
  sandboxes: [],
  sizes: [
    { vcpus: 2, memMiB: 4096, label: "2 vCPU / 4 GB" },
    { vcpus: 4, memMiB: 8192, label: "4 vCPU / 8 GB" },
  ],
  sleepChoices: [{ seconds: 1800, label: "After 30 min" }],
  snapshots: [{ id: "s1", name: "snap-s1" }],
  mcp: { claudeCode: true, codex: false },
  progress: null,
  oldApp: null,
  t3Switch: null,
  error: null,
  startAtLogin: false,
  update: null,
};

const box: SandboxView = {
  id: "a",
  name: "dev-1",
  state: "running",
  size: "4 vCPU / 8 GB",
  connected: true,
  ades: ["t3"],
  t3Version: "0.1.10",
  free: true,
  autoSleep: { seconds: 1800, label: "After 30 min" },
  runLeftMinutes: 80,
  cooldownMinutes: null,
  updatePending: false,
  desktopBehind: false,
};

type Answer = (args: Record<string, unknown>) => unknown;

function show(over: Partial<AppState>, sandboxes: Partial<SandboxView>[] = [], answers: Record<string, Answer> = {}) {
  const state: AppState = { ...sample, ...over, sandboxes: sandboxes.map((s) => ({ ...box, ...s })) };
  invoke.mockImplementation((cmd: string, args: Record<string, unknown>) => {
    if (answers[cmd]) return Promise.resolve().then(() => answers[cmd](args));
    if (cmd === "get_state") return Promise.resolve(state);
    if (cmd === "agent_status") return Promise.resolve(false);
    if (cmd === "ssh_command") return Promise.resolve("ssh dev-1@ssh.instabox.sh");
    return Promise.resolve();
  });
  return { state, ...render(<App />) };
}

const calls = (cmd: string) => invoke.mock.calls.filter((c) => c[0] === cmd);
const rowOf = (name: string) => screen.getByText(name).closest("li") as HTMLElement;
const menuItems = () => within(screen.getByRole("menu")).getAllByRole("menuitem").map((b) => b.textContent);
const openMenu = (name: string) => userEvent.click(screen.getByRole("button", { name: `More actions for ${name}` }));

const t3Ade: AdeView = sample.ades[0];
const cmuxAde: AdeView = {
  id: "cmux",
  name: "cmux",
  shortName: "cmux",
  version: "0.64.25",
  downloadUrl: "https://github.com/manaflow-ai/cmux/releases/latest",
};
const orcaAde: AdeView = {
  id: "orca",
  name: "Orca",
  shortName: "Orca",
  version: "1.4.269",
  downloadUrl: "https://github.com/stablyai/orca/releases/latest",
};
/** A Mac with T3 Code and cmux. */
const mac: Partial<AppState> = { ades: [t3Ade, cmuxAde] };
const paradevAde: AdeView = {
  id: "paradev",
  name: "Paradev",
  shortName: "Paradev",
  version: "2.0.11",
  downloadUrl: "https://example.test/paradev/releases/latest",
};
/** No T3 Code on this computer. */
const noT3: Partial<AppState> = { t3: null, ades: [{ ...t3Ade, version: null }] };
/** The mode alex picked earlier on this computer. */
const picked = (mode: string, login = "alex") => localStorage.setItem(modeKey(login), mode);
const switcher = () => screen.getByRole("button", { name: /^ADE: .*\. Switch ADE$/ });
const popover = () => document.querySelector("main.popover") as HTMLElement;
/** The theme of the background behind the list: the mode, with plain Instabox drawn as T3. */
const theme = () => document.querySelector(".app-background .hero-atmosphere")?.getAttribute("data-theme");

beforeEach(() => {
  invoke.mockReset();
  localStorage.clear();
});
afterEach(cleanup);

test("fixture and sample have the same nested keys", () => {
  const keys = (v: unknown, p = ""): string[] =>
    Array.isArray(v)
      ? v.length
        ? keys(v[0], p + "[]")
        : [p + "[]"]
      : v && typeof v === "object"
        ? Object.entries(v).flatMap(([k, x]) => keys(x, `${p}.${k}`))
        : [p];
  const sampleWithRows: AppState = {
    ...sample,
    sandboxes: [box],
    progress: { kind: "create", step: "x", machineId: null, ade: "t3", error: null },
    oldApp: { paths: [""] },
    t3Switch: { id: "", name: "", oldAddress: "", newAddress: "" },
    error: { kind: "other", message: "" },
    update: { current: "", enabled: true, step: "idle", version: null, percent: null, error: null },
  };
  // minutes and code are optional on ErrorView.
  const strip = (ks: string[]) => [...new Set(ks.filter((k) => !/\.(minutes|code)$/.test(k)))].sort();
  const want = strip(keys(sampleWithRows));
  const got = strip(keys(fixture));
  expect(got.filter((k) => !want.includes(k))).toEqual([]);
  expect(want.filter((k) => !got.includes(k))).toEqual([]);
});

// ---- list ----

test("a row shows state, connected ADEs and compact power controls", async () => {
  show({}, [{}, { id: "b", name: "web", state: "paused", t3Version: null }, { id: "c", name: "old", updatePending: true, t3Version: "0.1.2" }]);
  const row = await screen.findByText("dev-1").then(() => rowOf("dev-1"));
  expect(within(row).getByText("Running")).toBeTruthy();
  expect(within(row).getByRole("img", { name: "T3 Code" })).toBeTruthy();
  expect(within(rowOf("web")).getByText("Paused")).toBeTruthy();
  expect(within(rowOf("web")).getByRole("img", { name: "T3 Code" })).toBeTruthy();
  expect(within(rowOf("old")).getByText("needs update")).toBeTruthy();
  const labels = within(row)
    .getAllByRole("button")
    .slice(1)
    .map((b) => b.getAttribute("aria-label") ?? b.textContent);
  expect(labels).toEqual(["Pause dev-1", "More actions for dev-1"]);
  expect(screen.getByRole("heading", { name: "Sandboxes 3" })).toBeTruthy();
  expect(screen.getByRole("combobox", { name: "Workspace" }).textContent).toBe("org-me");
});

test("row power controls call the correct APIs without opening details", async () => {
  show({}, [{}, { id: "b", name: "web", state: "paused" }, { id: "c", name: "stopped", state: "stopped" }, { id: "d", name: "busy", state: "waking" }]);
  await userEvent.click(await screen.findByRole("button", { name: "Pause dev-1" }));
  expect(invoke).toHaveBeenCalledWith("pause", { id: "a" });
  await userEvent.click(screen.getByRole("button", { name: "Resume web" }));
  expect(invoke).toHaveBeenCalledWith("resume", { id: "b" });
  await userEvent.click(screen.getByRole("button", { name: "Resume stopped" }));
  expect(invoke).toHaveBeenCalledWith("resume", { id: "c" });
  expect(screen.queryByRole("button", { name: /^(Pause|Resume) busy$/ })).toBeNull();
  expect(screen.queryByRole("button", { name: "Back to sandboxes" })).toBeNull();
});

test("a pending power action cannot be submitted twice and reports errors", async () => {
  let fail!: (reason: unknown) => void;
  show({}, [{}], { pause: () => new Promise((_, reject) => { fail = reject; }) });
  const button = await screen.findByRole("button", { name: "Pause dev-1" });
  await userEvent.click(button);
  expect((button as HTMLButtonElement).disabled).toBe(true);
  await userEvent.click(button);
  expect(calls("pause")).toHaveLength(1);
  await act(async () => fail({ kind: "api", message: "Unable to pause" }));
  expect(await screen.findByText("Unable to pause")).toBeTruthy();
  expect((button as HTMLButtonElement).disabled).toBe(false);
});

test("signing in lands on the list, with the empty state and no ADE asked for", async () => {
  const { state } = show({ ...noT3, signedIn: false });
  await screen.findByRole("button", { name: "Sign in with GitHub" });
  act(() => emit({ ...state, signedIn: true }));
  expect(await screen.findByRole("heading", { name: "No sandboxes" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "New sandbox" })).toBeTruthy();
  expect(screen.getByText("Open sandboxes in your ADE")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Sign in with GitHub" })).toBeNull();
});

test("the workspace footer switches the organization and opens the dashboard and settings", async () => {
  show({ orgs: [...sample.orgs, { id: "org-team", name: "team", tier: "pro", personal: false }] }, [{}]);
  const workspace = await screen.findByRole("combobox", { name: "Workspace" });
  expect(workspace.textContent).toBe("org-me");
  await userEvent.click(workspace);
  await userEvent.click(screen.getByRole("option", { name: "team" }));
  expect(invoke).toHaveBeenCalledWith("switch_org", { orgId: "org-team" });
  await userEvent.click(screen.getByRole("button", { name: "Open InstaBox dashboard" }));
  expect(invoke).toHaveBeenCalledWith("open_url", { url: "https://dashboard.instabox.sh/dashboard/machines" });
  await userEvent.click(screen.getByRole("button", { name: "Settings" }));
  expect(screen.getByRole("heading", { name: "Account" })).toBeTruthy();
});

test("one organization leaves the workspace picker disabled", async () => {
  show({}, [{}]);
  expect(((await screen.findByRole("combobox", { name: "Workspace" })) as HTMLButtonElement).disabled).toBe(true);
});

test("clicking a row opens its details and Back returns to the list", async () => {
  show({}, [{}, { id: "b", name: "web", connected: false, ades: [] }]);
  await userEvent.click(await screen.findByText("dev-1"));
  expect(screen.getByRole("button", { name: "Back to sandboxes" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Open in T3 Code" })).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "Back to sandboxes" }));
  // The line under a row is part of the row too, except for its button.
  await userEvent.click(screen.getByText("no ADE connected"));
  expect(screen.getByRole("button", { name: "Install T3 Code" })).toBeTruthy();
  expect(screen.queryByText("dev-1")).toBeNull();
});

test("details of a sandbox that disappears go back to the list", async () => {
  show({}, [{}]);
  await userEvent.click(await screen.findByText("dev-1"));
  act(() => emit({ ...sample, sandboxes: [] }));
  expect(await screen.findByRole("heading", { name: "No sandboxes" })).toBeTruthy();
  expect(screen.getByText("Create one and open it in T3 Code.")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Back to sandboxes" })).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "New sandbox" }));
  expect(screen.getByLabelText("Name")).toBeTruthy();
});

test("a pushed state replaces the screen", async () => {
  show({}, [{}]);
  await screen.findByText("dev-1");
  act(() => emit({ ...sample, sandboxes: [{ ...box, name: "other" }] }));
  expect(await screen.findByText("other")).toBeTruthy();
});

test("the top bar of every screen drags the window, and not while the ADE menu is open", async () => {
  const { state } = show({ signedIn: false }, [{}]);
  await screen.findByRole("button", { name: "Sign in with GitHub" });
  expect(popover().querySelector(".setup")?.getAttribute("data-tauri-drag-region")).toBe("true");
  act(() => emit({ ...state, signedIn: true }));
  await screen.findByText("dev-1");
  const bar = () => popover().querySelector("header")?.getAttribute("data-tauri-drag-region");
  expect(bar()).toBe("deep");
  await userEvent.click(switcher());
  expect(bar()).toBeNull();
  await userEvent.keyboard("{Escape}");
  expect(bar()).toBe("deep");
  await userEvent.click(screen.getByText("dev-1"));
  expect(screen.getByRole("button", { name: "Back to sandboxes" }).closest("header")?.getAttribute("data-tauri-drag-region")).toBe("deep");
  await userEvent.click(screen.getByRole("button", { name: "Back to sandboxes" }));
  await userEvent.click(screen.getByRole("button", { name: "Settings" }));
  expect(screen.getByRole("button", { name: "Back to sandboxes" }).closest("header")?.getAttribute("data-tauri-drag-region")).toBe("deep");
  await userEvent.click(screen.getByRole("button", { name: "Back to sandboxes" }));
  await userEvent.click(screen.getByRole("button", { name: "New sandbox" }));
  expect(screen.getByRole("button", { name: "Back to sandboxes" }).closest("header")?.getAttribute("data-tauri-drag-region")).toBe("deep");
});

// ---- the … menu ----

test("the menu of a running, connected sandbox", async () => {
  show({ tier: "pro" }, [{ free: false }]);
  await screen.findByText("dev-1");
  await openMenu("dev-1");
  expect(menuItems()).toEqual([
    "Open in T3 Code",
    "Pair again",
    "Pause",
    "Resize4 vCPU / 8 GB",
    "Auto sleepAfter 30 min",
    "Copy SSH command",
    "Open in Terminal",
    "Reinstall T3",
    "Delete sandbox",
  ]);
  expect(screen.getByRole("button", { name: "More actions for dev-1" }).getAttribute("aria-expanded")).toBe("true");
  // The menu takes focus, so the keyboard can walk it.
  expect(document.activeElement?.textContent).toBe("Open in T3 Code");
  await userEvent.keyboard("{ArrowDown}");
  expect(document.activeElement?.textContent).toBe("Pair again");
});

test("the menu of a paused sandbox without T3", async () => {
  show({}, [{ state: "paused", connected: false, ades: [], t3Version: null }]);
  await screen.findByText("dev-1");
  await openMenu("dev-1");
  expect(menuItems()).toEqual([
    "Install T3 Code",
    "Resume",
    "Resize4 vCPU / 8 GB",
    "Auto sleepAfter 30 min",
    "Copy SSH command",
    "Open in Terminal",
    "Delete sandbox",
  ]);
  // The free plan fixes size and auto sleep.
  const menu = within(screen.getByRole("menu"));
  expect((menu.getByText("Resize").closest("button") as HTMLButtonElement).disabled).toBe(true);
  expect((menu.getByText("Auto sleep").closest("button") as HTMLButtonElement).disabled).toBe(true);
  await userEvent.click(menu.getByRole("menuitem", { name: "Resume" }));
  expect(invoke).toHaveBeenCalledWith("resume", { id: "a" });
});

test("a busy sandbox cannot be opened from the menu", async () => {
  show({}, [{ state: "waking" }]);
  await screen.findByText("dev-1");
  await openMenu("dev-1");
  expect((screen.getByRole("menuitem", { name: "Open in T3 Code" }) as HTMLButtonElement).disabled).toBe(true);
});

test("the menu closes on Escape, on a click outside and after an action", async () => {
  show({}, [{}]);
  await screen.findByText("dev-1");
  const more = screen.getByRole("button", { name: "More actions for dev-1" });
  await openMenu("dev-1");
  await userEvent.keyboard("{Escape}");
  expect(screen.queryByRole("menu")).toBeNull();
  expect(document.activeElement).toBe(more);

  await openMenu("dev-1");
  await userEvent.click(screen.getByText("Sandboxes"));
  expect(screen.queryByRole("menu")).toBeNull();

  await openMenu("dev-1");
  await openMenu("dev-1");
  expect(screen.queryByRole("menu")).toBeNull();

  await openMenu("dev-1");
  await userEvent.click(screen.getByRole("menuitem", { name: "Pause" }));
  expect(invoke).toHaveBeenCalledWith("pause", { id: "a" });
  expect(screen.queryByRole("menu")).toBeNull();
});

test("Resize and Auto sleep open their choices inside the menu", async () => {
  const sleep = [600, 1800].map((s) => ({ seconds: s, label: `After ${s / 60} min` }));
  show({ tier: "pro", sleepChoices: sleep }, [{ free: false }]);
  await screen.findByText("dev-1");
  await openMenu("dev-1");
  await userEvent.click(screen.getByText("Resize"));
  const current = screen.getByRole("menuitemradio", { name: "4 vCPU / 8 GB" });
  expect(current.getAttribute("aria-checked")).toBe("true");
  expect(screen.getByText("Restarts the sandbox")).toBeTruthy();
  await userEvent.click(screen.getByRole("menuitemradio", { name: "2 vCPU / 4 GB" }));
  expect(invoke).toHaveBeenCalledWith("resize", { id: "a", vcpus: 2, memMiB: 4096 });
  expect(screen.queryByRole("menu")).toBeNull();

  await openMenu("dev-1");
  await userEvent.click(screen.getByText("Auto sleep"));
  expect(screen.getByRole("menuitemradio", { name: "After 30 min" }).getAttribute("aria-checked")).toBe("true");
  await userEvent.click(screen.getByRole("menuitemradio", { name: "After 10 min" }));
  expect(invoke).toHaveBeenCalledWith("set_auto_sleep", { id: "a", seconds: 600 });
});

test("Delete sandbox asks to confirm inside the menu", async () => {
  show({}, [{}]);
  await screen.findByText("dev-1");
  await openMenu("dev-1");
  await userEvent.click(screen.getByRole("menuitem", { name: "Delete sandbox" }));
  expect(calls("delete").length).toBe(0);
  await userEvent.click(screen.getByRole("menuitem", { name: "Confirm delete" }));
  expect(invoke).toHaveBeenCalledWith("delete", { id: "a" });
});

test("Copy SSH command asks for the command only when chosen and copies it", async () => {
  const user = userEvent.setup();
  show({}, [{}]);
  await screen.findByText("dev-1");
  await user.click(screen.getByRole("button", { name: "More actions for dev-1" }));
  expect(calls("ssh_command").length).toBe(0);
  await user.click(screen.getByRole("menuitem", { name: "Copy SSH command" }));
  expect(await screen.findByText("SSH command copied")).toBeTruthy();
  expect(invoke).toHaveBeenCalledWith("ssh_command", { id: "a" });
  expect(await navigator.clipboard.readText()).toBe("ssh dev-1@ssh.instabox.sh");
});

test("details have every action inline, with no … menu in the header", async () => {
  show({}, [{}]);
  await userEvent.click(await screen.findByText("dev-1"));
  expect(screen.queryByRole("button", { name: "More actions for dev-1" })).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "Open in Terminal" }));
  expect(invoke).toHaveBeenCalledWith("open_ssh", { id: "a" });
});

// ---- the line under a row ----

test("each row shows at most one line, by priority", async () => {
  show({ progress: { kind: "connect", step: "installing", machineId: "a", ade: "t3", error: null } }, [
    { id: "a", name: "p-progress", updatePending: true },
    { id: "b", name: "p-update", updatePending: true, t3Version: "0.1.2" },
    { id: "c", name: "p-behind", desktopBehind: true, updatePending: true },
    { id: "d", name: "p-install", connected: false, ades: [] },
    { id: "e", name: "p-behind-unconnected", connected: false, ades: [], desktopBehind: true },
    { id: "f", name: "p-open" },
  ]);
  await screen.findByText("p-open");
  const progress = within(rowOf("p-progress"));
  expect(progress.getByText("Installing T3 0.1.10")).toBeTruthy();
  expect(progress.getByRole("progressbar")).toBeTruthy();
  expect(progress.queryByRole("button", { name: "Update" })).toBeNull();
  expect(progress.queryByRole("button", { name: "Open in T3 Code" })).toBeNull();

  const update = within(rowOf("p-update"));
  expect(update.getByText("needs update")).toBeTruthy();
  await userEvent.click(update.getByRole("button", { name: "Update" }));
  expect(invoke).toHaveBeenCalledWith("reinstall_t3", { id: "b", ade: "t3" });

  // Desktop updates open the download; they must never reinstall the sandbox.
  const behind = within(rowOf("p-behind"));
  expect(behind.getByText("needs update")).toBeTruthy();
  await userEvent.click(behind.getByRole("button", { name: "Update" }));
  expect(invoke).not.toHaveBeenCalledWith("reinstall_t3", { id: "c", ade: "t3" });
  expect(invoke).toHaveBeenCalledWith("open_url", { url: T3_DOWNLOAD_URL });

  const install = within(rowOf("p-install"));
  expect(install.getByText("no ADE connected")).toBeTruthy();
  await userEvent.click(install.getByRole("button", { name: "Connect" }));
  expect(invoke).toHaveBeenCalledWith("connect_sandbox", { id: "d", ade: "t3" });

  expect(within(rowOf("p-behind-unconnected")).queryByText("No T3 Code here yet")).toBeNull();
  const open = within(rowOf("p-open"));
  expect(open.getByText("connected")).toBeTruthy();
  expect(open.queryByRole("button", { name: "Connect" })).toBeNull();
  await userEvent.click(screen.getByText("p-open"));
  await userEvent.click(screen.getByRole("button", { name: "Open in T3 Code" }));
  expect(invoke).toHaveBeenCalledWith("open_in_t3", { id: "f", ade: "t3" });
});

test("the list never asks for agent status; it shows what details found", async () => {
  // Plain Instabox: a row has no ADE line, so the agent line shows.
  picked("instabox");
  show({}, [{}], { agent_status: (a) => a.agent === "claude" });
  await screen.findByText("dev-1");
  await openMenu("dev-1");
  await userEvent.keyboard("{Escape}");
  expect(calls("agent_status").length).toBe(0);
  expect(screen.queryByText(/needs a sign-in/)).toBeNull();

  await userEvent.click(screen.getByText("dev-1"));
  await waitFor(() => expect(calls("agent_status").length).toBe(2));
  expect(await screen.findByText("Signed in")).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "Back to sandboxes" }));

  const row = within(rowOf("dev-1"));
  expect(row.getByText("Codex needs a sign-in")).toBeTruthy();
  await userEvent.click(row.getByRole("button", { name: "Sign in" }));
  expect(invoke).toHaveBeenCalledWith("agent_sign_in", { id: "a", agent: "codex" });
  await waitFor(() => expect(screen.queryByText("Codex needs a sign-in")).toBeNull());
  expect(calls("agent_status").length).toBe(2);
});

test("a create before its sandbox exists is a row at the top with its progress", async () => {
  show({ progress: { kind: "create", step: "creating", machineId: null, ade: "t3", error: null } }, [{}]);
  await screen.findByText("dev-1");
  const first = screen.getAllByRole("listitem")[0];
  expect(within(first).getByText("New sandbox")).toBeTruthy();
  expect(within(first).getByText("Creating")).toBeTruthy();
  expect(within(first).getByText("Creating sandbox")).toBeTruthy();
  expect(within(first).getByRole("progressbar")).toBeTruthy();
});

test("connecting an existing sandbox starts at Waking, not Creating", async () => {
  show({ progress: { kind: "connect", step: "connecting", machineId: "a", ade: "t3", error: null } }, [{}]);
  expect(await screen.findByText("Waking sandbox")).toBeTruthy();
  expect(screen.queryByText("Creating sandbox")).toBeNull();
});

// ---- new sandbox ----

test("free plan: size and auto sleep are disabled and fixed", async () => {
  show({ tier: "free" });
  await userEvent.click(await screen.findByRole("button", { name: "New sandbox" }));
  const size = within(screen.getByRole("group", { name: "Size" })).getByRole("button") as HTMLButtonElement;
  const sleep = screen.getByLabelText("Auto sleep") as HTMLSelectElement;
  expect(size.disabled).toBe(true);
  expect(size.textContent).toBe("4 vCPU / 8 GB");
  expect(size.getAttribute("aria-pressed")).toBe("true");
  expect(sleep.disabled).toBe(true);
  expect(sleep.selectedOptions[0].textContent).toContain("30 min");
  expect(screen.getByText("Installs T3 0.1.10, the same version as T3 Code on this Mac. Then the sandbox opens in T3 Code.")).toBeTruthy();
});

const paidSleep = [300, 600, 900, 1200, 1500, 1800].map((s) => ({ seconds: s, label: `After ${s / 60} min` }));

test("paid plan with always on: auto sleep defaults to 30 min, size to the first, Never reads plainly", async () => {
  show({ tier: "pro", sleepChoices: [...paidSleep, { seconds: 0, label: "Never" }] });
  await userEvent.click(await screen.findByRole("button", { name: "New sandbox" }));
  const sizes = within(screen.getByRole("group", { name: "Size" }));
  const sleep = screen.getByLabelText("Auto sleep") as HTMLSelectElement;
  expect(sleep.value).toBe("1800");
  expect(sleep.selectedOptions[0].textContent).toBe("After 30 min");
  expect([...sleep.options].map((o) => o.textContent)).toContain("Never");
  expect([...sleep.options].some((o) => /After Never|Off/.test(o.textContent ?? ""))).toBe(false);
  expect(sizes.getByRole("button", { name: "2 vCPU / 4 GB" }).getAttribute("aria-pressed")).toBe("true");
  await userEvent.type(screen.getByLabelText("Name"), "dev-2");
  await userEvent.click(screen.getByRole("button", { name: "Create sandbox" }));
  expect(invoke).toHaveBeenCalledWith("create_sandbox", {
    name: "dev-2",
    snapshotId: undefined,
    vcpus: 2,
    memMiB: 4096,
    autoSuspendAfterSeconds: 1800,
    ade: "t3",
  });
});

test("a picked size and auto sleep go into the create", async () => {
  show({ tier: "pro", sleepChoices: paidSleep });
  await userEvent.click(await screen.findByRole("button", { name: "New sandbox" }));
  await userEvent.type(screen.getByLabelText("Name"), "dev-2");
  await userEvent.click(screen.getByRole("button", { name: "4 vCPU / 8 GB" }));
  await userEvent.selectOptions(screen.getByLabelText("Auto sleep"), "600");
  await userEvent.click(screen.getByRole("button", { name: "Create sandbox" }));
  expect(invoke).toHaveBeenCalledWith("create_sandbox", expect.objectContaining({ vcpus: 4, memMiB: 8192, autoSuspendAfterSeconds: 600 }));
});

test("without 30 min the default is the first choice that sleeps", async () => {
  show({ tier: "pro", sleepChoices: [{ seconds: 0, label: "Never" }, ...paidSleep.slice(0, 2)] });
  await userEvent.click(await screen.findByRole("button", { name: "New sandbox" }));
  expect((screen.getByLabelText("Auto sleep") as HTMLSelectElement).value).toBe("300");
});

test("My snapshot never creates a Standard sandbox and picks the first snapshot that arrives", async () => {
  show({ tier: "pro" });
  await userEvent.click(await screen.findByRole("button", { name: "New sandbox" }));
  await userEvent.type(screen.getByLabelText("Name"), "dev-2");
  await userEvent.click(screen.getByRole("button", { name: "My snapshot" }));
  expect(screen.getByRole("button", { name: "My snapshot" }).getAttribute("aria-pressed")).toBe("true");
  const create = screen.getByRole("button", { name: "Create sandbox" }) as HTMLButtonElement;
  expect(create.disabled).toBe(false);
  act(() => emit({ ...sample, tier: "pro", snapshots: [] }));
  expect(create.disabled).toBe(true);
  act(() => emit({ ...sample, tier: "pro", snapshots: [{ id: "s2", name: "snap-s2" }] }));
  expect((screen.getByLabelText("Snapshot") as HTMLSelectElement).value).toBe("s2");
  expect(create.disabled).toBe(false);
  await userEvent.click(create);
  expect(invoke).toHaveBeenCalledWith("create_sandbox", expect.objectContaining({ snapshotId: "s2" }));
});

test("My snapshot is off without snapshots", async () => {
  show({ tier: "pro", snapshots: [] });
  await userEvent.click(await screen.findByRole("button", { name: "New sandbox" }));
  expect((screen.getByRole("button", { name: "My snapshot" }) as HTMLButtonElement).disabled).toBe(true);
});

test("a create returns to the list at once and shows the new row while it runs", async () => {
  let finish: () => void = () => {};
  show({ tier: "pro" }, [{}], { create_sandbox: () => new Promise<void>((r) => (finish = r)) });
  await userEvent.click(await screen.findByRole("button", { name: "New sandbox" }));
  await userEvent.type(screen.getByLabelText("Name"), "dev-2");
  await userEvent.click(screen.getByRole("button", { name: "Create sandbox" }));
  expect(screen.queryByLabelText("Name")).toBeNull();
  act(() =>
    emit({ ...sample, tier: "pro", sandboxes: [box], progress: { kind: "create", step: "creating", machineId: null, ade: "t3", error: null } }),
  );
  const first = within(screen.getAllByRole("listitem")[0]);
  expect(first.getByText("dev-2")).toBeTruthy();
  expect(first.getByText("2 vCPU / 4 GB · Creating sandbox")).toBeTruthy();
  await act(async () => finish());
  // T3 Code asks to confirm in its own window. The popover adds no notice.
  expect(screen.queryByText(/Confirm the/)).toBeNull();
});

test("a successful create adds no notice", async () => {
  show({ tier: "pro" }, [{}]);
  await userEvent.click(await screen.findByRole("button", { name: "New sandbox" }));
  await userEvent.type(screen.getByLabelText("Name"), "dev-2");
  await userEvent.click(screen.getByRole("button", { name: "Create sandbox" }));
  expect(invoke).toHaveBeenCalledWith("create_sandbox", expect.objectContaining({ name: "dev-2" }));
  await act(async () => {});
  expect(screen.getByText("dev-1")).toBeTruthy();
  expect(screen.queryByText(/Confirm the/)).toBeNull();
});

test("a failed create shows its message in the list", async () => {
  show({ tier: "pro" }, [{}], { create_sandbox: () => Promise.reject({ kind: "api", message: "no_capacity" }) });
  await userEvent.click(await screen.findByRole("button", { name: "New sandbox" }));
  await userEvent.type(screen.getByLabelText("Name"), "dev-2");
  await userEvent.click(screen.getByRole("button", { name: "Create sandbox" }));
  expect(await screen.findByText("no_capacity")).toBeTruthy();
  expect(screen.getByText("dev-1")).toBeTruthy();
});

const tarBad = { kind: "setup" as const, message: "tar: bad" };

test("a failed create with a sandbox offers only Retry, not Create", async () => {
  show({ tier: "pro", progress: { kind: "create", step: "installing", machineId: "m9", ade: "t3", error: tarBad } }, [{}]);
  await userEvent.click(await screen.findByRole("button", { name: "New sandbox" }));
  await userEvent.type(screen.getByLabelText("Name"), "dev-2");
  expect((screen.getByRole("button", { name: "Create sandbox" }) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getAllByRole("button", { name: "Retry" }).length).toBe(1);
});

test("Cancel goes back to the list", async () => {
  show({}, [{}]);
  await userEvent.click(await screen.findByRole("button", { name: "New sandbox" }));
  await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(screen.getByText("dev-1")).toBeTruthy();
});

// ---- errors and progress ----

test("cooldown shows the cooling down line", async () => {
  show({ error: { kind: "cooldown", message: "x", minutes: 12 } }, [{}]);
  expect(await screen.findByText("Cooling down, ready in about 12 min")).toBeTruthy();
});

test("free limit offers Connect on the sandbox without the ADE, not create", async () => {
  show({ error: { kind: "freeLimit", message: "x" } }, [{ connected: false, ades: [] }]);
  expect(await screen.findByText("The free plan has one sandbox")).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "Connect to T3 Code" }));
  expect(invoke).toHaveBeenCalledWith("connect_sandbox", { id: "a", ade: "t3" });
  expect(within(rowOf("dev-1")).getByRole("button", { name: "Connect" })).toBeTruthy();
});

test("free limit in plain Instabox offers no Connect", async () => {
  picked("instabox");
  show({ error: { kind: "freeLimit", message: "x" } }, [{ connected: false, ades: [] }]);
  expect(await screen.findByText("The free plan has one sandbox")).toBeTruthy();
  expect(screen.queryByRole("button", { name: /Connect/ })).toBeNull();
});

test("retry after a failed create connects the existing sandbox with the same ADE", async () => {
  show({ ...mac, progress: { kind: "create", step: "installing", machineId: "m9", ade: "cmux", error: tarBad } }, [{}]);
  await userEvent.click(await screen.findByRole("button", { name: "New sandbox" }));
  expect(await screen.findByText("tar: bad")).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "Retry" }));
  // In T3 mode, the retry still sets up the ADE the failed create set up.
  expect(invoke).toHaveBeenCalledWith("connect_sandbox", { id: "m9", ade: "cmux" });
  expect(invoke).not.toHaveBeenCalledWith("create_sandbox", expect.anything());
});

test("a failed connect shows one notice and one Retry, in list and new views", async () => {
  show({ error: tarBad, progress: { kind: "create", step: "installing", machineId: "m9", ade: "t3", error: tarBad } }, [{}]);
  expect((await screen.findAllByText("tar: bad")).length).toBe(1);
  expect(screen.getAllByRole("button", { name: "Retry" }).length).toBe(1);
  await userEvent.click(screen.getByRole("button", { name: "New sandbox" }));
  expect(screen.getAllByText("tar: bad").length).toBe(1);
  expect(screen.getAllByRole("button", { name: "Retry" }).length).toBe(1);
});

test("settings shows a progress error as a banner with its Retry", async () => {
  show({ error: tarBad, progress: { kind: "create", step: "installing", machineId: "m9", ade: "t3", error: tarBad } }, [{}]);
  await userEvent.click(await screen.findByRole("button", { name: "Settings" }));
  expect(screen.getAllByText("tar: bad").length).toBe(1);
  await userEvent.click(screen.getByRole("button", { name: "Retry" }));
  expect(invoke).toHaveBeenCalledWith("connect_sandbox", { id: "m9", ade: "t3" });
});

test("a different state.error still shows next to a progress error", async () => {
  show(
    {
      error: { kind: "cooldown", message: "x", minutes: 12 },
      progress: { kind: "create", step: "installing", machineId: "m9", ade: "t3", error: tarBad },
    },
    [{}],
  );
  expect((await screen.findAllByText("Cooling down, ready in about 12 min")).length).toBe(1);
  expect(screen.getAllByText("tar: bad").length).toBe(1);
  expect(screen.getAllByRole("button", { name: "Retry" }).length).toBe(1);
});

test("Dismiss closes a failed progress", async () => {
  show({ progress: { kind: "create", step: "installing", machineId: "m9", ade: "t3", error: tarBad } }, [{}]);
  await userEvent.click(await screen.findByRole("button", { name: "Dismiss" }));
  expect(invoke).toHaveBeenCalledWith("dismiss_progress");
});

test("a rejected connect that is the progress error shows once", async () => {
  show({ error: tarBad, progress: { kind: "connect", step: "installing", machineId: "a", ade: "t3", error: tarBad } }, [{ connected: false }], {
    connect_sandbox: () => Promise.reject(tarBad),
  });
  const row = await screen.findByText("dev-1").then(() => within(rowOf("dev-1")));
  await userEvent.click(row.getByRole("button", { name: "Retry" }));
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("connect_sandbox", { id: "a", ade: "t3" }));
  expect(screen.getAllByText("tar: bad").length).toBe(1);
  expect(screen.getAllByRole("button", { name: "Retry" }).length).toBe(1);
});

test("another setup error (a reinstall) gets no Retry of the progress sandbox", async () => {
  show({ error: { kind: "setup", message: "reinstall: boom" }, progress: { kind: "create", step: "installing", machineId: "m9", ade: "t3", error: tarBad } }, [
    {},
  ]);
  expect(await screen.findByText("reinstall: boom")).toBeTruthy();
  // The one Retry is the progress's own.
  expect(screen.getAllByRole("button", { name: "Retry" }).length).toBe(1);
  expect(screen.getByText("tar: bad").closest(".progress")).toBeTruthy();
});

// ---- details ----

test("details of a sandbox without T3 offer Install T3 Code", async () => {
  show({}, [{ connected: false, ades: [] }]);
  await userEvent.click(await screen.findByText("dev-1"));
  expect(screen.queryByRole("button", { name: "Open in T3 Code" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Pair again" })).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "Install T3 Code" }));
  expect(invoke).toHaveBeenCalledWith("connect_sandbox", { id: "a", ade: "t3" });
  await act(async () => {});
  expect(screen.queryByText(/Confirm the/)).toBeNull();
});

test("details of a sandbox T3 is on but not paired from here: Open pairs it", async () => {
  show({}, [{ connected: false }]);
  await userEvent.click(await screen.findByText("dev-1"));
  // Pair again and Reinstall need a pairing from this computer.
  expect(screen.queryByRole("button", { name: "Pair again" })).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "Open in T3 Code" }));
  expect(invoke).toHaveBeenCalledWith("open_in_t3", { id: "a", ade: "t3" });
  await act(async () => {});
  expect(screen.queryByText(/Confirm the/)).toBeNull();
});

test("details show the T3 version against this Mac and the free plan line", async () => {
  show({}, [{}]);
  await userEvent.click(await screen.findByText("dev-1"));
  expect(within(screen.getByRole("region", { name: "Connected ADE" })).getByText("0.1.10")).toBeTruthy();
  expect(screen.getByText("Can run 1 h 20 m more")).toBeTruthy();
  expect(screen.getByText("Free plan")).toBeTruthy();
  // Delete sits at the end of the page, under the agents with their logos.
  const del = screen.getByRole("button", { name: "Delete sandbox" });
  expect(del.previousElementSibling?.tagName).toBe("HR");
  expect(screen.getByText("Claude Code").querySelector("img")).toBeTruthy();
});

test("details ask for the SSH command only when asked, then copy it", async () => {
  const user = userEvent.setup();
  show({}, [{}]);
  await user.click(await screen.findByText("dev-1"));
  await screen.findByText("Can run 1 h 20 m more");
  expect(calls("ssh_command").length).toBe(0);
  await user.click(screen.getByRole("button", { name: "Show command" }));
  expect(await screen.findByText("ssh dev-1@ssh.instabox.sh")).toBeTruthy();
  await user.click(screen.getByRole("button", { name: "Copy SSH command" }));
  expect(await screen.findByRole("button", { name: "Copied" })).toBeTruthy();
  expect(await navigator.clipboard.readText()).toBe("ssh dev-1@ssh.instabox.sh");
  expect(calls("ssh_command").length).toBe(1);
});

test("a failed SSH command shows as an error", async () => {
  show({}, [{}], { ssh_command: () => Promise.reject({ kind: "api", message: "machine_not_running" }) });
  await userEvent.click(await screen.findByText("dev-1"));
  await userEvent.click(screen.getByRole("button", { name: "Show command" }));
  const err = await screen.findByText("machine_not_running");
  expect(err.getAttribute("role")).toBe("alert");
  expect(screen.queryByText("ssh dev-1@ssh.instabox.sh")).toBeNull();
});

test("details: a resize starts at the current size and asks before the restart", async () => {
  show({ tier: "pro" }, [{ free: false, size: "4 vCPU / 8 GB" }]);
  await userEvent.click(await screen.findByText("dev-1"));
  const select = screen.getByLabelText("Size") as HTMLSelectElement;
  expect(select.value).toBe("4 vCPU / 8 GB");
  expect(screen.queryByRole("button", { name: "Apply" })).toBeNull();
  await userEvent.selectOptions(select, "2 vCPU / 4 GB");
  expect(screen.getByText("Restarts the sandbox")).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "Apply" }));
  expect(invoke).toHaveBeenCalledWith("resize", { id: "a", vcpus: 2, memMiB: 4096 });
});

test("details: an applied resize shows no pending change, before or after the new size arrives", async () => {
  const { state } = show({ tier: "pro" }, [{ free: false, size: "4 vCPU / 8 GB" }]);
  await userEvent.click(await screen.findByText("dev-1"));
  await userEvent.selectOptions(screen.getByLabelText("Size"), "2 vCPU / 4 GB");
  await userEvent.click(screen.getByRole("button", { name: "Apply" }));
  await waitFor(() => expect(screen.queryByText("Restarts the sandbox")).toBeNull());
  act(() => emit({ ...state, sandboxes: [{ ...state.sandboxes[0], size: "2 vCPU / 4 GB" }] }));
  expect((screen.getByLabelText("Size") as HTMLSelectElement).value).toBe("2 vCPU / 4 GB");
  expect(screen.queryByText("Restarts the sandbox")).toBeNull();
});

test("details: a failed resize goes back to the current size", async () => {
  show({ tier: "pro" }, [{ free: false, size: "4 vCPU / 8 GB" }], {
    resize: () => {
      throw { kind: "api", message: "no room" };
    },
  });
  await userEvent.click(await screen.findByText("dev-1"));
  const select = screen.getByLabelText("Size") as HTMLSelectElement;
  await userEvent.selectOptions(select, "2 vCPU / 4 GB");
  await userEvent.click(screen.getByRole("button", { name: "Apply" }));
  await waitFor(() => expect(select.value).toBe("4 vCPU / 8 GB"));
  expect(screen.queryByText("Restarts the sandbox")).toBeNull();
});

test("progress buttons never submit the New sandbox form", async () => {
  show({ progress: { kind: "create", step: "creating", machineId: null, ade: "t3", error: { kind: "api", message: "boom" } } });
  await userEvent.click(await screen.findByRole("button", { name: "New sandbox" }));
  await userEvent.type(screen.getByLabelText("Name"), "typed");
  for (const b of screen.getAllByRole("button", { name: "Dismiss" })) await userEvent.click(b);
  expect(calls("dismiss_progress").length).toBeGreaterThan(0);
  expect(calls("create_sandbox").length).toBe(0);
});

test("details: auto sleep applies on change; the free plan cannot change it", async () => {
  show({ tier: "pro", sleepChoices: paidSleep }, [{ free: false }]);
  await userEvent.click(await screen.findByText("dev-1"));
  await userEvent.selectOptions(screen.getByLabelText("Auto sleep"), "600");
  expect(invoke).toHaveBeenCalledWith("set_auto_sleep", { id: "a", seconds: 600 });
  cleanup();
  show({}, [{}]);
  await userEvent.click(await screen.findByText("dev-1"));
  expect((screen.getByLabelText("Auto sleep") as HTMLSelectElement).disabled).toBe(true);
  expect((screen.getByLabelText("Size") as HTMLSelectElement).disabled).toBe(true);
});

test("desktopBehind Update opens the desktop download", async () => {
  show({}, [{ desktopBehind: true, updatePending: true }]);
  await userEvent.click(await screen.findByText("dev-1"));
  expect(screen.getByText("Needs update")).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "Update" }));
  expect(invoke).toHaveBeenCalledWith("open_url", { url: T3_DOWNLOAD_URL });
  expect(calls("reinstall_t3")).toHaveLength(0);
  expect(screen.queryByRole("button", { name: "Reinstall T3" })).toBeNull();
});

test("updatePending offers Update that reinstalls", async () => {
  show({}, [{ updatePending: true, t3Version: "0.1.2" }]);
  await userEvent.click(await screen.findByText("dev-1"));
  expect(screen.getByText("Needs update")).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "Update" }));
  expect(invoke).toHaveBeenCalledWith("reinstall_t3", { id: "a", ade: "t3" });
});

test("details check agent status once for a running sandbox, never for a paused one", async () => {
  show({}, [{}, { id: "b", name: "web", state: "paused" }]);
  await userEvent.click(await screen.findByText("dev-1"));
  await waitFor(() => expect(calls("agent_status").length).toBe(2));
  expect(screen.queryByText("GitHub")).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "Back to sandboxes" }));
  await userEvent.click(screen.getByText("web"));
  expect(screen.getByRole("button", { name: "Resume" })).toBeTruthy();
  expect(calls("agent_status").length).toBe(2);
});

test("details: Pause, Pair again and a confirmed delete", async () => {
  show({}, [{}]);
  await userEvent.click(await screen.findByText("dev-1"));
  await userEvent.click(screen.getByRole("button", { name: "Pause" }));
  expect(invoke).toHaveBeenCalledWith("pause", { id: "a" });
  await userEvent.click(screen.getByRole("button", { name: "Pair again" }));
  expect(invoke).toHaveBeenCalledWith("pair_again", { id: "a", ade: "t3" });
  await userEvent.click(screen.getByRole("button", { name: "Delete sandbox" }));
  expect(calls("delete").length).toBe(0);
  await userEvent.click(screen.getByRole("button", { name: "Confirm delete" }));
  expect(invoke).toHaveBeenCalledWith("delete", { id: "a" });
});

// ---- settings ----

test("settings: account, the ADEs on this Mac and start at login", async () => {
  show(
    {
      ades: [t3Ade, { ...cmuxAde, version: null }, orcaAde, paradevAde],
      orgs: [...sample.orgs, { id: "org-team", name: "team", tier: "pro", personal: false }],
    },
    [{}],
  );
  await userEvent.click(await screen.findByRole("button", { name: "Settings" }));
  expect(screen.getByRole("heading", { name: "This Mac" })).toBeTruthy();
  const ades = within(screen.getByRole("list", { name: "ADEs on this Mac" }));
  expect(ades.getAllByRole("listitem").map((li) => li.textContent)).toEqual([
    "T3 Code0.1.10",
    "cmuxNot found",
    "Orca1.4.269",
    "Paradev2.0.11",
  ]);
  // The account card shows the login; there is no email or avatar to show yet.
  expect(screen.getByText("alex")).toBeTruthy();
  expect(screen.queryByText(/Email/)).toBeNull();
  // The organization is picked in the list's footer, not here.
  expect(screen.queryByLabelText("Organization")).toBeNull();

  const login = screen.getByRole("switch", { name: "Start at login" });
  expect(login.getAttribute("aria-checked")).toBe("false");
  await userEvent.click(login);
  expect(invoke).toHaveBeenCalledWith("set_start_at_login", { on: true });

  await userEvent.click(screen.getByRole("button", { name: "Check again" }));
  expect(invoke).toHaveBeenCalledWith("refresh_t3");

  await userEvent.click(screen.getByRole("button", { name: "Sign out" }));
  expect(invoke).toHaveBeenCalledWith("sign_out");

  await userEvent.click(screen.getByRole("button", { name: "Back to sandboxes" }));
  expect(screen.getByText("dev-1")).toBeTruthy();
});

test("settings: no section for agents on this Mac, whatever their MCP state", async () => {
  show({ mcp: { claudeCode: true, codex: false } }, [{}]);
  await userEvent.click(await screen.findByRole("button", { name: "Settings" }));
  expect(screen.queryByText("Let agents on this Mac manage sandboxes")).toBeNull();
  expect(screen.queryByRole("button", { name: "Add" })).toBeNull();
  expect(screen.queryByText("Added")).toBeNull();
  expect(calls("add_mcp").length).toBe(0);
});

// ---- updates of InstaADE ----

/** The updates of InstaADE 0.1.7, a build that updates itself. */
const updates = (over: Partial<UpdateView> = {}): UpdateView => ({
  current: "0.1.7",
  enabled: true,
  step: "idle",
  version: null,
  percent: null,
  error: null,
  ...over,
});
const offered = updates({ step: "available", version: "0.2.0" });
const openSettings = async () => userEvent.click(await screen.findByRole("button", { name: /^Settings/ }));
const updatesSection = () => within(screen.getByRole("heading", { name: "Updates" }).closest("section") as HTMLElement);

test("updates: the Settings button has a dot only while an update is on offer, and the popover no row", async () => {
  const { state } = show({ update: updates() }, [{}]);
  const gear = await screen.findByRole("button", { name: "Settings" });
  expect(gear.querySelector(".badge")).toBeNull();

  act(() => emit({ ...state, update: offered }));
  const dotted = screen.getByRole("button", { name: "Settings, update available" });
  expect(dotted.querySelector(".badge")).toBeTruthy();
  // Nothing pops up in the popover itself.
  expect(screen.queryByText(/is available/)).toBeNull();
  expect(screen.queryByRole("button", { name: "Update" })).toBeNull();

  act(() => emit({ ...state, update: updates({ step: "downloading", version: "0.2.0", percent: 10 }) }));
  expect(screen.getByRole("button", { name: "Settings, update available" })).toBeTruthy();
  act(() => emit({ ...state, update: updates({ step: "upToDate" }) }));
  expect(screen.getByRole("button", { name: "Settings" }).querySelector(".badge")).toBeNull();
});

test("updates: Check for updates shows checking, then up to date", async () => {
  const { state } = show({ update: updates() }, [{}]);
  await openSettings();
  const section = updatesSection();
  const versionRow = section.getByText("Version").closest(".group-row") as HTMLElement;
  expect(within(versionRow).getByText("0.1.7")).toBeTruthy();
  expect(section.getByText("Checks every few hours")).toBeTruthy();
  await userEvent.click(section.getByRole("button", { name: "Check for updates" }));
  expect(invoke).toHaveBeenCalledWith("check_for_updates");

  act(() => emit({ ...state, update: updates({ step: "checking" }) }));
  expect(updatesSection().getByText("Checking for updates…")).toBeTruthy();
  expect((updatesSection().getByRole("button", { name: "Check for updates" }) as HTMLButtonElement).disabled).toBe(true);

  act(() => emit({ ...state, update: updates({ step: "upToDate" }) }));
  expect(updatesSection().getByText("Up to date")).toBeTruthy();
  expect((updatesSection().getByRole("button", { name: "Check for updates" }) as HTMLButtonElement).disabled).toBe(false);
});

test("updates: a failed check shows why inline", async () => {
  const message = "Could not check for updates: offline";
  show({ update: updates({ step: "checkFailed", error: { kind: "other", message } }) }, [{}], {
    check_for_updates: () => Promise.reject({ kind: "other", message }),
  });
  await openSettings();
  const row = updatesSection().getByText(message).closest(".group-row") as HTMLElement;
  expect(row.getAttribute("role")).toBe("alert");
  await userEvent.click(updatesSection().getByRole("button", { name: "Check for updates" }));
  // The rejection shows in state.update only.
  await act(() => new Promise((r) => setTimeout(r, 0)));
  expect(screen.getAllByText(message).length).toBe(1);
});

test("updates: Update downloads with progress, then installs", async () => {
  const { state } = show({ update: offered }, [{}]);
  await openSettings();
  expect(updatesSection().getByText("0.2.0 is available")).toBeTruthy();
  // The update is the one action: no check while it is on offer.
  expect(updatesSection().queryByRole("button", { name: "Check for updates" })).toBeNull();
  await userEvent.click(updatesSection().getByRole("button", { name: "Update" }));
  expect(invoke).toHaveBeenCalledWith("install_update");

  // The back end reports the download. The size may come later.
  act(() => emit({ ...state, update: { ...offered, step: "downloading" } }));
  expect(updatesSection().getByText("Downloading 0.2.0")).toBeTruthy();
  expect(updatesSection().getByRole("progressbar", { name: "Update progress" }).getAttribute("aria-valuenow")).toBeNull();
  expect(updatesSection().queryByRole("button")).toBeNull();
  act(() => emit({ ...state, update: { ...offered, step: "downloading", percent: 42 } }));
  expect(updatesSection().getByText("Downloading 0.2.0 (42%)")).toBeTruthy();
  expect(updatesSection().getByRole("progressbar", { name: "Update progress" }).getAttribute("aria-valuenow")).toBe("42");
  act(() => emit({ ...state, update: { ...offered, step: "installing" } }));
  expect(updatesSection().getByText("Installing 0.2.0, then restarting")).toBeTruthy();
  expect(calls("install_update").length).toBe(1);
});

test("updates: a failed update shows why with Retry, and no second banner", async () => {
  const error = { kind: "other" as const, message: "Could not update to 0.2.0: signature mismatch" };
  show({ update: { ...offered, step: "failed", error } }, [{}], { install_update: () => Promise.reject(error) });
  await openSettings();
  const row = updatesSection().getByText(error.message).closest(".group-row") as HTMLElement;
  expect(row.getAttribute("role")).toBe("alert");
  await userEvent.click(within(row).getByRole("button", { name: "Retry" }));
  expect(calls("install_update").length).toBe(1);
  // The rejection shows in state.update only.
  await act(() => new Promise((r) => setTimeout(r, 0)));
  expect(screen.getAllByText(error.message).length).toBe(1);
});

test("updates: a dev or preview build shows its version and never checks", async () => {
  show({ update: updates({ enabled: false }) }, [{}]);
  await openSettings();
  const section = updatesSection();
  const versionRow = section.getByText("Version").closest(".group-row") as HTMLElement;
  expect(within(versionRow).getByText("0.1.7")).toBeTruthy();
  expect(section.getByText("Updates are only for released builds")).toBeTruthy();
  const check = section.getByRole("button", { name: "Check for updates" }) as HTMLButtonElement;
  expect(check.disabled).toBe(true);
  await userEvent.click(check);
  expect(calls("check_for_updates").length).toBe(0);
});

test("updates: signed out, the sign-in page offers the update and installs it", async () => {
  const { state } = show({ ...noT3, signedIn: false, login: null, update: offered });
  await screen.findByRole("button", { name: "Sign in with GitHub" });
  const card = () => screen.getByRole("group", { name: "Update" });
  expect(within(card()).getByText("0.2.0 is available")).toBeTruthy();
  await userEvent.click(within(card()).getByRole("button", { name: "Update" }));
  expect(invoke).toHaveBeenCalledWith("install_update");
  act(() => emit({ ...state, update: { ...offered, step: "downloading", percent: 42 } }));
  expect(within(card()).getByText("Downloading 0.2.0 (42%)")).toBeTruthy();
});

test("updates: signed out with nothing on offer, the sign-in page shows no update", async () => {
  show({ ...noT3, signedIn: false, login: null, update: updates({ step: "upToDate" }) });
  await screen.findByRole("button", { name: "Sign in with GitHub" });
  expect(screen.queryByRole("group", { name: "Update" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Check for updates" })).toBeNull();
});

// ---- first run ----

test("sign-in asks for no ADE", async () => {
  show({ ...noT3, signedIn: false, login: null });
  await userEvent.click(await screen.findByRole("button", { name: "Sign in with GitHub" }));
  expect(invoke).toHaveBeenCalledWith("sign_in", { provider: "github" });
  expect(screen.getByText("Run any ADE on an InstaBox Sandbox.")).toBeTruthy();
  expect(screen.getByRole("img", { name: "InstaADE" })).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "Sign in with Google" }));
  expect(invoke).toHaveBeenCalledWith("sign_in", { provider: "google" });
  expect(screen.queryByText(/T3 Code/)).toBeNull();
  expect(screen.queryByRole("button", { name: "Settings" })).toBeNull();
  expect(screen.queryByRole("button", { name: /Switch ADE/ })).toBeNull();
});

test("sign-in screen shows the device code", async () => {
  show({ signedIn: false, signInCode: "ABCD-1234" });
  expect(await screen.findByText("ABCD-1234")).toBeTruthy();
  expect(screen.getByText(/Waiting for sign-in in your browser/)).toBeTruthy();
});

test("Check again shows that it is checking, then that it checked", async () => {
  let finish: () => void = () => {};
  show(noT3, [{}], { refresh_t3: () => new Promise<void>((r) => (finish = r)) });
  await userEvent.click(await screen.findByRole("button", { name: "Check again" }));
  expect((screen.getByRole("button", { name: "Checking…" }) as HTMLButtonElement).disabled).toBe(true);
  await act(async () => finish());
  expect(await screen.findByRole("button", { name: "Checked" })).toBeTruthy();
  expect(await screen.findByRole("button", { name: "Check again" }, { timeout: 2500 })).toBeTruthy();
});

const windowsAgent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0";

test("on Windows the copy says this PC, not this Mac", async () => {
  const ua = vi.spyOn(navigator, "userAgent", "get").mockReturnValue(windowsAgent);
  try {
    show(noT3, [{}]);
    expect(await screen.findByText("T3 Code is not on this PC")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(screen.getByRole("heading", { name: "This PC" })).toBeTruthy();
    expect(screen.getByRole("list", { name: "ADEs on this PC" })).toBeTruthy();
    expect(screen.queryByText(/this Mac/i)).toBeNull();
  } finally {
    ua.mockRestore();
  }
});

test("T3 mode without T3 Code on this Mac shows the card above the list", async () => {
  // A sandbox with T3 puts a v0.1.x user in T3 mode even when T3 Code is not found.
  show(noT3, [{}]);
  expect(await screen.findByText("T3 Code is not on this Mac")).toBeTruthy();
  expect(screen.getByText("dev-1")).toBeTruthy();
  // Check again is a text button, so Download stays the one action that stands out.
  expect(screen.getByRole("button", { name: "Check again" }).classList.contains("text")).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Download T3 Code" }));
  expect(invoke).toHaveBeenCalledWith("open_url", { url: T3_DOWNLOAD_URL });
  await userEvent.click(screen.getByRole("button", { name: "Check again" }));
  expect(invoke).toHaveBeenCalledWith("refresh_t3");
});

// ---- the ADE switcher ----

test("the switcher lists Instabox, the ADEs on this Mac and the ones coming soon", async () => {
  show({ ades: [t3Ade, cmuxAde, orcaAde] }, [{}, { id: "b", name: "web", ades: ["t3", "cmux"] }]);
  await screen.findByText("dev-1");
  expect(switcher().getAttribute("aria-label")).toBe("ADE: T3 Code. Switch ADE");
  await userEvent.click(switcher());
  const menu = within(screen.getByRole("menu", { name: "Open sandboxes in" }));
  const items = menu.getAllByRole("menuitemradio");
  expect(items.map((b) => b.textContent)).toEqual([
    "InstaBox" + "Sandboxes only, no ADE",
    "T3 Code" + "0.1.10 on this Mac · on 2 sandboxes",
    "cmux" + "0.64.25 on this Mac · on 1 sandbox",
    "Orca" + "1.4.269 on this Mac",
  ]);
  expect(items.map((b) => b.getAttribute("aria-checked"))).toEqual(["false", "true", "false", "false"]);
  expect((items[3] as HTMLButtonElement).disabled).toBe(false);
  // The checked mode has the focus, so the keyboard can walk the menu.
  expect(document.activeElement).toBe(items[1]);
  await userEvent.keyboard("{Escape}");
  expect(screen.queryByRole("menu")).toBeNull();
  expect(document.activeElement).toBe(switcher());
});

test("the switcher on Windows has no cmux", async () => {
  const ua = vi.spyOn(navigator, "userAgent", "get").mockReturnValue(windowsAgent);
  try {
    // The back end lists only the ADEs that run here.
    show({ ades: [t3Ade] }, [{}]);
    await userEvent.click(await screen.findByRole("button", { name: /Switch ADE/ }));
    const names = within(screen.getByRole("menu")).getAllByRole("menuitemradio").map((b) => b.textContent);
    expect(names.some((n) => n?.includes("cmux"))).toBe(false);
    expect(names[1]).toBe("T3 Code" + "0.1.10 on this PC · on 1 sandbox");
    // A cmux pick saved on another computer falls back to plain Instabox here.
    cleanup();
    picked("cmux");
    show({ ades: [t3Ade] }, [{}]);
    expect((await screen.findByRole("button", { name: /Switch ADE/ })).getAttribute("aria-label")).toBe("ADE: InstaBox. Switch ADE");
  } finally {
    ua.mockRestore();
  }
});

test("picking an ADE switches the background and the row actions, and is remembered", async () => {
  show(mac, [{}, { id: "b", name: "web", ades: ["t3", "cmux"] }]);
  await screen.findByText("dev-1");
  expect(popover().dataset.mode).toBe("t3");
  expect(theme()).toBe("t3");
  await userEvent.click(switcher());
  await userEvent.click(screen.getByRole("menuitemradio", { name: /cmux/ }));
  expect(localStorage.getItem(modeKey("alex"))).toBe("cmux");
  expect(popover().dataset.mode).toBe("cmux");
  expect(theme()).toBe("cmux");
  expect(switcher().getAttribute("aria-label")).toBe("ADE: cmux. Switch ADE");
  // A switch shows no notice: each row's Install or Open says enough.
  expect(screen.queryByText(/Switched to/)).toBeNull();
  // All connected ADEs stay visible; Connect targets the newly selected cmux.
  expect(within(rowOf("dev-1")).getByRole("button", { name: "Connect" }).title).toBe("Connect to cmux");
  expect(within(rowOf("dev-1")).getByRole("img", { name: "T3 Code" })).toBeTruthy();
  expect(within(rowOf("web")).getByRole("img", { name: "cmux" })).toBeTruthy();
  expect(within(rowOf("web")).queryByRole("button", { name: "Connect" })).toBeNull();

  // Plain Instabox has T3's plain black background.
  await userEvent.click(switcher());
  await userEvent.click(screen.getByRole("menuitemradio", { name: /InstaBox/ }));
  expect(popover().dataset.mode).toBe("instabox");
  expect(theme()).toBe("t3");
  expect(localStorage.getItem(modeKey("alex"))).toBe("instabox");
});

test("cmux rows: Open runs open_in_t3 with cmux, Install sets it up, neither adds a notice", async () => {
  picked("cmux");
  show(mac, [
    { id: "a", name: "dev-1", ades: ["t3"] },
    { id: "b", name: "web", ades: ["cmux"], state: "paused" },
    { id: "c", name: "busy", ades: ["cmux"], state: "waking" },
  ]);
  const install = within(await screen.findByText("dev-1").then(() => rowOf("dev-1")));
  await userEvent.click(install.getByRole("button", { name: "Connect" }));
  expect(invoke).toHaveBeenCalledWith("connect_sandbox", { id: "a", ade: "cmux" });
  // cmux asks in its own window. The popover adds no notice once the action is done.
  await act(async () => {});
  expect(screen.queryByText(/Confirm the/)).toBeNull();

  await userEvent.click(screen.getByText("web"));
  await userEvent.click(screen.getByRole("button", { name: "Open in cmux" }));
  expect(invoke).toHaveBeenCalledWith("open_in_t3", { id: "b", ade: "cmux" });
  await act(async () => {});
  expect(screen.queryByText(/Confirm the/)).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "Back to sandboxes" }));
  await userEvent.click(screen.getByText("busy"));
  expect((screen.getByRole("button", { name: "Open in cmux" }) as HTMLButtonElement).disabled).toBe(true);
  await userEvent.click(screen.getByRole("button", { name: "Back to sandboxes" }));

  // The menu offers the same, with no T3 pairing or reinstall.
  await openMenu("web");
  expect(menuItems()).toEqual([
    "Open in cmux",
    "Resume",
    "Resize4 vCPU / 8 GB",
    "Auto sleepAfter 30 min",
    "Copy SSH command",
    "Open in Terminal",
    "Delete sandbox",
  ]);
});

test("installing cmux shows a progress line with no time left", async () => {
  picked("cmux");
  show({ ...mac, progress: { kind: "connect", step: "installing", machineId: "a", ade: "cmux", error: null } }, [{ ades: [] }]);
  const row = within(await screen.findByText("dev-1").then(() => rowOf("dev-1")));
  expect(row.getByText("Installing cmux, then opening")).toBeTruthy();
  expect(row.getByRole("progressbar")).toBeTruthy();
  expect(row.queryByRole("button", { name: "Install cmux" })).toBeNull();
  // No time estimates anywhere.
  expect(document.body.textContent).not.toMatch(/~|\d+\s*s\b|left/);
  act(() => emit({ ...sample, ...mac, sandboxes: [{ ...box, ades: [] }], progress: { kind: "connect", step: "done", machineId: "a", ade: "cmux", error: null } }));
  expect(row.getByText("Opening cmux")).toBeTruthy();
});

test("plain Instabox shows existing connections in details without selecting an ADE", async () => {
  picked("instabox");
  show({ ...mac, tier: "pro" }, [{ free: false }, { id: "b", name: "web", connected: false, ades: [] }]);
  await screen.findByText("dev-1");
  expect(popover().dataset.mode).toBe("instabox");
  for (const name of [/^Open in (T3|cmux)/, /^Install/, /^Connect/, /^Update$/]) expect(screen.queryByRole("button", { name })).toBeNull();
  expect(screen.queryByText(/here yet|T3 Code 0\.1\.10/)).toBeNull();
  expect(within(rowOf("dev-1")).getByText("Running")).toBeTruthy();

  await openMenu("dev-1");
  expect(menuItems()).toEqual(["Pause", "Resize4 vCPU / 8 GB", "Auto sleepAfter 30 min", "Copy SSH command", "Open in Terminal", "Delete sandbox"]);
  await userEvent.keyboard("{Escape}");

  await userEvent.click(screen.getByText("dev-1"));
  expect(screen.getByRole("button", { name: "Open in T3 Code" })).toBeTruthy();
  for (const name of [/^Install/, "Pair again", "Reinstall T3", "Reconnect"]) expect(screen.queryByRole("button", { name })).toBeNull();
  expect(screen.queryByText("ADEs on this sandbox")).toBeNull();
  expect(screen.queryByText("cmux mode")).toBeNull();

  // A new sandbox gets no ADE.
  await userEvent.click(screen.getByRole("button", { name: "Back to sandboxes" }));
  await userEvent.click(screen.getByRole("button", { name: "New sandbox" }));
  expect(screen.queryByText(/Installs T3|Sets the sandbox up/)).toBeNull();
  await userEvent.type(screen.getByLabelText("Name"), "dev-2");
  await userEvent.click(screen.getByRole("button", { name: "Create sandbox" }));
  expect(invoke).toHaveBeenCalledWith("create_sandbox", expect.objectContaining({ name: "dev-2", ade: undefined }));
  await waitFor(() => expect(screen.queryByLabelText("Name")).toBeNull());
  expect(screen.queryByText(/Confirm/)).toBeNull();
});

test("a new sandbox in cmux mode sets cmux up", async () => {
  picked("cmux");
  show({ ...mac, tier: "pro" }, [{}]);
  await userEvent.click(await screen.findByRole("button", { name: "New sandbox" }));
  expect(screen.getByText("Sets the sandbox up for cmux. Then it opens in cmux.")).toBeTruthy();
  await userEvent.type(screen.getByLabelText("Name"), "dev-2");
  await userEvent.click(screen.getByRole("button", { name: "Create sandbox" }));
  expect(invoke).toHaveBeenCalledWith("create_sandbox", expect.objectContaining({ ade: "cmux" }));
  await act(async () => {});
  expect(screen.queryByText(/Confirm the/)).toBeNull();
});

const orcaNotice = (name: string) =>
  `In Orca, open Settings, then SSH Hosts. Click Import if instabox-${name} is not listed, then connect it and open /data/home.`;
const ORCA_NEXT = orcaNotice("dev-1");

test("Orca mode: Connect writes the host and says where to connect it in Orca", async () => {
  picked("orca");
  show({ ades: [t3Ade, cmuxAde, orcaAde] }, [{ id: "a", name: "dev-1", ades: ["t3"] }, { id: "b", name: "web", ades: ["orca"] }], {
    connect_sandbox: () => ORCA_NEXT,
    open_in_t3: () => orcaNotice("web"),
  });
  await screen.findByText("dev-1");
  expect(switcher().getAttribute("aria-label")).toBe("ADE: Orca. Switch ADE");
  expect(popover().dataset.mode).toBe("orca");
  expect(theme()).toBe("orca");
  // A row without Orca offers Connect; one with it shows Orca's logo.
  const connect = within(rowOf("dev-1")).getByRole("button", { name: "Connect" });
  expect(connect.title).toBe("Connect to Orca");
  expect(within(rowOf("web")).getByRole("img", { name: "Orca" })).toBeTruthy();
  expect(within(rowOf("web")).queryByRole("button", { name: "Connect" })).toBeNull();
  await userEvent.click(connect);
  expect(invoke).toHaveBeenCalledWith("connect_sandbox", { id: "a", ade: "orca" });
  expect((await screen.findByRole("status")).textContent).toBe(ORCA_NEXT);

  // Details open it in Orca, and say again where to connect it.
  await userEvent.click(screen.getByText("web"));
  expect(screen.queryByRole("status")).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "Open in Orca" }));
  expect(invoke).toHaveBeenCalledWith("open_in_t3", { id: "b", ade: "orca" });
  expect((await screen.findByRole("status")).textContent).toBe(orcaNotice("web"));
  // Orca pairs nothing: no T3 pairing or reinstall.
  for (const name of ["Pair again", "Reinstall T3"]) expect(screen.queryByRole("button", { name })).toBeNull();
});

test("Orca mode: a failed open shows the error, not the notice", async () => {
  picked("orca");
  show({ ades: [t3Ade, orcaAde] }, [{ id: "b", name: "web", ades: ["orca"] }], {
    open_in_t3: () => Promise.reject({ kind: "other", message: "Could not verify the Orca app. Install it again from its official download." }),
  });
  await userEvent.click(await screen.findByText("web"));
  await userEvent.click(screen.getByRole("button", { name: "Open in Orca" }));
  expect(await screen.findByText("Could not verify the Orca app. Install it again from its official download.")).toBeTruthy();
  expect(screen.queryByRole("status")).toBeNull();
});

test("a new sandbox in Orca mode sets Orca up and says where to connect it", async () => {
  picked("orca");
  show({ ades: [t3Ade, orcaAde], tier: "pro" }, [{}], { create_sandbox: () => ORCA_NEXT });
  await userEvent.click(await screen.findByRole("button", { name: "New sandbox" }));
  expect(screen.getByText("Sets the sandbox up for Orca. Then it opens in Orca.")).toBeTruthy();
  await userEvent.type(screen.getByLabelText("Name"), "dev-2");
  await userEvent.click(screen.getByRole("button", { name: "Create sandbox" }));
  expect(invoke).toHaveBeenCalledWith("create_sandbox", expect.objectContaining({ ade: "orca" }));
  expect((await screen.findByRole("status")).textContent).toBe(ORCA_NEXT);
});

test("a notice from a tray action shows in the popover like its own", async () => {
  picked("orca");
  show({ ades: [t3Ade, orcaAde] }, [{ id: "b", name: "web", ades: ["orca"] }]);
  await screen.findByText("web");
  expect(screen.queryByRole("status")).toBeNull();
  // The tray's Open in Orca ran in the back end.
  act(() => emitNotice(orcaNotice("web")));
  expect(screen.getByRole("status").textContent).toBe(orcaNotice("web"));
  // The next action clears it, as it clears the popover's own notices.
  await userEvent.click(screen.getByText("web"));
  expect(screen.queryByRole("status")).toBeNull();
});

test("an Orca pick holds on Windows too, where the back end lists Orca", async () => {
  const ua = vi.spyOn(navigator, "userAgent", "get").mockReturnValue(windowsAgent);
  try {
    picked("orca");
    show({ ades: [t3Ade, orcaAde] }, [{}]);
    expect((await screen.findByRole("button", { name: /Switch ADE/ })).getAttribute("aria-label")).toBe("ADE: Orca. Switch ADE");
    // Without Orca on this computer, the pick falls back to plain InstaBox.
    cleanup();
    picked("orca");
    show({ ades: [t3Ade] }, [{}]);
    expect((await screen.findByRole("button", { name: /Switch ADE/ })).getAttribute("aria-label")).toBe("ADE: InstaBox. Switch ADE");
  } finally {
    ua.mockRestore();
  }
});

// ---- Paradev ----

const PARADEV_NOTICE = "The SSH address is copied. In Paradev, click Set up a host over SSH and paste it.";

test("Paradev on a Mac: one entry in the switcher, with no Superset and nothing coming soon", async () => {
  show({ ades: [t3Ade, cmuxAde, orcaAde, paradevAde] }, [{}, { id: "b", name: "web", ades: ["paradev"], connected: false }]);
  await screen.findByText("dev-1");
  await userEvent.click(switcher());
  const items = within(screen.getByRole("menu", { name: "Open sandboxes in" })).getAllByRole("menuitemradio");
  expect(items.map((b) => b.textContent)).toEqual([
    "InstaBox" + "Sandboxes only, no ADE",
    "T3 Code" + "0.1.10 on this Mac · on 1 sandbox",
    "cmux" + "0.64.25 on this Mac",
    "Orca" + "1.4.269 on this Mac",
    "Paradev" + "2.0.11 on this Mac · on 1 sandbox",
  ]);
  expect(document.body.textContent).not.toMatch(/Superset|Coming soon/);
  await userEvent.click(screen.getByRole("menuitemradio", { name: /Paradev/ }));
  expect(localStorage.getItem(modeKey("alex"))).toBe("paradev");
  expect(popover().dataset.mode).toBe("paradev");
  expect(theme()).toBe("paradev");
  expect(switcher().getAttribute("aria-label")).toBe("ADE: Paradev. Switch ADE");
});

test("Paradev rows: Install says what to do in Paradev, Open opens it with no notice", async () => {
  picked("paradev");
  show(
    { ades: [t3Ade, cmuxAde, paradevAde] },
    [
      { id: "a", name: "dev-1", ades: ["t3"] },
      // Marked once its host ran. Paradev pairs nothing from here, so it is not "connected".
      { id: "b", name: "web", ades: ["paradev"], connected: false, state: "paused" },
    ],
    { connect_sandbox: () => PARADEV_NOTICE, open_in_t3: () => null },
  );
  const dev = within(await screen.findByText("dev-1").then(() => rowOf("dev-1")));
  const install = dev.getByRole("button", { name: "Connect" });
  expect(install.title).toBe("Connect to Paradev");
  await userEvent.click(install);
  expect(invoke).toHaveBeenCalledWith("connect_sandbox", { id: "a", ade: "paradev" });
  // The back end's notice: the user finishes the setup in Paradev.
  expect((await screen.findByRole("status")).textContent).toBe(PARADEV_NOTICE);

  const web = within(rowOf("web"));
  expect(web.getByRole("img", { name: "Paradev" })).toBeTruthy();
  expect(web.getByText("connected")).toBeTruthy();
  expect(web.queryByRole("button", { name: "Connect" })).toBeNull();
  await userEvent.click(screen.getByText("web"));
  // A new screen drops the notice.
  expect(screen.queryByText(PARADEV_NOTICE)).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "Open in Paradev" }));
  expect(invoke).toHaveBeenCalledWith("open_in_t3", { id: "b", ade: "paradev" });
  await act(async () => {});
  expect(screen.queryByText(PARADEV_NOTICE)).toBeNull();
  // No T3 pairing or reinstall; Reconnect checks the host again.
  expect(screen.queryByRole("button", { name: /Reinstall|Pair again/ })).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "Reconnect" }));
  expect(invoke).toHaveBeenCalledWith("connect_sandbox", { id: "b", ade: "paradev" });
  expect((await screen.findByRole("status")).textContent).toBe(PARADEV_NOTICE);
  await userEvent.click(screen.getByRole("button", { name: "Back to sandboxes" }));

  await openMenu("web");
  expect(menuItems()).toEqual([
    "Open in Paradev",
    "Resume",
    "Resize4 vCPU / 8 GB",
    "Auto sleepAfter 30 min",
    "Copy SSH command",
    "Open in Terminal",
    "Delete sandbox",
  ]);
  await userEvent.keyboard("{Escape}");
  await openMenu("dev-1");
  expect(menuItems()[0]).toBe("Install Paradev");
});

test("a new sandbox in Paradev mode says Paradev finishes the setup", async () => {
  picked("paradev");
  show({ ades: [t3Ade, paradevAde], tier: "pro" }, [{}], { create_sandbox: () => PARADEV_NOTICE });
  await userEvent.click(await screen.findByRole("button", { name: "New sandbox" }));
  expect(screen.getByText("Copies the SSH address of the new sandbox and opens Paradev, where you set it up as a host.")).toBeTruthy();
  await userEvent.type(screen.getByLabelText("Name"), "dev-2");
  await userEvent.click(screen.getByRole("button", { name: "Create sandbox" }));
  expect(invoke).toHaveBeenCalledWith("create_sandbox", expect.objectContaining({ ade: "paradev" }));
  expect((await screen.findByRole("status")).textContent).toBe(PARADEV_NOTICE);
});

test("Windows has no Paradev: not in the switcher, a Paradev pick falls back, details say so", async () => {
  const ua = vi.spyOn(navigator, "userAgent", "get").mockReturnValue(windowsAgent);
  try {
    // The back end lists only the ADEs that run here.
    show({ ades: [t3Ade] }, [{ ades: ["t3", "paradev"] }]);
    await userEvent.click(await screen.findByRole("button", { name: /Switch ADE/ }));
    const names = within(screen.getByRole("menu")).getAllByRole("menuitemradio").map((b) => b.textContent);
    expect(names.some((n) => n?.includes("Paradev"))).toBe(false);
    await userEvent.keyboard("{Escape}");
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(within(screen.getByRole("list", { name: "ADEs on this PC" })).queryByText("Paradev")).toBeNull();
    cleanup();

    // A Paradev pick saved on a Mac falls back to plain Instabox here.
    picked("paradev");
    show({ ades: [t3Ade] }, [{ ades: ["t3", "paradev"] }]);
    expect((await screen.findByRole("button", { name: /Switch ADE/ })).getAttribute("aria-label")).toBe("ADE: InstaBox. Switch ADE");
    await userEvent.click(screen.getByText("dev-1"));
    const section = screen.getByRole("region", { name: "Connected ADE" });
    const paradev = within(within(section).getByText("Paradev").closest(".connected-ade-row") as HTMLElement);
    expect(paradev.queryByRole("button", { name: "Open in Paradev" })).toBeNull();
    expect(paradev.getByText("Not available on this PC")).toBeTruthy();
  } finally {
    ua.mockRestore();
  }
});

// ---- Paseo ----

const paseoAde: AdeView = {
  id: "paseo",
  name: "Paseo",
  shortName: "Paseo",
  version: "0.17.4",
  downloadUrl: "https://github.com/getpaseo/paseo/releases/latest",
};

const paseoNotice = (name: string) =>
  `In Paseo, finish or skip its setup if it shows one. Then add this sandbox once: open Settings, click Switch host, Add host, then Remote SSH, and enter ssh://instabox-${name}.`;

test("Paseo in the switcher: its own entry, look and saved pick", async () => {
  show({ ades: [t3Ade, cmuxAde, orcaAde, paradevAde, paseoAde] }, [{}, { id: "b", name: "web", ades: ["paseo"], connected: false }]);
  await screen.findByText("dev-1");
  await userEvent.click(switcher());
  const items = within(screen.getByRole("menu", { name: "Open sandboxes in" })).getAllByRole("menuitemradio");
  expect(items.map((b) => b.textContent)).toEqual([
    "InstaBox" + "Sandboxes only, no ADE",
    "T3 Code" + "0.1.10 on this Mac · on 1 sandbox",
    "cmux" + "0.64.25 on this Mac",
    "Orca" + "1.4.269 on this Mac",
    "Paradev" + "2.0.11 on this Mac",
    "Paseo" + "0.17.4 on this Mac · on 1 sandbox",
  ]);
  await userEvent.click(screen.getByRole("menuitemradio", { name: /Paseo/ }));
  expect(localStorage.getItem(modeKey("alex"))).toBe("paseo");
  expect(popover().dataset.mode).toBe("paseo");
  expect(theme()).toBe("paseo");
  expect(switcher().getAttribute("aria-label")).toBe("ADE: Paseo. Switch ADE");
});

test("Paseo rows: Connect sets it up and says where to add the host, Open says it again", async () => {
  picked("paseo");
  show(
    { ades: [t3Ade, paseoAde] },
    [
      { id: "a", name: "dev-1", ades: ["t3"] },
      // Paseo pairs nothing from here, so a sandbox with it is not "connected".
      { id: "b", name: "web", ades: ["paseo"], connected: false },
    ],
    { connect_sandbox: () => paseoNotice("dev-1"), open_in_t3: () => paseoNotice("web") },
  );
  await screen.findByText("dev-1");
  const connect = within(rowOf("dev-1")).getByRole("button", { name: "Connect" });
  expect(connect.title).toBe("Connect to Paseo");
  const web = within(rowOf("web"));
  expect(web.getByRole("img", { name: "Paseo" })).toBeTruthy();
  expect(web.getByText("connected")).toBeTruthy();
  expect(web.queryByRole("button", { name: "Connect" })).toBeNull();
  await userEvent.click(connect);
  expect(invoke).toHaveBeenCalledWith("connect_sandbox", { id: "a", ade: "paseo" });
  expect((await screen.findByRole("status")).textContent).toBe(paseoNotice("dev-1"));

  // Details open it in Paseo and say again where to add it. Reconnect runs the setup again.
  await userEvent.click(screen.getByText("web"));
  expect(screen.queryByRole("status")).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "Open in Paseo" }));
  expect(invoke).toHaveBeenCalledWith("open_in_t3", { id: "b", ade: "paseo" });
  expect((await screen.findByRole("status")).textContent).toBe(paseoNotice("web"));
  expect(screen.queryByRole("button", { name: /Reinstall|Pair again/ })).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "Reconnect" }));
  expect(invoke).toHaveBeenCalledWith("connect_sandbox", { id: "b", ade: "paseo" });
  await userEvent.click(screen.getByRole("button", { name: "Back to sandboxes" }));
  await openMenu("dev-1");
  expect(menuItems()[0]).toBe("Install Paseo");
});

test("a new sandbox in Paseo mode says InstaADE runs the daemon and Paseo adds the host", async () => {
  picked("paseo");
  show({ ades: [t3Ade, paseoAde], tier: "pro" }, [{}], { create_sandbox: () => paseoNotice("dev-2") });
  await userEvent.click(await screen.findByRole("button", { name: "New sandbox" }));
  expect(screen.getByText("Installs the Paseo daemon on the sandbox and opens Paseo, where you add the sandbox once.")).toBeTruthy();
  await userEvent.type(screen.getByLabelText("Name"), "dev-2");
  await userEvent.click(screen.getByRole("button", { name: "Create sandbox" }));
  expect(invoke).toHaveBeenCalledWith("create_sandbox", expect.objectContaining({ ade: "paseo" }));
  expect((await screen.findByRole("status")).textContent).toBe(paseoNotice("dev-2"));
});

test("a Paseo pick holds on Windows too, where the back end lists Paseo", async () => {
  const ua = vi.spyOn(navigator, "userAgent", "get").mockReturnValue(windowsAgent);
  try {
    picked("paseo");
    show({ ades: [t3Ade, orcaAde, paseoAde] }, [{}]);
    expect((await screen.findByRole("button", { name: /Switch ADE/ })).getAttribute("aria-label")).toBe("ADE: Paseo. Switch ADE");
    // Without Paseo on this computer, the pick falls back to plain InstaBox.
    cleanup();
    picked("paseo");
    show({ ades: [t3Ade, orcaAde] }, [{}]);
    expect((await screen.findByRole("button", { name: /Switch ADE/ })).getAttribute("aria-label")).toBe("ADE: InstaBox. Switch ADE");
  } finally {
    ua.mockRestore();
  }
});

// ---- Herdr ----

const herdrAde: AdeView = {
  id: "herdr",
  name: "Herdr",
  shortName: "Herdr",
  version: "OpenSSH 10.3p1",
  downloadUrl: "https://example.test/openssh",
};

test("Herdr in the switcher says what it needs on this Mac, and takes Herdr's look", async () => {
  show({ ades: [t3Ade, cmuxAde, orcaAde, paradevAde, herdrAde] }, [{}, { id: "b", name: "web", ades: ["herdr"], connected: false }]);
  await screen.findByText("dev-1");
  await userEvent.click(switcher());
  const items = within(screen.getByRole("menu", { name: "Open sandboxes in" })).getAllByRole("menuitemradio");
  expect(items.map((b) => b.textContent)).toEqual([
    "InstaBox" + "Sandboxes only, no ADE",
    "T3 Code" + "0.1.10 on this Mac · on 1 sandbox",
    "cmux" + "0.64.25 on this Mac",
    "Orca" + "1.4.269 on this Mac",
    "Paradev" + "2.0.11 on this Mac",
    "Herdr" + "OpenSSH 10.3p1 on this Mac · on 1 sandbox",
  ]);
  await userEvent.click(screen.getByRole("menuitemradio", { name: /Herdr/ }));
  expect(localStorage.getItem(modeKey("alex"))).toBe("herdr");
  expect(popover().dataset.mode).toBe("herdr");
  expect(theme()).toBe("herdr");
  expect(switcher().getAttribute("aria-label")).toBe("ADE: Herdr. Switch ADE");
  expect(switcher().querySelector('.ade-logo[data-ade="herdr"] img')).toBeTruthy();

  await userEvent.click(screen.getByRole("button", { name: "Settings" }));
  const ades = within(screen.getByRole("list", { name: "ADEs on this Mac" }));
  expect(ades.getAllByRole("listitem").map((li) => li.textContent).at(-1)).toBe("HerdrOpenSSH 10.3p1");
});

test("Herdr rows: Install sets it up and Open opens it, neither with a notice", async () => {
  picked("herdr");
  show(
    { ades: [t3Ade, herdrAde] },
    [
      { id: "a", name: "dev-1", ades: ["t3"] },
      // Herdr pairs nothing from here, so it is not "connected", and any computer opens it.
      { id: "b", name: "web", ades: ["herdr"], connected: false, state: "paused" },
    ],
    { connect_sandbox: () => null, open_in_t3: () => null },
  );
  const dev = within(await screen.findByText("dev-1").then(() => rowOf("dev-1")));
  const install = dev.getByRole("button", { name: "Connect" });
  expect(install.title).toBe("Connect to Herdr");
  await userEvent.click(install);
  expect(invoke).toHaveBeenCalledWith("connect_sandbox", { id: "a", ade: "herdr" });
  await act(async () => {});
  expect(screen.queryByRole("status")).toBeNull();

  const web = within(rowOf("web"));
  expect(web.getByRole("img", { name: "Herdr" })).toBeTruthy();
  expect(web.getByText("connected")).toBeTruthy();
  expect(web.queryByRole("button", { name: "Connect" })).toBeNull();
  await userEvent.click(screen.getByText("web"));
  await userEvent.click(screen.getByRole("button", { name: "Open in Herdr" }));
  expect(invoke).toHaveBeenCalledWith("open_in_t3", { id: "b", ade: "herdr" });
  await act(async () => {});
  expect(screen.queryByRole("status")).toBeNull();
  // No T3 pairing or reinstall; Reconnect sets Herdr up again and opens it.
  expect(screen.queryByRole("button", { name: /Reinstall|Pair again/ })).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "Reconnect" }));
  expect(invoke).toHaveBeenCalledWith("connect_sandbox", { id: "b", ade: "herdr" });
  await userEvent.click(screen.getByRole("button", { name: "Back to sandboxes" }));

  await openMenu("web");
  expect(menuItems()[0]).toBe("Open in Herdr");
  await userEvent.keyboard("{Escape}");
  await openMenu("dev-1");
  expect(menuItems()[0]).toBe("Install Herdr");
});

test("a new sandbox in Herdr mode says it installs Herdr and opens a terminal", async () => {
  picked("herdr");
  show({ ades: [t3Ade, herdrAde], tier: "pro" }, [{}], { create_sandbox: () => null });
  await userEvent.click(await screen.findByRole("button", { name: "New sandbox" }));
  expect(screen.getByText("Installs Herdr on the sandbox. Then it opens in a terminal on this Mac.")).toBeTruthy();
  await userEvent.type(screen.getByLabelText("Name"), "dev-2");
  await userEvent.click(screen.getByRole("button", { name: "Create sandbox" }));
  expect(invoke).toHaveBeenCalledWith("create_sandbox", expect.objectContaining({ ade: "herdr" }));
});

test("Herdr on a PC without OpenSSH asks for the OpenSSH client, not for Herdr", async () => {
  const ua = vi.spyOn(navigator, "userAgent", "get").mockReturnValue(windowsAgent);
  try {
    picked("herdr");
    show({ ades: [t3Ade, orcaAde, { ...herdrAde, version: null }] }, [{}]);
    expect(await screen.findByText("OpenSSH is not on this PC")).toBeTruthy();
    expect(screen.getByText("InstaADE opens Herdr in a terminal over SSH. Install the OpenSSH client first.")).toBeTruthy();
    expect(screen.queryByText(/Herdr is not on/)).toBeNull();
    expect(screen.queryByRole("button", { name: "Download Herdr" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Get OpenSSH" }));
    expect(invoke).toHaveBeenCalledWith("open_url", { url: "https://example.test/openssh" });
  } finally {
    ua.mockRestore();
  }
});

// ---- first run and the default mode ----

test("first run after sign-in lands in plain Instabox with a Connect an ADE card", async () => {
  show({ ...noT3, ades: [{ ...t3Ade, version: null }, cmuxAde] }, [{ ades: [] }]);
  expect(await screen.findByText("Open sandboxes in your ADE")).toBeTruthy();
  expect(switcher().getAttribute("aria-label")).toBe("ADE: InstaBox. Switch ADE");
  expect(screen.getByText(/^cmux is on this Mac\. Pick one/)).toBeTruthy();
  // Nothing is forced: the list works, with no ADE actions.
  expect(screen.getByText("dev-1")).toBeTruthy();
  expect(screen.queryByRole("button", { name: /^Install/ })).toBeNull();

  // Connect an ADE opens the switcher.
  await userEvent.click(screen.getByRole("button", { name: "Connect an ADE" }));
  expect(screen.getByRole("menu", { name: "Open sandboxes in" })).toBeTruthy();
  await userEvent.keyboard("{Escape}");

  // Not now keeps plain Instabox and does not ask again.
  await userEvent.click(screen.getByRole("button", { name: "Not now" }));
  expect(screen.queryByText("Open sandboxes in your ADE")).toBeNull();
  expect(localStorage.getItem(modeKey("alex"))).toBe("instabox");
  cleanup();
  show({ ...noT3, ades: [{ ...t3Ade, version: null }, cmuxAde] }, [{ ades: [] }]);
  await screen.findByText("dev-1");
  expect(screen.queryByText("Open sandboxes in your ADE")).toBeNull();
});

test("a new user with no sandboxes and no ADE sees the empty list and the card", async () => {
  show(noT3);
  expect(await screen.findByRole("heading", { name: "No sandboxes" })).toBeTruthy();
  expect(screen.getByText("Create one to get started.")).toBeTruthy();
  expect(screen.getByText("Open sandboxes in your ADE")).toBeTruthy();
  expect(screen.queryByText("T3 Code is not on this Mac")).toBeNull();
});

test("the default mode keeps v0.1.x users in T3 Code until they pick", async () => {
  const modeShown = async () => {
    const b = await screen.findByRole("button", { name: /Switch ADE/ });
    const m = popover().dataset.mode;
    cleanup();
    return [b.getAttribute("aria-label"), m];
  };
  // T3 Code on this Mac, no sandbox with T3.
  show({}, [{ ades: [] }]);
  expect(await modeShown()).toEqual(["ADE: T3 Code. Switch ADE", "t3"]);
  // No T3 Code here, but a sandbox has T3.
  show(noT3, [{}]);
  expect(await modeShown()).toEqual(["ADE: T3 Code. Switch ADE", "t3"]);
  // Neither: plain Instabox.
  show(noT3, [{ ades: [] }, { id: "b", name: "web", ades: ["cmux"] }]);
  expect(await modeShown()).toEqual(["ADE: InstaBox. Switch ADE", "instabox"]);
  // A pick wins over the default, and it is per user.
  picked("instabox");
  show({}, [{}]);
  expect(await modeShown()).toEqual(["ADE: InstaBox. Switch ADE", "instabox"]);
  show({ login: "sam" }, [{}]);
  expect(await modeShown()).toEqual(["ADE: T3 Code. Switch ADE", "t3"]);
});

test("T3 mode keeps the first-run card away and T3's own wording", async () => {
  show({}, [{}]);
  await screen.findByText("dev-1");
  expect(screen.queryByText("Open sandboxes in your ADE")).toBeNull();
  await userEvent.click(screen.getByText("dev-1"));
  for (const name of ["Open in T3 Code", "Pair again", "Reinstall T3"]) expect(screen.getByRole("button", { name })).toBeTruthy();
});

test("in T3 mode a plain sandbox offers Install T3 Code, and a v0.1.x one Open", async () => {
  // A sandbox created in plain Instabox has no mark, so no ADEs. One v0.1.x connected has t3.
  show({}, [
    { id: "p", name: "plain", connected: false, ades: [], t3Version: null },
    { id: "o", name: "old", ades: ["t3"] },
  ]);
  const plain = within(await screen.findByText("plain").then(() => rowOf("plain")));
  expect(plain.getByText("no ADE connected")).toBeTruthy();
  expect(plain.getByRole("button", { name: "Connect" })).toBeTruthy();
  expect(plain.queryByRole("button", { name: "Open in T3 Code" })).toBeNull();
  expect(within(rowOf("old")).getByRole("img", { name: "T3 Code" })).toBeTruthy();
  await userEvent.click(screen.getByText("plain"));
  expect(screen.getByRole("button", { name: "Install T3 Code" })).toBeTruthy();
  expect(screen.queryByText("T3 CodeIn use")).toBeNull();
  const section = within(screen.getByRole("region", { name: "Connected ADE" }));
  expect(section.getByText("No ADE connected")).toBeTruthy();
  expect(section.queryByRole("button")).toBeNull();
});

// ---- details: the ADEs on a sandbox ----

test("details show an Open button for each connected ADE and preserve adding another", async () => {
  picked("cmux");
  show(mac, [{ ades: ["t3", "cmux"] }, { id: "b", name: "web", ades: ["t3"] }]);
  await userEvent.click(await screen.findByText("dev-1"));
  const section = within(screen.getByRole("region", { name: "Connected ADE" }));
  expect(section.getAllByRole("button").map((button) => button.textContent)).toEqual(["Open", "Open"]);
  await userEvent.click(section.getByRole("button", { name: "Open in T3 Code" }));
  expect(invoke).toHaveBeenCalledWith("open_in_t3", { id: "a", ade: "t3" });

  // cmux has nothing to reinstall: Reconnect runs its prep again and opens it.
  expect(screen.queryByRole("button", { name: /Reinstall|Pair again/ })).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "Reconnect" }));
  expect(invoke).toHaveBeenCalledWith("connect_sandbox", { id: "a", ade: "cmux" });
  await userEvent.click(screen.getByRole("button", { name: "Open in cmux" }));
  expect(invoke).toHaveBeenCalledWith("open_in_t3", { id: "a", ade: "cmux" });

  await userEvent.click(screen.getByRole("button", { name: "Back to sandboxes" }));
  await userEvent.click(screen.getByText("web"));
  const add = screen.getByRole("button", { name: "Install cmux on web" });
  expect(add.textContent).toBe("cmux");
  expect(screen.getByRole("button", { name: "Install cmux" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Reconnect" })).toBeNull();
  await userEvent.click(add);
  expect(invoke).toHaveBeenCalledWith("connect_sandbox", { id: "b", ade: "cmux" });
});

test("details in T3 mode offer adding cmux, never an ADE not found here", async () => {
  show({ ades: [t3Ade, { ...cmuxAde, version: null }] }, [{}]);
  await userEvent.click(await screen.findByText("dev-1"));
  const section = within(screen.getByRole("region", { name: "Connected ADE" }));
  expect(section.getByText("T3 Code")).toBeTruthy();
  expect(section.getByRole("button", { name: "Open in T3 Code" }).textContent).toBe("Open");
  expect(screen.queryByRole("button", { name: /Install cmux/ })).toBeNull();
  cleanup();
  show(mac, [{}]);
  await userEvent.click(await screen.findByText("dev-1"));
  await userEvent.click(screen.getByRole("button", { name: "Install cmux on dev-1" }));
  expect(invoke).toHaveBeenCalledWith("connect_sandbox", { id: "a", ade: "cmux" });
});

test("machine cards prioritize updates from another ADE over connecting the selected ADE", async () => {
  picked("cmux");
  const { state } = show(mac, [{ ades: ["t3"], updatePending: true }]);
  await screen.findByText("dev-1");
  const row = within(rowOf("dev-1"));
  expect(row.getByRole("img", { name: "T3 Code" })).toBeTruthy();
  expect(row.queryByRole("button", { name: "Connect" })).toBeNull();
  await userEvent.click(row.getByRole("button", { name: "Update" }));
  expect(invoke).toHaveBeenCalledWith("reinstall_t3", { id: "a", ade: "t3" });
  act(() => emit({ ...state, sandboxes: [{ ...state.sandboxes[0], updatePending: false }] }));
  await userEvent.click(row.getByRole("button", { name: "Connect" }));
  expect(invoke).toHaveBeenCalledWith("connect_sandbox", { id: "a", ade: "cmux" });
});

test("details put Status first in Power and show per-ADE update and Open actions below it", async () => {
  show(mac, [{ ades: ["t3", "cmux"], updatePending: true }]);
  await userEvent.click(await screen.findByText("dev-1"));
  const power = screen.getByRole("region", { name: "Power" });
  const status = within(power).getByRole("group", { name: "Machine status" });
  expect(document.querySelector(".stack")?.firstElementChild).toBe(power);
  expect(status.parentElement?.firstElementChild).toBe(status);
  expect(within(status).getByText("Status")).toBeTruthy();
  expect(within(status).getByText("Running")).toBeTruthy();
  expect(within(status).queryByText("4 vCPU / 8 GB")).toBeNull();
  const section = screen.getByRole("region", { name: "Connected ADE" });
  expect(power.nextElementSibling).toBe(section);
  const t3 = within(within(section).getByText("T3 Code").closest(".connected-ade-row") as HTMLElement);
  expect(t3.getByText("Needs update")).toBeTruthy();
  expect(t3.getByRole("button", { name: "Update" })).toBeTruthy();
  const cmux = within(within(section).getByText("cmux").closest(".connected-ade-row") as HTMLElement);
  expect(cmux.queryByText("Connected")).toBeNull();
  expect(cmux.queryByText("Needs update")).toBeNull();
  await userEvent.click(cmux.getByRole("button", { name: "Open in cmux" }));
  expect(invoke).toHaveBeenCalledWith("open_in_t3", { id: "a", ade: "cmux" });
});

test("a row says connected only when this computer can open the sandbox", async () => {
  show(mac, [
    { id: "a", name: "here", ades: ["t3"] },
    // T3 was set up from another computer and is not paired from this one.
    { id: "b", name: "elsewhere", ades: ["t3"], connected: false },
    // cmux opens from a link, so it needs no pairing here.
    { id: "c", name: "link", ades: ["cmux"], connected: false },
    { id: "d", name: "none", ades: [], connected: false },
  ]);
  await screen.findByText("here");
  expect(within(rowOf("here")).getByText("connected")).toBeTruthy();
  expect(within(rowOf("elsewhere")).getByText("not connected on this Mac")).toBeTruthy();
  expect(within(rowOf("link")).getByText("connected")).toBeTruthy();
  expect(within(rowOf("none")).getByText("no ADE connected")).toBeTruthy();
});

test("details offer Open only for an ADE whose app is on this computer", async () => {
  const ua = vi.spyOn(navigator, "userAgent", "get").mockReturnValue(windowsAgent);
  try {
    // Windows lists only T3. The sandbox also has cmux, set up from a Mac.
    show({ ades: [t3Ade] }, [{ ades: ["t3", "cmux"] }]);
    await userEvent.click(await screen.findByText("dev-1"));
    const section = screen.getByRole("region", { name: "Connected ADE" });
    const cmux = within(within(section).getByText("cmux").closest(".connected-ade-row") as HTMLElement);
    expect(cmux.queryByRole("button", { name: "Open in cmux" })).toBeNull();
    expect(cmux.getByText("Not available on this PC")).toBeTruthy();
    const t3 = within(within(section).getByText("T3 Code").closest(".connected-ade-row") as HTMLElement);
    expect(t3.getByRole("button", { name: "Open in T3 Code" })).toBeTruthy();
  } finally {
    ua.mockRestore();
  }
});

test("details without T3 Code on this computer say so instead of Open", async () => {
  show(noT3, [{ ades: ["t3"] }]);
  await userEvent.click(await screen.findByText("dev-1"));
  const section = screen.getByRole("region", { name: "Connected ADE" });
  expect(within(section).queryByRole("button", { name: "Open in T3 Code" })).toBeNull();
  expect(within(section).getByText("Not installed on this Mac")).toBeTruthy();
});

// ---- the old app ----

const oldApp = { paths: ["/Applications/Instabox.app"] };
const oldAppText = "Instabox is still in Applications. InstaADE replaces it.";

test("old app: no notice without one", async () => {
  show({}, [{}]);
  await screen.findByText("dev-1");
  expect(screen.queryByText(oldAppText)).toBeNull();
  expect(screen.queryByRole("button", { name: "Move to Trash" })).toBeNull();
});

test("old app: Move to Trash asks the back end, which clears the notice", async () => {
  const { state } = show({ oldApp }, [{}]);
  const notice = (await screen.findByText(oldAppText)).closest(".banner") as HTMLElement;
  expect(notice.getAttribute("role")).toBe("status");
  await userEvent.click(within(notice).getByRole("button", { name: "Move to Trash" }));
  expect(invoke).toHaveBeenCalledWith("trash_old_app");
  act(() => emit({ ...state, oldApp: null }));
  expect(screen.queryByText(oldAppText)).toBeNull();
  expect(calls("dismiss_old_app").length).toBe(0);
});

test("old app: Not now hides it for this run", async () => {
  const { state } = show({ oldApp }, [{}]);
  const notice = (await screen.findByText(oldAppText)).closest(".banner") as HTMLElement;
  await userEvent.click(within(notice).getByRole("button", { name: "Not now" }));
  expect(invoke).toHaveBeenCalledWith("dismiss_old_app");
  act(() => emit({ ...state, oldApp: null }));
  expect(screen.queryByText(oldAppText)).toBeNull();
  expect(calls("trash_old_app").length).toBe(0);
});

test("old app: a failed move shows why and keeps the notice", async () => {
  const error = { kind: "other" as const, message: "Quit Instabox, then click Move to Trash again." };
  show({ oldApp }, [{}], { trash_old_app: () => Promise.reject(error) });
  await userEvent.click(await screen.findByRole("button", { name: "Move to Trash" }));
  // The rejection and state.error carry the same text: it shows once.
  expect((await screen.findByText(error.message)).closest(".banner")?.getAttribute("role")).toBe("alert");
  expect(screen.getByText(oldAppText)).toBeTruthy();
});

test("old app: the notice shows on the other screens too, sign-in included", async () => {
  show({ oldApp, signedIn: false });
  await screen.findByRole("button", { name: "Sign in with GitHub" });
  expect(screen.getByText(oldAppText)).toBeTruthy();
});
