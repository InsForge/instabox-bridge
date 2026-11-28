import { describe, expect, it } from "vitest";
import fixture from "./fixtures/state.json";
import type { AppState } from "./types";

describe("AppState fixture", () => {
  it("has the keys of AppState (the Rust test checks the same file)", () => {
    // Type-checked: tsc fails when the fixture drifts from AppState.
    const state: AppState = fixture as AppState;
    expect(Object.keys(state).sort()).toEqual(
      [
        "signedIn",
        "login",
        "signInCode",
        "orgs",
        "orgId",
        "tier",
        "t3",
        "ades",
        "sandboxes",
        "sizes",
        "sleepChoices",
        "snapshots",
        "mcp",
        "progress",
        "oldApp",
        "t3Switch",
        "error",
        "startAtLogin",
        "update",
      ].sort(),
    );
  });
});
