import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { dataAdapter } from "../../../data";
import { resetSimulation } from "../../../data/mock/simulation";
import { resetViewport, setViewport } from "../../../test-viewport";
import { SettingsPage } from "../SettingsPage";
import { MoreRoute } from "./MorePage";
import { renderAt } from "./harness";

const routes = [
  { path: "more", element: <MoreRoute /> },
  { path: "settings", element: <SettingsPage /> },
  { path: "settings/:tab", element: <SettingsPage /> },
];

describe("E2–E4 · phone settings", () => {
  beforeEach(() => {
    resetSimulation();
    setViewport(390);
  });
  afterEach(() => {
    resetViewport();
    vi.restoreAllMocks();
  });

  it("sends bare Settings to More and gives every Settings screen a back bar to More", async () => {
    renderAt("/app/settings", routes);
    expect(await screen.findByRole("heading", { name: "More" }, { timeout: 5000 })).toBeInTheDocument();
  });

  it("E3: toggles per-Session rules with wrapping chips and rolls back a refusal", async () => {
    const setRule = vi.spyOn(dataAdapter.notifications, "setRule").mockRejectedValueOnce(new Error("Machine offline"));
    renderAt("/app/settings/notifications", routes);
    const user = userEvent.setup();
    expect(await screen.findByRole("button", { name: "‹ More" }, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Enable notifications on this device" })).toBeInTheDocument();
    const row = await screen.findByRole("group", { name: "Qwen Training on RTX-PC" }, { timeout: 5000 });
    expect(within(row).getAllByRole("button").map((chip) => chip.textContent)).toEqual([
      "Finished", "Error", "Stalled", "Waiting", "Checkpoint", "Progress",
    ]);
    const stalled = within(row).getByRole("button", { name: "Stalled" });
    expect(stalled).toHaveAttribute("aria-pressed", "true");
    await user.click(stalled);
    expect(await screen.findByRole("alert")).toHaveTextContent("Machine offline");
    expect(stalled).toHaveAttribute("aria-pressed", "true");
    expect(setRule).toHaveBeenCalledWith("qwen", expect.objectContaining({ stalled: false }));
  });

  it("E2: shows Machine cards with role and status, and revokes only through the sheet", async () => {
    const revoke = vi.spyOn(dataAdapter.registry, "revoke");
    renderAt("/app/settings/machines", routes);
    const user = userEvent.setup();
    const card = await screen.findByRole("article", { name: "RTX-PC" }, { timeout: 5000 });
    expect(within(card).getByText("WORKER")).toBeInTheDocument();
    expect(screen.getByRole("article", { name: "Mac Mini" })).toHaveTextContent("CONTROL");
    await user.click(within(card).getByRole("button", { name: "Revoke…" }));
    const sheet = screen.getByRole("dialog", { name: "Revoke RTX-PC?" });
    await user.click(within(sheet).getByRole("button", { name: "Cancel" }));
    expect(revoke).not.toHaveBeenCalled();
    await user.click(within(card).getByRole("button", { name: "Revoke…" }));
    await user.click(screen.getByRole("button", { name: "Revoke" }));
    expect(revoke).toHaveBeenCalledWith("rtx");
  });

  it("E4: password change and sign-out on this browser", async () => {
    renderAt("/app/settings/account", routes);
    expect(await screen.findByRole("button", { name: "‹ More" }, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Account" })).toBeInTheDocument();
    expect(screen.getByLabelText(/Current password/i)).toBeInTheDocument();
  });
});
