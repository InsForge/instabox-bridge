import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { useAdeConnections } from "./ade";
import type { Ade } from "./ade";
import fixture from "./fixtures/state.json";
import type { AppState } from "./types";

const state = { ...fixture, progress: null, error: null } as AppState;
beforeEach(() => vi.useFakeTimers());
afterEach(() => { cleanup(); vi.useRealTimers(); });

test("ADE checks return distinct results without changing the source machines", () => {
  const before = structuredClone(state);
  const { result, rerender } = renderHook(({ ade, request }) => useAdeConnections(ade, state, request), {
    initialProps: { ade: "orca" as Ade, request: 1 },
  });
  expect(Object.values(result.current.connections!)).toEqual(state.sandboxes.map(() => "checking"));
  act(() => vi.advanceTimersByTime(450));
  expect(result.current.connections?.a).toBe("connected");
  expect(result.current.connections?.e).toBe("disconnected");
  rerender({ ade: "cmux", request: 2 });
  expect(result.current.missing).toBe(true);
  expect(result.current.connections).toBeNull();
  act(() => result.current.connect("a"));
  act(() => vi.advanceTimersByTime(1000));
  expect(result.current.connections).toBeNull();
  rerender({ ade: "t3", request: 3 });
  act(() => vi.advanceTimersByTime(450));
  for (const machine of state.sandboxes) expect(result.current.connections?.[machine.id]).toBe(machine.connected ? "connected" : "disconnected");
  expect(state).toEqual(before);
});

test("rapid switching cancels old checks and revisiting an ADE checks again", () => {
  const { result, rerender } = renderHook(({ ade, request }) => useAdeConnections(ade, state, request), {
    initialProps: { ade: "orca" as Ade, request: 1 },
  });
  act(() => vi.advanceTimersByTime(450));
  rerender({ ade: "cmux", request: 2 });
  act(() => vi.advanceTimersByTime(100));
  rerender({ ade: "orca", request: 3 });
  expect(result.current.connections?.a).toBe("checking");
  act(() => vi.advanceTimersByTime(350));
  expect(result.current.connections?.a).toBe("checking");
  act(() => vi.advanceTimersByTime(100));
  expect(result.current.connections?.a).toBe("connected");
});

test("workspace and machine changes invalidate results, and signing out cancels checks", () => {
  const { result, rerender } = renderHook(({ snapshot }) => useAdeConnections("orca", snapshot, 1), {
    initialProps: { snapshot: state },
  });
  act(() => vi.advanceTimersByTime(450));
  const changed = { ...state, orgId: "another-workspace", sandboxes: [{ ...state.sandboxes[0], id: "new-machine" }] };
  rerender({ snapshot: changed });
  expect(result.current.connections).toEqual({ "new-machine": "checking" });
  act(() => vi.advanceTimersByTime(450));
  expect(Object.keys(result.current.connections!)).toEqual(["new-machine"]);
  rerender({ snapshot: { ...changed, signedIn: false } });
  act(() => vi.advanceTimersByTime(450));
  expect(result.current.connections).toBeNull();
});


test("placeholder connects are scoped to the ADE and workspace and survive rechecks", () => {
  const { result, rerender } = renderHook(({ ade, snapshot, request }) => useAdeConnections(ade, snapshot, request), {
    initialProps: { ade: "paradev" as Ade, snapshot: state, request: 1 },
  });
  act(() => vi.advanceTimersByTime(450));
  expect(result.current.connections?.e).toBe("disconnected");
  act(() => result.current.connect("e"));
  expect(result.current.connections?.e).toBe("connecting");
  act(() => vi.advanceTimersByTime(650));
  expect(result.current.connections?.e).toBe("connected");
  rerender({ ade: "orca", snapshot: state, request: 2 });
  act(() => vi.advanceTimersByTime(450));
  expect(result.current.connections?.e).toBe("disconnected");
  rerender({ ade: "paradev", snapshot: state, request: 3 });
  act(() => vi.advanceTimersByTime(450));
  expect(result.current.connections?.e).toBe("connected");
  rerender({ ade: "paradev", snapshot: { ...state, orgId: "another-workspace" }, request: 3 });
  act(() => vi.advanceTimersByTime(450));
  expect(result.current.connections?.e).toBe("disconnected");
});

test("switching ADE cancels an unfinished placeholder connect", () => {
  const { result, rerender } = renderHook(({ ade, request }) => useAdeConnections(ade, state, request), {
    initialProps: { ade: "paradev" as Ade, request: 1 },
  });
  act(() => vi.advanceTimersByTime(450));
  act(() => result.current.connect("e"));
  rerender({ ade: "orca", request: 2 });
  act(() => vi.advanceTimersByTime(1000));
  rerender({ ade: "paradev", request: 3 });
  act(() => vi.advanceTimersByTime(450));
  expect(result.current.connections?.e).toBe("disconnected");
});
