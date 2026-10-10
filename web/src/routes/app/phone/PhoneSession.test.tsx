import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { dataAdapter } from "../../../data";
import { resetSimulation } from "../../../data/mock/simulation";
import { resetViewport, setViewport } from "../../../test-viewport";
import { SessionDetailPage } from "../SessionDetailPage";
import { SessionTerminalRoute } from "./PhoneTerminal";
import { renderAt } from "./harness";

const routes = [
  { path: "sessions/:id", element: <SessionDetailPage /> },
  { path: "sessions/:id/terminal", element: <SessionTerminalRoute /> },
];

describe("B3/B4/B5 · phone Session screens", () => {
  beforeEach(() => {
    resetSimulation();
    setViewport(390);
  });
  afterEach(() => {
    resetViewport();
    vi.restoreAllMocks();
  });

  it("B3: shows badges, a metric grid, health, and Open terminal / Stop", async () => {
    renderAt("/app/sessions/qwen", routes);
    expect(await screen.findByRole("heading", { name: "Qwen Training" }, { timeout: 5000 })).toBeInTheDocument();
    const grid = screen.getByRole("list", { name: "Metrics" });
    for (const label of ["LOSS", "GPU", "VRAM", "LAST ACTIVITY"]) {
      expect(within(grid).getByText(label)).toBeInTheDocument();
    }
    expect(screen.getByRole("link", { name: "Open terminal" })).toHaveAttribute("href", "/app/sessions/qwen/terminal");
    expect(screen.getByRole("button", { name: "Stop" })).toBeInTheDocument();
  });

  it("B3: Stop on a long job asks first; Cancel sends nothing and Send Ctrl-C interrupts", async () => {
    const sendSignal = vi.spyOn(dataAdapter, "sendSignal").mockResolvedValue();
    renderAt("/app/sessions/qwen", routes);
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Stop" }, { timeout: 5000 }));
    const sheet = screen.getByRole("dialog", { name: "Send Ctrl-C to Qwen Training?" });
    expect(sheet).toHaveTextContent("1h31m");
    await user.click(within(sheet).getByRole("button", { name: "Cancel" }));
    expect(sendSignal).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Stop" }));
    await user.click(screen.getByRole("button", { name: "Send Ctrl-C" }));
    expect(sendSignal).toHaveBeenCalledWith("qwen", "SIGINT");
  });

  it("B4: a waiting Session shows its prompt large with y/n that send the key and Enter", async () => {
    const sendInput = vi.spyOn(dataAdapter, "sendInput").mockResolvedValue();
    renderAt("/app/sessions/npm", routes);
    expect(await screen.findByText("Continue? [y/N]", {}, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.getByText(/WAITING FOR INPUT/)).toBeInTheDocument();
    await userEvent.setup().click(screen.getByRole("button", { name: "n" }));
    expect(sendInput).toHaveBeenCalledWith("npm", "n\r");
  });

  it("B5: a failed Session leads with the failure and a stat grid", async () => {
    vi.spyOn(dataAdapter, "getHistory").mockResolvedValue([]);
    renderAt("/app/sessions/pytest", routes);
    expect(await screen.findByText(/✕ FAILED · RTX-PC/, {}, { timeout: 5000 })).toBeInTheDocument();
    const stats = screen.getByRole("list", { name: "Result" });
    expect(within(stats).getByText("FAILED")).toBeInTheDocument();
    expect(within(stats).getByText("PASSED")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "See how it passed last time" })).not.toBeInTheDocument();
  });

  it("B5: links the last successful run of the same Command on the same Machine", async () => {
    vi.spyOn(dataAdapter, "getHistory").mockResolvedValue([
      { id: "h1", ok: true, command: "pytest tests/", duration: "40s", machine: "rtx", sessionId: "rtx-old" } as never,
    ]);
    renderAt("/app/sessions/pytest", routes);
    expect(await screen.findByRole("button", { name: "See how it passed last time" }, { timeout: 5000 })).toBeInTheDocument();
  });

  it("explains reduced capture with its badge and log, and offers no terminal or answer keys", async () => {
    renderAt("/app/sessions/adopted-log", routes);
    expect(await screen.findByRole("heading", { name: "Tailed fine-tune" }, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.getByLabelText("PROCESS_LOG capture")).toHaveTextContent("ADOPTED");
    expect(screen.getByText("Read-only log and Metrics — no terminal input.")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Watched log" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "y" })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Open terminal" })).not.toBeInTheDocument();
  });

  it("keeps a missing Session on its own Machine and offers a way back", async () => {
    renderAt("/app/sessions/rtx:999", routes);
    expect(await screen.findByText(/RTX-PC · Session missing/, {}, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "‹ Overview" })).toBeInTheDocument();
  });

  it("says when a deep link's Machine is offline or no longer enrolled", async () => {
    renderAt("/app/sessions/mbp:999", routes);
    expect(await screen.findByText(/MacBook · Machine offline/, {}, { timeout: 5000 })).toBeInTheDocument();
  });

  it("names a revoked Machine without borrowing another Machine's Session", async () => {
    renderAt("/app/sessions/gone:1", routes);
    expect(await screen.findByText(/Machine gone is not enrolled or is no longer available/, {}, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Open terminal" })).not.toBeInTheDocument();
  });
});

describe("C1/C2 · phone terminal", () => {
  beforeEach(() => resetSimulation());
  afterEach(() => {
    resetViewport();
    vi.restoreAllMocks();
  });

  it("is full screen with Detach, the Session, and the phone key bar", async () => {
    setViewport(390);
    renderAt("/app/sessions/qwen/terminal", routes);
    expect(await screen.findByRole("button", { name: "‹ Detach" }, { timeout: 5000 })).toBeInTheDocument();
    const keys = screen.getByLabelText("Terminal keys");
    expect(within(keys).getAllByRole("button").map((key) => key.textContent)).toEqual(["Ctrl‑C", "Esc", "Tab", "↑", "↓", "y", "n"]);
  });

  it("Ctrl-C on a long job opens the confirmation sheet instead of sending", async () => {
    setViewport(390);
    const sendSignal = vi.spyOn(dataAdapter, "sendSignal").mockResolvedValue();
    renderAt("/app/sessions/qwen/terminal", routes);
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Ctrl‑C" }, { timeout: 5000 }));
    expect(screen.getByRole("dialog", { name: "Send Ctrl-C to Qwen Training?" })).toBeInTheDocument();
    expect(sendSignal).not.toHaveBeenCalled();
  });

  it("shows no keys for a read-only Session", async () => {
    setViewport(390);
    renderAt("/app/sessions/pytest/terminal", routes);
    expect(await screen.findByRole("button", { name: "‹ Detach" }, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.queryByLabelText("Terminal keys")).not.toBeInTheDocument();
  });

  it("on desktop the terminal route returns to the Session page", async () => {
    setViewport(1440);
    renderAt("/app/sessions/qwen/terminal", routes);
    expect(await screen.findByRole("heading", { name: "Qwen Training" }, { timeout: 5000 })).toBeInTheDocument();
  });
});
