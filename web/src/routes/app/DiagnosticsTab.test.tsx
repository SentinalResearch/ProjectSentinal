import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { AuthProvider } from "../../context/AuthContext";
import { ThemeProvider } from "../../context/ThemeContext";
import { markLoad, resetLoadTiming } from "../../data/load-timing";
import { resetSimulation } from "../../data/mock/simulation";
import { store } from "../../data/mock/store";
import { AppShell } from "./AppShell";
import { SettingsPage } from "./SettingsPage";
import { settingsTabs } from "./settings-tabs";

function renderDiagnostics() {
  return render(
    <MemoryRouter initialEntries={["/app/settings/diagnostics"]}>
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

describe("Diagnostics", () => {
  beforeAll(() => {
    Object.defineProperty(window, "innerWidth", { writable: true, configurable: true, value: 1440 });
  });
  beforeEach(() => resetSimulation());

  it("is a Settings tab in both data modes", () => {
    expect(settingsTabs(false).map((tab) => tab.key)).toContain("diagnostics");
    expect(settingsTabs(true).map((tab) => tab.key)).toContain("diagnostics");
  });

  it("shows when each step of the last page load happened", async () => {
    resetLoadTiming();
    const now = vi.spyOn(performance, "now");
    now.mockReturnValue(120);
    markLoad("snapshot");
    now.mockReturnValue(340);
    markLoad("live");
    now.mockRestore();
    renderDiagnostics();
    expect(await screen.findByText(/^Last page load:/, {}, { timeout: 5000 })).toHaveTextContent(
      /snapshot 120 ms · live updates 340 ms/,
    );
  });

  it("shows a delayed Machine as online with its last update", async () => {
    Object.assign(store.machines.find((machine) => machine.id === "mini")!, {
      freshness: "STALE",
      syncedAt: Date.now() - 14_400,
    });
    renderDiagnostics();
    const row = await screen.findByRole("row", { name: /Mac Mini/ }, { timeout: 5000 });
    expect(within(row).getByText("Online · updated 14 s ago")).toBeInTheDocument();
  });

  it("measures each Machine from this browser and from the control, with the Tailscale path", async () => {
    renderDiagnostics();
    expect(await screen.findByRole("heading", { name: "Diagnostics" }, { timeout: 5000 })).toBeInTheDocument();

    await userEvent.setup().click(screen.getByRole("button", { name: "Run network test" }));

    const relayed = await screen.findByRole("row", { name: /RTX-PC/ }, { timeout: 5000 });
    expect(within(relayed).getByText("Relayed · fra")).toBeInTheDocument();
    expect(within(relayed).getByText("30.1 ms")).toBeInTheDocument();
    expect(within(relayed).getByText(/relay adds delay/i)).toBeInTheDocument();

    const control = screen.getByRole("row", { name: /Mac Mini/ });
    expect(within(control).getByText("Control")).toBeInTheDocument();
    expect(within(control).getByText("12.0 ms")).toBeInTheDocument();

    const offline = screen.getByRole("row", { name: /MacBook/ });
    expect(within(offline).getAllByText("Unreachable").length).toBeGreaterThan(0);
  });
});
