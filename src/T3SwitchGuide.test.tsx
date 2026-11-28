import { act, cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import fixture from "./fixtures/state.json";
import type { AppState, T3SwitchView } from "./types";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a), isTauri: () => true }));
vi.mock("./hero/HeroCube", () => ({ HeroCube: () => null }));
let emit: (s: AppState) => void = () => {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, cb: (e: { payload: unknown }) => void) => {
    if (name === "state") emit = (s) => cb({ payload: s });
    return Promise.resolve(() => {});
  },
}));

import App from "./App";

afterEach(() => {
  cleanup();
  invoke.mockReset();
});

const guide: T3SwitchView = fixture.t3Switch;
const title = "Switch name-e to the local connection";

function show(t3Switch: T3SwitchView | null, answers: Record<string, () => unknown> = {}) {
  const state: AppState = {
    ...(structuredClone(fixture) as AppState),
    progress: null,
    oldApp: null,
    error: null,
    update: null,
    t3Switch,
  };
  invoke.mockImplementation((cmd: string) => {
    if (answers[cmd]) return Promise.resolve().then(answers[cmd]);
    if (cmd === "get_state") return Promise.resolve(state);
    if (cmd === "agent_status") return Promise.resolve(false);
    return Promise.resolve();
  });
  render(<App />);
  return state;
}

const dialog = () => screen.findByRole("dialog", { name: title });
const calls = (cmd: string) => invoke.mock.calls.filter((c) => c[0] === cmd);

test("t3 switch guide: steps with T3 Code's labels and the sandbox's addresses", async () => {
  show(guide);
  const d = within(await dialog());
  expect(d.getByText("T3 Code saved this sandbox with its old address. That address keeps the sandbox awake.")).toBeTruthy();
  const steps = d.getAllByRole("listitem").map((li) => li.textContent);
  expect(steps).toHaveLength(5);
  expect(steps[0]).toContain("open Settings. Under Connections, click Environments. If you see a map, click List.");
  expect(steps[1]).toBe(`On this sandbox's row, click Routes. One route shows ${guide.oldAddress}.`);
  expect(steps[2]).toBe("Click Add route. Keep Remote link selected.");
  expect(steps[3]).toContain("Click Copy. Paste into Host. It fills Pairing code too. Click Add route.");
  expect(steps[3]).toContain("The new route shows as This device.");
  expect(steps[4]).toBe(`On the route with ${guide.oldAddress}, click Remove route (×). Then click Confirm.`);
  expect(d.getByText("Your chats and settings stay. Only the address changes.")).toBeTruthy();
  // What to paste: the proxy's address, never its one-time code.
  expect(d.getByLabelText(`Pairing link for ${guide.newAddress}`).textContent).toBe(`${guide.newAddress}/pair#token=••••••`);
  // No em dashes or semicolons in what it says.
  expect(screen.getByRole("dialog").textContent).not.toMatch(/[—;]/);
  // No other command ran: copying waits for the click.
  expect(calls("copy_t3_switch_url")).toHaveLength(0);
});

test("t3 switch guide: Copy copies a new pairing link each time, and says so", async () => {
  show(guide);
  const d = within(await dialog());
  await userEvent.click(d.getByRole("button", { name: "Copy" }));
  expect(invoke).toHaveBeenCalledWith("copy_t3_switch_url", { id: "e" });
  expect(await d.findByRole("button", { name: "Copied" })).toBeTruthy();
  await userEvent.click(d.getByRole("button", { name: "Copied" }));
  expect(calls("copy_t3_switch_url")).toHaveLength(2);
  // Copying never closes the guide.
  expect(calls("close_t3_switch")).toHaveLength(0);
});

test("t3 switch guide: a failed copy says why, and Copy works again", async () => {
  const error = { kind: "other", message: "The T3 proxy of this sandbox is not running. Use Open in T3 Code again." };
  show(guide, { copy_t3_switch_url: () => Promise.reject(error) });
  const d = within(await dialog());
  await userEvent.click(d.getByRole("button", { name: "Copy" }));
  expect((await d.findByRole("alert")).textContent).toBe(error.message);
  expect(d.getByRole("button", { name: "Copy" })).toBeTruthy();
});

test("t3 switch guide: Don't show again and Done end it for the sandbox", async () => {
  for (const button of ["Don't show again", "Done"]) {
    const state = show(guide);
    await userEvent.click(within(await dialog()).getByRole("button", { name: button }));
    expect(invoke).toHaveBeenLastCalledWith("close_t3_switch", { id: "e", done: true });
    act(() => emit({ ...state, t3Switch: null }));
    expect(screen.queryByRole("dialog")).toBeNull();
    cleanup();
    invoke.mockReset();
  }
});

test("t3 switch guide: Close and Escape close it for now only", async () => {
  show(guide);
  await userEvent.click(within(await dialog()).getByRole("button", { name: "Close" }));
  expect(invoke).toHaveBeenLastCalledWith("close_t3_switch", { id: "e", done: false });
  await userEvent.keyboard("{Escape}");
  expect(calls("close_t3_switch")).toEqual([
    ["close_t3_switch", { id: "e", done: false }],
    ["close_t3_switch", { id: "e", done: false }],
  ]);
});

test("t3 switch guide: shows only while the state has one", async () => {
  const state = show(null);
  await screen.findByText("name-a");
  expect(screen.queryByRole("dialog")).toBeNull();
  act(() => emit({ ...state, t3Switch: guide }));
  expect(await dialog()).toBeTruthy();
  act(() => emit({ ...state, t3Switch: null }));
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("t3 switch guide: hidden while signed out", async () => {
  show(guide);
  await dialog();
  act(() => emit({ ...(structuredClone(fixture) as AppState), signedIn: false, error: null, progress: null, t3Switch: guide }));
  expect(screen.queryByRole("dialog")).toBeNull();
});
