import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { dataAdapter } from "../../../data";
import { resetSimulation } from "../../../data/mock/simulation";
import { resetViewport, setViewport } from "../../../test-viewport";
import { OverviewPage } from "../OverviewPage";
import { renderAt } from "./harness";

const routes = [{ path: "overview", element: <OverviewPage /> }];

describe("B2 · phone Overview", () => {
  beforeEach(() => {
    resetSimulation();
    setViewport(390);
  });
  afterEach(() => {
    resetViewport();
    vi.restoreAllMocks();
  });

  it("shows the title, a new-Session button, Machine chips, and a Needs-you count", async () => {
    renderAt("/app/overview", routes);
    expect(await screen.findByRole("heading", { name: "Overview" }, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "New session" })).toBeInTheDocument();
    const chips = screen.getByRole("group", { name: "Machines" });
    expect(within(chips).getByRole("button", { name: /RTX-PC/ })).toBeInTheDocument();
    const needs = screen.getByRole("region", { name: "Needs you" });
    const count = Number(/(\d+)/.exec(within(needs).getByRole("heading").textContent ?? "")?.[1]);
    expect(count).toBeGreaterThan(0);
    expect(within(needs).getAllByRole("article")).toHaveLength(count);
  });

  it("puts the state badge and duration above the title, never on the title line", async () => {
    renderAt("/app/overview", routes);
    const row = await screen.findByRole("link", { name: /Qwen Training/ }, { timeout: 5000 });
    const [status, title, command] = Array.from(row.children);
    expect(status).toHaveTextContent(/RUNNING/);
    expect(status).toHaveTextContent("1h31m");
    expect(title).toHaveTextContent("Qwen Training");
    expect(title).not.toHaveTextContent(/RUNNING/);
    expect(command).toHaveTextContent("python train.py --config moe.yaml");
  });

  it("answers a waiting prompt from its card with the key and Enter", async () => {
    const sendInput = vi.spyOn(dataAdapter, "sendInput").mockResolvedValue();
    renderAt("/app/overview", routes);
    const card = await screen.findByRole("article", { name: /npm install/ }, { timeout: 5000 });
    expect(within(card).getByText("Continue? [y/N]")).toBeInTheDocument();
    await userEvent.setup().click(within(card).getByRole("button", { name: "y" }));
    expect(sendInput).toHaveBeenCalledWith("npm", "y\r");
  });

  it("offers no y/n on a waiting Session whose capture cannot send input", async () => {
    renderAt("/app/overview", routes);
    const card = await screen.findByRole("article", { name: /Tailed fine-tune/ }, { timeout: 5000 });
    expect(within(card).queryByRole("button", { name: "y" })).not.toBeInTheDocument();
  });
});
