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
    expect(screen.getByRole("region", { name: "Live resources" })).toBeInTheDocument();
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
    expect(screen.getByText(/Session ended — output saved/)).toHaveTextContent("FAILED");
    expect(screen.getByText(/Session ended — output saved/)).toHaveTextContent("saved with this Session");
    expect(screen.queryByRole("button", { name: "Ctrl‑C" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Ctrl‑C…" })).not.toBeInTheDocument();
    expect(screen.getByTestId("terminal-host")).toHaveAttribute("aria-readonly", "true");
  });

  it("continues once into persisted-looking history with isolated read-only output", async () => {
    renderSession("/app/sessions/pytest");
    const user = userEvent.setup();
    await screen.findByRole("heading", { name: "pytest" });

    await user.dblClick(screen.getByRole("button", { name: "Continue in new Session" }));

    expect(await screen.findByText("Generation 2 of 2", { selector: "strong" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "pytest" })).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByText("pytest tests/")).not.toBeInTheDocument());
    expect(screen.getByText(/Session pytest ended — continued as Session session-/)).toBeInTheDocument();
    const terminals = screen.getAllByTestId("terminal-host");
    expect(terminals).toHaveLength(2);
    expect(terminals[0]).toHaveAttribute("aria-readonly", "true");
    expect(terminals[1]).toHaveAttribute("aria-readonly", "false");
  });

  it("renders the waiting panel with y/n for a WAITING session", async () => {
    renderSession("/app/sessions/npm");

    expect(await screen.findByText("WAITING FOR INPUT")).toBeInTheDocument();
    expect(screen.getByText("Continue? [y/N]")).toBeInTheDocument();
  });

  it("shows an adopted PROCESS session read-only with metrics and a log-detected hint", async () => {
    renderSession("/app/sessions/adopted-train");

    expect(await screen.findByRole("heading", { name: "Existing training run" })).toBeInTheDocument();
    expect(screen.getByLabelText("PROCESS capture")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Live resources" })).toBeInTheDocument();
    expect(
      screen.getByText(
        "Log detected: /home/me/Projects/TIM/train.log (stdout and stderr) — tailing not enabled yet.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("note")).toHaveTextContent("no output, input, resize, or kill");
    expect(screen.queryByTestId("terminal-host")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Ctrl‑C…" })).not.toBeInTheDocument();
    expect(screen.queryByText("PROCESS_LOG")).not.toBeInTheDocument();
  });

  it("shows an ended adopted process as exit code unknown, never as success", async () => {
    renderSession("/app/sessions/adopted-ended");

    expect(await screen.findByRole("heading", { name: "Old export job" })).toBeInTheDocument();
    expect(screen.getByText(/Ended · exit code unknown/)).toBeInTheDocument();
    expect(screen.getAllByText("ENDED").length).toBeGreaterThan(0);
    expect(screen.queryByText("COMPLETED")).not.toBeInTheDocument();
    expect(screen.queryByText(/Session ended — output saved/)).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Continue in new Session" }),
    ).not.toBeInTheDocument();
  });
});
