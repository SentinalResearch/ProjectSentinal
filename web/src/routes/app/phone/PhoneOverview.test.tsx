import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { dataAdapter } from "../../../data";
import { resetSimulation } from "../../../data/mock/simulation";
import { store } from "../../../data/mock/store";
import { resetViewport, setViewport } from "../../../test-viewport";
import { OverviewPage } from "../OverviewPage";
import { renderAt } from "./harness";

const routes = [{ path: "overview", element: <OverviewPage /> }];

describe("M1 · phone dashboard (MOBILE_GLASS_SPEC §2)", () => {
  beforeEach(() => {
    resetSimulation();
    setViewport(390);
  });
  afterEach(() => {
    resetViewport();
    vi.restoreAllMocks();
  });

  it("leads with how many need you, a full-width waiting card, then a lane per Machine", async () => {
    renderAt("/app/overview", routes);
    expect(await screen.findByText(/^\d+ needs? you\.$/, {}, { timeout: 5000 })).toBeInTheDocument();
    const needs = screen.getByRole("region", { name: "Needs you" });
    const waiting = within(needs).getByRole("article", { name: "npm install" });
    expect(within(waiting).getByRole("button", { name: "Answer y to npm install" })).toBeInTheDocument();
    expect(within(needs).getByRole("link", { name: /pytest/ })).toHaveAttribute("href", "/app/sessions/pytest");
    const rtx = screen.getByRole("region", { name: "RTX-PC" });
    expect(within(rtx).getByRole("link", { name: /Qwen Training/ })).toHaveTextContent("68 %");
    expect(screen.getByRole("region", { name: "MacBook" })).toHaveAttribute("data-offline", "true");
  });

  it("shows a delayed Machine as online with its last update", async () => {
    const mini = store.machines.find((machine) => machine.id === "mini")!;
    Object.assign(mini, { freshness: "STALE", syncedAt: Date.now() - 14_400 });
    renderAt("/app/overview", routes);
    const lane = await screen.findByRole("region", { name: "Mac Mini" }, { timeout: 5000 });
    expect(within(lane).getByText("online · updated 14 s ago")).toBeInTheDocument();
  });

  it("answers a waiting prompt from its card with the key and Enter", async () => {
    const send = vi.spyOn(dataAdapter, "sendInput").mockResolvedValue();
    renderAt("/app/overview", routes);
    await userEvent.setup().click(await screen.findByRole("button", { name: "Answer y to npm install" }, { timeout: 5000 }));
    expect(send).toHaveBeenCalledWith("npm", "y\r");
  });

  it("offers no y/n on a waiting Session whose capture cannot send input", async () => {
    renderAt("/app/overview", routes);
    const needs = await screen.findByRole("region", { name: "Needs you" }, { timeout: 5000 });
    expect(within(needs).getByRole("link", { name: /Tailed fine-tune/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Answer y to Tailed fine-tune/ })).not.toBeInTheDocument();
  });

  it("filters to what needs you from the island's count, with a way back to all", async () => {
    renderAt("/app/overview?needs=1", routes);
    expect(await screen.findByRole("region", { name: "Needs you" }, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "RTX-PC" })).not.toBeInTheDocument();
    await userEvent.setup().click(screen.getByRole("button", { name: "Show all" }));
    expect(await screen.findByRole("region", { name: "RTX-PC" })).toBeInTheDocument();
  });

  it("narrows to one Machine for `go <machine>`", async () => {
    renderAt("/app/overview?machine=mini", routes);
    expect(await screen.findByRole("region", { name: "Mac Mini" }, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "RTX-PC" })).not.toBeInTheDocument();
    expect(screen.getByText(/^Mac Mini ·/)).toBeInTheDocument();
  });
});
