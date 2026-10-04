import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeAll, describe, expect, it } from "vitest";

import { AuthProvider } from "../../context/AuthContext";
import { ThemeProvider } from "../../context/ThemeContext";
import { store } from "../../data/mock/store";
import { AppShell } from "./AppShell";
import { OverviewPage } from "./OverviewPage";

function renderOverview() {
  return render(
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
}

describe("OverviewPage", () => {
  beforeAll(() => {
    // AppShell auto-collapses the sidebar under 1200px; jsdom defaults to 1024px.
    Object.defineProperty(window, "innerWidth", { writable: true, configurable: true, value: 1440 });
  });

  it("renders the shell, needs-you grid, and machine groups from the mock adapter", async () => {
    renderOverview();

    // Sidebar wordmark confirms AppShell rendered around the page.
    expect(await screen.findByText("Sentinal")).toBeInTheDocument();
    // Running session, only in its machine's table (not in Needs you).
    expect(await screen.findByText("Qwen Training")).toBeInTheDocument();
    expect(screen.getByText("Needs you")).toBeInTheDocument();
    expect(screen.getAllByText("Waiting for input")).toHaveLength(2);
    // Machine name appears once in the sidebar and once as a group heading.
    expect((await screen.findAllByText("MacBook")).length).toBeGreaterThanOrEqual(2);
  });

  it("resolves a waiting prompt via y/n and removes it from Needs you", async () => {
    renderOverview();
    const user = userEvent.setup();

    await screen.findAllByText("Waiting for input");
    // The adopted PROCESS_LOG prompt has no terminal input, so only one card offers y/n.
    const yButtons = await screen.findAllByRole("button", { name: "y" });
    expect(yButtons).toHaveLength(1);
    await user.click(yButtons[0]);

    await waitFor(() => expect(screen.getAllByText("Waiting for input")).toHaveLength(1), { timeout: 3000 });
    expect(screen.queryByRole("button", { name: "y" })).not.toBeInTheDocument();
  });

  it("shows authentication expiry separately from an offline Machine", async () => {
    const machine = store.machines.find((entry) => entry.id === "mbp");
    if (!machine) throw new Error("MacBook fixture missing");
    const previous = machine.status;
    machine.status = "AUTH_EXPIRED";
    renderOverview();

    expect((await screen.findAllByText(/Authentication expired · last seen/)).length).toBeGreaterThanOrEqual(2);
    machine.status = previous;
  });
});
