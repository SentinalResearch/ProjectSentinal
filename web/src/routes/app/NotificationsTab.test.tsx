import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { AuthProvider } from "../../context/AuthContext";
import { ThemeProvider } from "../../context/ThemeContext";
import { dataAdapter } from "../../data";
import { resetSimulation } from "../../data/mock/simulation";
import { AppShell } from "./AppShell";
import { SettingsPage } from "./SettingsPage";

function renderNotifications() {
  return render(
    <MemoryRouter initialEntries={["/app/settings/notifications"]}>
      <ThemeProvider>
        <AuthProvider>
          <Routes>
            <Route path="/app" element={<AppShell />}>
              <Route path="settings/:tab" element={<SettingsPage />} />
            </Route>
          </Routes>
        </AuthProvider>
      </ThemeProvider>
    </MemoryRouter>,
  );
}

const cell = (name: string) => screen.findByRole("button", { name }, { timeout: 5000 });

describe("NotificationsTab", () => {
  beforeAll(() => {
    Object.defineProperty(window, "innerWidth", { writable: true, configurable: true, value: 1440 });
  });
  beforeEach(() => resetSimulation());
  afterEach(() => vi.restoreAllMocks());

  it("shows the daemon defaults: checkpoint and progress off, even for a Session with progress", async () => {
    renderNotifications();
    expect(await cell("Qwen Training on RTX-PC: finished")).toHaveAttribute("aria-pressed", "true");
    expect(await cell("Qwen Training on RTX-PC: checkpoint")).toHaveAttribute("aria-pressed", "false");
    expect(await cell("Qwen Training on RTX-PC: progress")).toHaveAttribute("aria-pressed", "false");
  });

  it("a Session without an override follows its own Machine's default only", async () => {
    renderNotifications();
    const user = userEvent.setup();
    const setRule = vi.spyOn(dataAdapter.notifications, "setRule");

    await user.click(await cell("Default for new sessions on RTX-PC: finished"));

    expect(setRule).toHaveBeenCalledWith("rtx:default", expect.objectContaining({ finished: false }));
    expect(await cell("Qwen Training on RTX-PC: finished")).toHaveAttribute("aria-pressed", "false");
    expect(await cell("Default for new sessions on Mac Mini: finished")).toHaveAttribute("aria-pressed", "true");
  });

  it("rolls a toggle back and says so when the Machine refuses the rule", async () => {
    renderNotifications();
    const user = userEvent.setup();
    vi.spyOn(dataAdapter.notifications, "setRule").mockRejectedValueOnce(new Error("Machine offline"));

    const button = await cell("Qwen Training on RTX-PC: stalled");
    await user.click(button);

    expect(await screen.findByRole("alert")).toHaveTextContent("Machine offline");
    await waitFor(() => expect(button).toHaveAttribute("aria-pressed", "true"));
  });
});
