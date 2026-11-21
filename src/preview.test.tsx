import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { StrictMode } from "react";

vi.mock("./hero/HeroCube", () => ({ HeroCube: () => null }));

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => false,
  invoke: () => { throw new Error("Browser preview must not call native APIs"); },
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: () => { throw new Error("Browser preview must not listen to native events"); },
}));

import App from "./App";
import { openInAde, pause } from "./api";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });

test("browser preview renders, navigates and updates sample data without native APIs", async () => {
  render(<StrictMode><App /></StrictMode>);
  await userEvent.click(await screen.findByText("dev-workspace"));
  expect(screen.getByRole("button", { name: "Open in T3 Code" })).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "Back to sandboxes" }));
  const open = vi.spyOn(window, "open").mockReturnValue(null);
  await userEvent.click(screen.getByRole("button", { name: "Open InstaBox dashboard" }));
  expect(open).toHaveBeenCalledWith("https://dashboard.instabox.sh/dashboard/machines", "_blank", "noopener,noreferrer");
  await act(() => pause("a"));
  expect(screen.getByRole("button", { name: "Resume dev-workspace" })).toBeTruthy();
  await userEvent.click(screen.getByRole("combobox", { name: "Workspace" }));
  await userEvent.click(screen.getByRole("option", { name: "org-team" }));
  expect(screen.getByRole("combobox", { name: "Workspace" }).textContent).toBe("org-team");
  await expect(openInAde("a", "t3")).rejects.toMatchObject({ kind: "other", message: expect.stringContaining("browser preview") });
});
