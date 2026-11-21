import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { T3_DOWNLOAD_URL } from "./api";
import fixture from "./fixtures/state.json";
import { modeKey } from "./mode";
import type { AdeView, AppState, SandboxView } from "./types";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a), isTauri: () => true }));
vi.mock("./hero/HeroCube", () => ({ HeroCube: () => null }));
let emit: (s: AppState) => void = () => {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: (_: string, cb: (e: { payload: AppState }) => void) => {
    emit = (s) => cb({ payload: s });
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
/** A Mac with T3 Code and cmux. */
const mac: Partial<AppState> = { ades: [t3Ade, cmuxAde] };
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
    error: { kind: "other", message: "" },
    update: { version: "", url: "" },
  };
  // minutes and code are optional on ErrorView.
  const strip = (ks: string[]) => [...new Set(ks.filter((k) => !/\.(minutes|code)$/.test(k)))].sort();
  const want = strip(keys(sampleWithRows));
  const got = strip(keys(fixture));
  expect(got.filter((k) => !want.includes(k))).toEqual([]);
  expect(want.filter((k) => !got.includes(k))).toEqual([]);
});

// ---- list ----

test("a row shows its state, pause or resume, the … menu and Open in T3 Code", async () => {
  show({}, [{}, { id: "b", name: "web", state: "paused", t3Version: null }, { id: "c", name: "old", updatePending: true, t3Version: "0.1.2" }]);
  const row = await screen.findByText("dev-1").then(() => rowOf("dev-1"));
  expect(within(row).getByText("Running")).toBeTruthy();
  expect(within(row).getByText("T3 Code 0.1.10")).toBeTruthy();
  expect(within(rowOf("web")).getByText("Paused")).toBeTruthy();
  expect(within(rowOf("web")).getByText("T3 Code")).toBeTruthy();
  expect(within(rowOf("old")).getByText("T3 is older than this Mac's 0.1.10")).toBeTruthy();
  const labels = within(row)
    .getAllByRole("button")
    .slice(1)
    .map((b) => b.getAttribute("aria-label") ?? b.textContent);
  expect(labels).toEqual(["Pause dev-1", "More actions for dev-1", "Open in T3 Code"]);
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
  await userEvent.click(screen.getByText("No T3 Code here yet"));
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
  expect(update.getByText("T3 is older than this Mac's 0.1.10")).toBeTruthy();
  await userEvent.click(update.getByRole("button", { name: "Update" }));
  expect(invoke).toHaveBeenCalledWith("reinstall_t3", { id: "b", ade: "t3" });

  // A sandbox ahead of this Mac never offers Update.
  const behind = within(rowOf("p-behind"));
  expect(behind.getByText("T3 Code on this Mac is older")).toBeTruthy();
  expect(behind.queryByRole("button", { name: "Update" })).toBeNull();
  await userEvent.click(behind.getByRole("button", { name: "Download" }));
  expect(invoke).toHaveBeenCalledWith("open_url", { url: T3_DOWNLOAD_URL });

  const install = within(rowOf("p-install"));
  expect(install.getByText("No T3 Code here yet")).toBeTruthy();
  await userEvent.click(install.getByRole("button", { name: "Install T3 Code" }));
  expect(invoke).toHaveBeenCalledWith("connect_sandbox", { id: "d", ade: "t3" });

  expect(within(rowOf("p-behind-unconnected")).queryByText("No T3 Code here yet")).toBeNull();
  const open = within(rowOf("p-open"));
  expect(open.getByText("T3 Code 0.1.10")).toBeTruthy();
  await userEvent.click(open.getByRole("button", { name: "Open in T3 Code" }));
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
  expect(within(rowOf("dev-1")).getByRole("button", { name: "Install T3 Code" })).toBeTruthy();
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
  expect(screen.getByText("Same as this Mac")).toBeTruthy();
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

test("desktopBehind never offers Update", async () => {
  show({}, [{ desktopBehind: true, updatePending: true }]);
  await userEvent.click(await screen.findByText("dev-1"));
  expect(screen.getByText("Newer than this Mac")).toBeTruthy();
  expect(screen.getByText("T3 Code on this Mac is older")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Update" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Reinstall T3" })).toBeNull();
});

