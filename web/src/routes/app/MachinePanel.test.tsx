import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { AuthProvider } from "../../context/AuthContext";
import { ThemeProvider } from "../../context/ThemeContext";
import { dataAdapter } from "../../data";
import { resetSimulation } from "../../data/mock/simulation";
import { AppShell } from "./AppShell";
import { OverviewPage } from "./OverviewPage";

function renderMachine(id: string) {
  return render(
    <MemoryRouter initialEntries={[`/app/overview?machine=${id}`]}>
      <ThemeProvider>
        <AuthProvider>
          <Routes>
            <Route path="/app" element={<AppShell />}>
              <Route path="overview" element={<OverviewPage />} />
            </Route>
          </Routes>
        </AuthProvider>
      </ThemeProvider>
    </MemoryRouter>,
  );
}

describe("Machine view", () => {
  beforeAll(() => {
    Object.defineProperty(window, "innerWidth", { writable: true, configurable: true, value: 1440 });
  });
  beforeEach(() => resetSimulation());
  afterEach(() => vi.restoreAllMocks());

  it("shows system info, daemon status, and the link", async () => {
    renderMachine("rtx");
    const system = await screen.findByRole("region", { name: "System" }, { timeout: 5000 });
    expect(await within(system).findByText("Ubuntu 24.04 LTS", {}, { timeout: 5000 })).toBeInTheDocument();
    expect(within(system).getByText("6.8.0-45-generic")).toBeInTheDocument();
    const daemon = screen.getByRole("region", { name: "Sentinal daemon" });
    expect(within(daemon).getByText("0.1.9")).toBeInTheDocument();
    expect(within(daemon).getByText("100.64.0.10:7420")).toBeInTheDocument();
    expect(within(daemon).getByText(/ACTIVE/)).toBeInTheDocument();
  });

  it("lists each capture source on this Machine only, with its capture level", async () => {
    renderMachine("rtx");
    const sources = await screen.findByRole("region", { name: "Capture sources" }, { timeout: 5000 });
    expect(within(sources).getByText("Existing training run")).toBeInTheDocument();
    expect(within(sources).getAllByText("Adopted process").length).toBeGreaterThan(0);
    expect(within(sources).getByText("PROCESS_LOG")).toBeInTheDocument();
    // zsh (hooked) belongs to the Mac Mini, not RTX-PC.
    expect(within(sources).queryByText("zsh (hooked)")).not.toBeInTheDocument();
  });

  it("shows the busiest processes of each running Session", async () => {
    renderMachine("rtx");
    const processes = await screen.findByRole("region", { name: "Processes" }, { timeout: 5000 });
    expect(within(processes).getByText("Qwen Training")).toBeInTheDocument();
    expect(within(processes).getAllByText("worker").length).toBeGreaterThan(0);
  });

  it("charts Machine history and switches range on request", async () => {
    const metrics = vi.spyOn(dataAdapter.machineInsights, "metrics");
    renderMachine("rtx");
    const user = userEvent.setup();

    expect(await screen.findByRole("img", { name: /CPU, RAM, GPU, VRAM over the last hour/ }, { timeout: 5000 })).toBeInTheDocument();
    expect(metrics).toHaveBeenLastCalledWith("rtx", "HOUR");

    await user.click(screen.getByRole("button", { name: "24h" }));
    await waitFor(() => expect(metrics).toHaveBeenLastCalledWith("rtx", "DAY"));
    expect(await screen.findByRole("img", { name: /over the last 24 hours/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "24h" })).toHaveAttribute("aria-pressed", "true");
  });

  it("is not shown on the all-Machines Overview", async () => {
    render(
      <MemoryRouter initialEntries={["/app/overview"]}>
        <ThemeProvider>
          <AuthProvider>
            <Routes>
              <Route path="/app" element={<AppShell />}>
                <Route path="overview" element={<OverviewPage />} />
              </Route>
            </Routes>
          </AuthProvider>
        </ThemeProvider>
      </MemoryRouter>,
    );
    expect(await screen.findByRole("heading", { name: "Overview" }, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "System" })).not.toBeInTheDocument();
  });
});
