import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeAll, describe, expect, it } from "vitest";

import { AuthProvider } from "../../context/AuthContext";
import { ThemeProvider } from "../../context/ThemeContext";
import { AppShell } from "./AppShell";
import { SplitViewPage } from "./SplitViewPage";

function renderSplit(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <ThemeProvider>
        <AuthProvider>
          <Routes>
            <Route path="/app" element={<AppShell />}>
              <Route path="split" element={<SplitViewPage />} />
            </Route>
          </Routes>
        </AuthProvider>
      </ThemeProvider>
    </MemoryRouter>,
  );
}

describe("SplitViewPage", () => {
  beforeAll(() => {
    Object.defineProperty(window, "innerWidth", { writable: true, configurable: true, value: 1440 });
  });

  it("defaults to the first 4 sessions and reports the count", async () => {
    renderSplit("/app/split");

    expect(await screen.findByText("4 of 4 terminals")).toBeInTheDocument();
    expect(screen.getByText("Qwen Training")).toBeInTheDocument();
    expect(screen.getByText("vLLM")).toBeInTheDocument();
    expect(screen.getByText("pytest")).toBeInTheDocument();
    expect(screen.getByText("shell")).toBeInTheDocument();
  });

  it("puts the ?focus= session first", async () => {
    renderSplit("/app/split?focus=npm");

    await screen.findByText("4 of 4 terminals");
    expect(screen.getByText("npm install")).toBeInTheDocument();
  });

  it("closing a pane removes it and drops the count", async () => {
    renderSplit("/app/split");
    const user = userEvent.setup();

    await screen.findByText("4 of 4 terminals");
    const qwenHeading = screen.getByText("Qwen Training");
    const pane = qwenHeading.closest("section");
    if (!pane) throw new Error("pane not found");

    await user.click(within(pane).getByTitle("Close pane"));

    await waitFor(() => expect(screen.queryByText("Qwen Training")).not.toBeInTheDocument());
    expect(screen.getByText("3 of 4 terminals")).toBeInTheDocument();
  });

  it("switching to layout 2 keeps only two panes", async () => {
    renderSplit("/app/split");
    const user = userEvent.setup();

    await screen.findByText("4 of 4 terminals");
    await user.click(screen.getByRole("button", { name: "2" }));

    await waitFor(() => expect(screen.getByText("2 of 4 terminals")).toBeInTheDocument());
  });

  it("shows each pane's owning Machine and independent connection state", async () => {
    renderSplit("/app/split?focus=compression");

    await screen.findByText("4 of 4 terminals");
    expect(screen.getAllByText(/RTX-PC · online/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/Mac Mini · online/).length).toBeGreaterThan(0);
  });
});