test("updatePending offers Update that reinstalls", async () => {
  show({}, [{ updatePending: true, t3Version: "0.1.2" }]);
  await userEvent.click(await screen.findByText("dev-1"));
  expect(screen.getByText("Older than this Mac")).toBeTruthy();
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

test("settings: update, account, the ADEs on this Mac and start at login", async () => {
  show(
    {
      ades: [t3Ade, { ...cmuxAde, version: null }],
      orgs: [...sample.orgs, { id: "org-team", name: "team", tier: "pro", personal: false }],
      update: { version: "0.2.0", url: "https://example.com/v0.2.0" },
    },
    [{}],
  );
  await userEvent.click(await screen.findByRole("button", { name: "Settings" }));
  expect(screen.getByRole("heading", { name: "This Mac" })).toBeTruthy();
  const ades = within(screen.getByRole("list", { name: "ADEs on this Mac" }));
  expect(ades.getAllByRole("listitem").map((li) => li.textContent)).toEqual([
    "T3 Code0.1.10",
    "cmuxNot found",
    "OrcaComing soon",
    "SSupersetComing soon",
    "ParadevComing soon",
  ]);
  expect(screen.getByText("A new version of InstaBox is ready.")).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "Download" }));
  expect(invoke).toHaveBeenCalledWith("open_url", { url: "https://example.com/v0.2.0" });

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

// ---- first run ----

test("sign-in asks for no ADE", async () => {
  show({ ...noT3, signedIn: false, login: null });
  await userEvent.click(await screen.findByRole("button", { name: "Sign in with GitHub" }));
  expect(invoke).toHaveBeenCalledWith("sign_in", { provider: "github" });
  expect(screen.getByText("Run any ADE on an InstaBox Sandbox.")).toBeTruthy();
  expect(screen.getByRole("img", { name: "InstaBox" })).toBeTruthy();
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
  fireEvent.click(screen.getByRole("button", { name: "Download T3 Code" }));
  expect(invoke).toHaveBeenCalledWith("open_url", { url: T3_DOWNLOAD_URL });
  await userEvent.click(screen.getByRole("button", { name: "Check again" }));
  expect(invoke).toHaveBeenCalledWith("refresh_t3");
});

// ---- the ADE switcher ----

test("the switcher lists Instabox, the ADEs on this Mac and the ones coming soon", async () => {
  show(mac, [{}, { id: "b", name: "web", ades: ["t3", "cmux"] }]);
  await screen.findByText("dev-1");
  expect(switcher().getAttribute("aria-label")).toBe("ADE: T3 Code. Switch ADE");
  await userEvent.click(switcher());
  const menu = within(screen.getByRole("menu", { name: "Open sandboxes in" }));
  const items = menu.getAllByRole("menuitemradio");
  expect(items.map((b) => b.textContent)).toEqual([
    "InstaBox" + "Sandboxes only, no ADE",
    "T3 Code" + "0.1.10 on this Mac · on 2 sandboxes",
    "cmux" + "0.64.25 on this Mac · on 1 sandbox",
    "OrcaNot supported yetComing soon",
    "SSupersetNot supported yetComing soon",
    "ParadevNot supported yetComing soon",
  ]);
  expect(items.map((b) => b.getAttribute("aria-checked"))).toEqual(["false", "true", "false", "false", "false", "false"]);
  for (const b of items.slice(3)) expect((b as HTMLButtonElement).disabled).toBe(true);
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
  // T3's lines are gone; each row offers cmux.
  expect(within(rowOf("dev-1")).getByText("No cmux here yet")).toBeTruthy();
  expect(within(rowOf("web")).getByText("Ready for cmux")).toBeTruthy();

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
  await userEvent.click(install.getByRole("button", { name: "Install cmux" }));
  expect(invoke).toHaveBeenCalledWith("connect_sandbox", { id: "a", ade: "cmux" });
  // cmux asks in its own window. The popover adds no notice once the action is done.
  await act(async () => {});
  expect(screen.queryByText(/Confirm the/)).toBeNull();

  await userEvent.click(within(rowOf("web")).getByRole("button", { name: "Open in cmux" }));
  expect(invoke).toHaveBeenCalledWith("open_in_t3", { id: "b", ade: "cmux" });
  await act(async () => {});
  expect(screen.queryByText(/Confirm the/)).toBeNull();
  expect((within(rowOf("busy")).getByRole("button", { name: "Open in cmux" }) as HTMLButtonElement).disabled).toBe(true);

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

test("plain Instabox: sandboxes only, with no Open, Install, Connect or Reinstall", async () => {
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
  for (const name of [/^Open in (T3|cmux)/, /^Install/, "Pair again", "Reinstall T3", "Reconnect"]) expect(screen.queryByRole("button", { name })).toBeNull();
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
  expect(plain.getByText("No T3 Code here yet")).toBeTruthy();
  expect(plain.getByRole("button", { name: "Install T3 Code" })).toBeTruthy();
  expect(plain.queryByRole("button", { name: "Open in T3 Code" })).toBeNull();
  expect(within(rowOf("old")).getByRole("button", { name: "Open in T3 Code" })).toBeTruthy();
  await userEvent.click(screen.getByText("plain"));
  expect(screen.getByRole("button", { name: "Install T3 Code" })).toBeTruthy();
  expect(screen.queryByText("T3 CodeIn use")).toBeNull();
  const chips = within(screen.getByRole("list", { name: "ADEs on this sandbox" }));
  expect(chips.queryAllByRole("listitem").filter((li) => li.className.includes("chip"))).toEqual([]);
});

// ---- details: the ADEs on a sandbox ----

test("details show a chip for each ADE on the sandbox and a dashed one to add another", async () => {
  picked("cmux");
  show(mac, [{ ades: ["t3", "cmux"] }, { id: "b", name: "web", ades: ["t3"] }]);
  await userEvent.click(await screen.findByText("dev-1"));
  const chips = within(screen.getByRole("list", { name: "ADEs on this sandbox" }));
  expect(chips.getAllByRole("listitem").map((c) => c.textContent)).toEqual(["T3 Code0.1.10", "cmuxIn use"]);
  expect(chips.queryByRole("button")).toBeNull();

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
  const chips = within(screen.getByRole("list", { name: "ADEs on this sandbox" }));
  expect(chips.getAllByRole("listitem").map((c) => c.textContent)).toEqual(["T3 CodeIn use"]);
  expect(screen.queryByRole("button", { name: /Install cmux/ })).toBeNull();
  cleanup();
  show(mac, [{}]);
  await userEvent.click(await screen.findByText("dev-1"));
  await userEvent.click(screen.getByRole("button", { name: "Install cmux on dev-1" }));
  expect(invoke).toHaveBeenCalledWith("connect_sandbox", { id: "a", ade: "cmux" });
});
