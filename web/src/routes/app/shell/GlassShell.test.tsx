import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AuthProvider } from "../../../context/AuthContext";
import { ThemeProvider } from "../../../context/ThemeContext";
import { dataAdapter } from "../../../data";
import { resetViewport, setViewport } from "../../../test-viewport";
import { AppShell } from "../AppShell";

function Where() {
  const location = useLocation();
  return <div data-testid="where">{`${location.pathname}${location.search}`}</div>;
}

function renderShell(path: string) {
  setViewport(1440);
  return render(
    <MemoryRouter initialEntries={[path]}>
      <ThemeProvider>
        <AuthProvider>
          <Routes>
            <Route path="/app" element={<AppShell />}>
              <Route path="*" element={<Where />} />
            </Route>
          </Routes>
        </AuthProvider>
      </ThemeProvider>
    </MemoryRouter>,
  );
}

async function islandReady() {
  const island = screen.getByRole("navigation", { name: "Sentinal" });
  // The mock's live data has arrived once the ticker names a Session.
  await within(island).findAllByText(/waiting|failed|stalled/);
  return island;
}

describe("glass shell prompt (GLASS_SPEC.md §5)", () => {
  afterEach(() => {
    resetViewport();
    vi.restoreAllMocks();
  });

  it("answers a waiting prompt from any page in three keystrokes: / y ↵", async () => {
    const send = vi.spyOn(dataAdapter, "sendInput").mockResolvedValue();
    renderShell("/app/history");
    await islandReady();
    const user = userEvent.setup();

    await user.keyboard("/");
    const command = screen.getByRole("combobox", { name: "Command" });
    expect(command).toHaveFocus();
    await user.keyboard("y");
    await user.keyboard("{Enter}");

    expect(send).toHaveBeenCalledTimes(1);
    expect(send.mock.calls[0][1]).toBe("y\r");
    expect(screen.queryByRole("dialog", { name: "Prompt" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /You are on History/ })).toBeInTheDocument();
  });

  it("⌘K opens it, go navigates, Escape closes and returns focus", async () => {
    renderShell("/app/overview");
    await islandReady();
    const user = userEvent.setup();

    await user.keyboard("{Control>}k{/Control}");
    await user.keyboard("go history");
    expect(screen.getByRole("option", { selected: true })).toHaveTextContent("go history");
    await user.keyboard("{Enter}");
    expect(await screen.findByRole("button", { name: /You are on History/ })).toBeInTheDocument();

    const button = screen.getByRole("button", { name: "Open the prompt" });
    await user.click(button);
    expect(screen.getByRole("dialog", { name: "Prompt" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Prompt" })).not.toBeInTheDocument();
    expect(button).toHaveFocus();
  });

  it("kill never interrupts directly: it opens the Ctrl-C confirmation", async () => {
    const signal = vi.spyOn(dataAdapter, "sendSignal").mockResolvedValue();
    renderShell("/app/overview");
    await islandReady();
    const user = userEvent.setup();

    await user.keyboard("/");
    await user.keyboard("kill ");
    await waitFor(() => expect(screen.getByRole("option", { selected: true })).toHaveTextContent(/^kill /));
    await user.keyboard("{Enter}");

    expect(signal).not.toHaveBeenCalled();
    const dialog = await screen.findByRole("dialog", { name: /Send Ctrl‑C to/ });
    await user.click(within(dialog).getByRole("button", { name: "Send Ctrl‑C" }));
    expect(signal).toHaveBeenCalledWith(expect.any(String), "SIGINT");
  });

  it("clicking where you are opens the prompt with `go ` typed", async () => {
    renderShell("/app/history");
    const island = await islandReady();
    const user = userEvent.setup();
    await user.click(within(island).getByRole("button", { name: /You are on History/ }));
    expect(screen.getByRole("combobox", { name: "Command" })).toHaveValue("go ");
  });

  it("does not steal / while typing in a field", async () => {
    renderShell("/app/overview");
    await islandReady();
    const field = document.createElement("input");
    document.body.appendChild(field);
    field.focus();
    const user = userEvent.setup();
    await user.keyboard("/");
    expect(screen.queryByRole("dialog", { name: "Prompt" })).not.toBeInTheDocument();
    field.remove();
  });

  it("keeps the pages you leave as sheets; ⌘[ and a click bring them back", async () => {
    renderShell("/app/history");
    await islandReady();
    const user = userEvent.setup();
    const go = async (text: string) => {
      await user.keyboard("/");
      await user.keyboard(text);
      await user.keyboard("{Enter}");
    };

    await go("go recipes");
    await go("go split");
    expect(await screen.findByRole("button", { name: /You are on Split view/ })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /^Back to/ }).map((b) => b.getAttribute("aria-label"))).toEqual([
      "Back to Recipes",
      "Back to History",
    ]);

    await user.keyboard("{Meta>}[[{/Meta}");
    expect(await screen.findByRole("button", { name: /You are on Recipes/ })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Back to History" }));
    expect(await screen.findByRole("button", { name: /You are on History/ })).toBeInTheDocument();
    // Real pages (split view mounts terminals) render slowly under jsdom.
  }, 20_000);

  it("shows the rest of the chosen suggestion as ghost text; Tab accepts it (5.A)", async () => {
    renderShell("/app/overview");
    await islandReady();
    const user = userEvent.setup();
    await user.keyboard("/");
    await user.keyboard("go hist");
    expect(screen.getByTestId("prompt-ghost")).toHaveTextContent("ory");
    await user.keyboard("{Tab}");
    expect(screen.getByRole("combobox", { name: "Command" })).toHaveValue("go history");
    expect(screen.queryByTestId("prompt-ghost")).not.toBeInTheDocument();
  });

  it("searches every Machine for what is typed and opens the hit (5.S)", async () => {
    const search = vi.spyOn(dataAdapter, "search").mockImplementation(async (query) =>
      query.text === "oom"
        ? [{
            id: "m:EVENTS:1",
            kind: "EVENTS",
            title: "Error · Qwen Training",
            detail: "CUDA out of memory",
            score: 54,
            machineId: "rtx-pc",
            to: "/app/history?q=cuda",
          }]
        : []);
    renderShell("/app/overview");
    await islandReady();
    const user = userEvent.setup();

    await user.keyboard("/");
    await user.keyboard("oom");
    const hit = await screen.findByRole("option", { name: /Error · Qwen Training/ });
    // Typed quickly: one search for the settled text, not one per keystroke.
    expect(search.mock.calls.filter(([query]) => query.text === "o")).toHaveLength(0);
    expect(search).toHaveBeenLastCalledWith(expect.objectContaining({ text: "oom" }));
    expect(hit).toHaveTextContent("CUDA out of memory");

    await user.click(hit);
    expect(await screen.findByRole("button", { name: /You are on History/ })).toBeInTheDocument();
  });
});
