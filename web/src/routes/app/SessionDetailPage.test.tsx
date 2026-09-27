import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeAll, describe, expect, it } from "vitest";

import { AuthProvider } from "../../context/AuthContext";
import { ThemeProvider } from "../../context/ThemeContext";
import { AppShell } from "./AppShell";
import { SessionDetailPage } from "./SessionDetailPage";

function renderSession(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <ThemeProvider>
        <AuthProvider>
          <Routes>
            <Route path="/app" element={<AppShell />}>
              <Route path="sessions/:id" element={<SessionDetailPage />} />
            </Route>
          </Routes>
        </AuthProvider>
      </ThemeProvider>
    </MemoryRouter>,
  );
}

describe("SessionDetailPage", () => {
  beforeAll(() => {
    Object.defineProperty(window, "innerWidth", { writable: true, configurable: true, value: 1440 });
  });

  it("renders the training top block and terminal for a RUNNING session", async () => {
    renderSession("/app/sessions/qwen");

    expect(await screen.findByRole("heading", { name: "Qwen Training" })).toBeInTheDocument();
    expect(screen.getByText("epoch 68/100")).toBeInTheDocument();
    expect(screen.getByText("LOSS")).toBeInTheDocument();
    expect(screen.getByTestId("terminal-host")).toBeInTheDocument();
  });

  it("opens and cancels the Ctrl-C confirm dialog without sending anything", async () => {
    renderSession("/app/sessions/qwen");
    const user = userEvent.setup();

    await screen.findByRole("heading", { name: "Qwen Training" });
    await user.click(screen.getByRole("button", { name: "Ctrl‑C…" }));

    expect(await screen.findByText("Send Ctrl‑C to Qwen Training?")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));

    await waitFor(() => expect(screen.queryByText("Send Ctrl‑C to Qwen Training?")).not.toBeInTheDocument());
    // Still RUNNING, not INTERRUPTED, since we cancelled.
    expect(screen.getAllByText("RUNNING").length).toBeGreaterThan(0);
  });

  it("renders the failed panel for a FAILED session with a result", async () => {
    renderSession("/app/sessions/pytest");

    expect(await screen.findByText("FAILURE SUMMARY")).toBeInTheDocument();
    expect(screen.getByText("3 failed")).toBeInTheDocument();
    expect(screen.getByText("182 passed · 42s")).toBeInTheDocument();
  });

  it("renders the waiting panel with y/n for a WAITING session", async () => {
    renderSession("/app/sessions/npm");

    expect(await screen.findByText("WAITING FOR INPUT")).toBeInTheDocument();
    expect(screen.getByText("Continue? [y/N]")).toBeInTheDocument();
  });
});
