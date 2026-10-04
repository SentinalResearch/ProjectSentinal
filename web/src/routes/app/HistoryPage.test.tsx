import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeAll, describe, expect, it, vi } from "vitest";

import { AuthProvider } from "../../context/AuthContext";
import { ThemeProvider } from "../../context/ThemeContext";
import { dataAdapter } from "../../data";
import type { HistoryResults } from "../../data/types";
import { AppShell } from "./AppShell";
import { HistoryPage } from "./HistoryPage";

function renderHistory(initialPath = "/app/history") {
  return render(
    <MemoryRouter initialEntries={[initialPath]}>
      <ThemeProvider>
        <AuthProvider>
          <Routes>
            <Route path="/app" element={<AppShell />}>
              <Route path="history" element={<HistoryPage />} />
            </Route>
          </Routes>
        </AuthProvider>
      </ThemeProvider>
    </MemoryRouter>,
  );
}

describe("HistoryPage", () => {
  beforeAll(() => {
    Object.defineProperty(window, "innerWidth", { writable: true, configurable: true, value: 1440 });
  });

  it("lists commands from every source (fixture history + live running/waiting sessions)", async () => {
    renderHistory();
    expect(await screen.findByText("cargo build")).toBeInTheDocument();
    expect(await screen.findByText("pytest tests/")).toBeInTheDocument();
    expect(await screen.findByText("npm install")).toBeInTheDocument();
  });

  it("narrows to one row via the Result filter chip", async () => {
    renderHistory();
    const user = userEvent.setup();
    await screen.findByText("cargo build");

    await user.click(screen.getByRole("button", { name: "Failed" }));

    await waitFor(() => expect(screen.queryByText("cargo build")).not.toBeInTheDocument());
    expect(screen.getByText("pytest tests/")).toBeInTheDocument();
  });

  it("pre-selects the Project filter and shows an empty state when nothing matches", async () => {
    renderHistory("/app/history?project=mentalanimaition&q=zzz-no-match");
    await waitFor(() => expect(screen.getByText("No commands match.")).toBeInTheDocument());

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Reset filters" }));
    await screen.findByText("cargo build");
  });

  it("labels an all-Machine search when one Machine is unavailable", async () => {
    const partial = [] as HistoryResults;
    partial.unavailableMachines = ["mbp"];
    const history = vi.spyOn(dataAdapter, "getHistory").mockResolvedValue(partial);
    renderHistory("/app/history?q=train");

    expect(await screen.findByRole("status")).toHaveTextContent("Partial results · unavailable: MacBook");
    history.mockRestore();
  });
});
