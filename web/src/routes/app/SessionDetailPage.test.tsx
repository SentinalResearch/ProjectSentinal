import { render, screen, waitFor, within } from "@testing-library/react";
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

  it("puts the terminal in the left half and every detail in the right half, in order", async () => {
    renderSession("/app/sessions/qwen");
    await screen.findByRole("heading", { name: "Qwen Training" });

    const terminal = screen.getByRole("region", { name: "Terminal" });
    const details = screen.getByRole("region", { name: "Session details" });
    expect(terminal.compareDocumentPosition(details) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

    // Left: header (label, machine · cwd, attach status), xterm, then the key bar.
    expect(within(terminal).getByText("TERMINAL")).toBeInTheDocument();
    expect(within(terminal).getByText(/^RTX-PC · /)).toBeInTheDocument();
    expect(within(terminal).getByText(/attached|connecting…/)).toBeInTheDocument();
    expect(within(terminal).getByTestId("terminal-host")).toBeInTheDocument();
    expect(within(within(terminal).getByLabelText("Terminal keys")).getAllByRole("button").map((b) => b.textContent))
      .toEqual(["Ctrl‑C", "Esc", "Tab", "↑", "↓"]);
    expect(within(terminal).queryByRole("heading")).not.toBeInTheDocument();

    // Right: breadcrumb, header, state block, health card, events — in that order.
    const ordered = [
      within(details).getByRole("navigation", { name: "Breadcrumb" }),
      within(details).getByRole("heading", { name: "Qwen Training" }),
      within(details).getByText("LOSS"),
      within(details).getByText("HEALTH"),
      within(details).getByText("Events"),
    ];
    for (let i = 1; i < ordered.length; i += 1) {
      expect(ordered[i - 1].compareDocumentPosition(ordered[i]) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    }
    expect(within(details).queryByTestId("terminal-host")).not.toBeInTheDocument();
  });

  it("asks before the key bar's Ctrl-C sends anything", async () => {
    renderSession("/app/sessions/qwen");
    const user = userEvent.setup();
    await screen.findByRole("heading", { name: "Qwen Training" });

    const keys = within(screen.getByRole("region", { name: "Terminal" })).getByLabelText("Terminal keys");
    await user.click(within(keys).getByRole("button", { name: "Ctrl‑C" }));
    expect(await screen.findByText("Send Ctrl‑C to Qwen Training?")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.getAllByText("RUNNING").length).toBeGreaterThan(0);
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

    await user.dblClick(screen.getByRole("button", { name: "Resume terminal" }));

    expect(await screen.findByText("Part 2 of 2", { selector: "strong" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "pytest" })).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByText("pytest tests/")).not.toBeInTheDocument());
    expect(screen.getByText(/^FAILED — resumed at /)).toBeInTheDocument();
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
      screen.getByText("Log found at /home/me/Projects/TIM/train.log (stdout and stderr), but Sentinal cannot read it."),
    ).toBeInTheDocument();
    expect(screen.getByRole("note")).toHaveTextContent("Metrics only.");
    expect(screen.getByLabelText("Watch fully next time")).toHaveTextContent(
      "cd ~/'Projects/TIM' && sentinal watch -t 'Existing training run'",
    );
    expect(screen.queryByTestId("terminal-host")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Ctrl‑C…" })).not.toBeInTheDocument();
    expect(screen.queryByText("PROCESS_LOG")).not.toBeInTheDocument();
  });

  it("shows a tailed PROCESS_LOG Session as read-only log events with no input controls", async () => {
    renderSession("/app/sessions/adopted-log");

    expect(await screen.findByRole("heading", { name: "Tailed fine-tune" })).toBeInTheDocument();
    const badge = screen.getByLabelText("PROCESS_LOG capture");
    expect(badge).toHaveTextContent("ADOPTED·PROCESS LOG");
    expect(screen.getByRole("note")).toHaveTextContent("Read-only log and Metrics — no terminal input.");
    expect(
      screen.getByText("Watching /home/me/Projects/TIM/finetune.log (named with --log), read-only."),
    ).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Watched log" })).toHaveTextContent("epoch 3/8");
    // The prompt is shown, but Sentinal cannot answer it.
    expect(screen.getAllByText("Overwrite checkpoint? [y/N]").length).toBeGreaterThan(0);
    expect(screen.getByText(/Answer where the process runs/)).toBeInTheDocument();
    for (const name of ["y", "n", "Ctrl‑C…", "Run again", "Show output", "Resume terminal"]) {
      expect(screen.queryByRole("button", { name })).not.toBeInTheDocument();
    }
    expect(screen.queryByTestId("terminal-host")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Watch fully next time")).toHaveTextContent(
      "cd ~/'Projects/TIM' && sentinal watch -t 'Tailed fine-tune'",
    );
  });

  it("replaces the terminal with the Command view for a hooked COMMANDS shell", async () => {
    renderSession("/app/sessions/hooked-shell");

    expect(await screen.findByRole("heading", { name: "zsh (hooked)" })).toBeInTheDocument();
    expect(screen.getByLabelText("COMMANDS capture")).toHaveTextContent("SHELL HOOK·COMMANDS");
    expect(screen.getByRole("note")).toHaveTextContent("Commands and Metrics only — no terminal.");
    expect(screen.queryByTestId("terminal-host")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Ctrl‑C…" })).not.toBeInTheDocument();
  });

  it("labels an ordinary Session as UI · FULL with a terminal and no Watch hint", async () => {
    renderSession("/app/sessions/qwen");

    expect(await screen.findByLabelText("FULL capture")).toHaveTextContent("UI·FULL");
    expect(screen.queryByLabelText("Watch fully next time")).not.toBeInTheDocument();
  });

  it("shows an ended adopted process as exit code unknown, never as success", async () => {
    renderSession("/app/sessions/adopted-ended");

    expect(await screen.findByRole("heading", { name: "Old export job" })).toBeInTheDocument();
    expect(screen.getByText(/Ended · exit code unknown/)).toBeInTheDocument();
    expect(screen.getAllByText("ENDED").length).toBeGreaterThan(0);
    expect(screen.queryByText("COMPLETED")).not.toBeInTheDocument();
    expect(screen.queryByText(/Session ended — output saved/)).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Resume terminal" }),
    ).not.toBeInTheDocument();
  });

  it("keeps a missing notification target namespaced to its intended Machine", async () => {
    renderSession("/app/sessions/rtx%3A999");

    expect(await screen.findByRole("heading", { name: "Session 999" })).toBeInTheDocument();
    expect(screen.getByText(/RTX-PC · Session missing/)).toBeInTheDocument();
    expect(screen.getByText(/will not open a same-numbered Session elsewhere/)).toBeInTheDocument();
  });
});
