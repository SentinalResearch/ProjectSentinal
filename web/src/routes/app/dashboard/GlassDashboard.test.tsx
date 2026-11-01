import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";

import { LiveDataProvider } from "../../../data/LiveDataContext";
import { store } from "../../../data/mock/store";
import { resetSimulation } from "../../../data/mock/simulation";
import { GlassDashboard } from "./GlassDashboard";

function renderDashboard() {
  return render(
    <MemoryRouter initialEntries={["/app/overview"]}>
      <LiveDataProvider>
        <GlassDashboard />
      </LiveDataProvider>
    </MemoryRouter>,
  );
}

describe("glass dashboard (GLASS_SPEC.md §7)", () => {
  afterEach(() => resetSimulation());

  it("leads with how many need you and a tinted card for each", async () => {
    renderDashboard();
    expect(await screen.findByRole("heading", { level: 1, name: /^\d+ needs? you\.$/ })).toBeInTheDocument();
    const needs = screen.getByRole("region", { name: "Needs you" });
    expect(within(needs).getByRole("button", { name: /npm install, waiting on Mac Mini/ })).toBeInTheDocument();
    expect(within(needs).getByRole("button", { name: /pytest, failed on RTX-PC/ })).toBeInTheDocument();
    expect(within(needs).getByRole("button", { name: "Kill…" })).toBeInTheDocument();
  });

  it("answers a waiting prompt inline and drops the card", async () => {
    renderDashboard();
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Answer y to npm install" }));
    await waitFor(
      () => expect(screen.queryByRole("button", { name: "Answer y to npm install" })).not.toBeInTheDocument(),
      { timeout: 3_000 },
    );
  });

  it("keeps an unreachable Machine's last Sessions under its status (milestone 4.A)", async () => {
    const mini = store.machines.find((machine) => machine.id === "mini")!;
    Object.assign(mini, { status: "DISCONNECTED", lastSeen: "3m" });
    renderDashboard();
    const lane = await screen.findByRole("region", { name: "Mac Mini" });
    expect(within(lane).getByText(/^Disconnected · last seen 3m ago/)).toBeInTheDocument();
    expect(within(lane).getByText("Compression")).toBeInTheDocument();
  });

  it("feeds notable Events and offline Machines, not process churn", async () => {
    renderDashboard();
    const feed = await screen.findByRole("complementary", { name: "While you were away" });
    expect(await within(feed).findByText("Checkpoint saved")).toBeInTheDocument();
    expect(within(feed).getByText("MacBook went offline")).toBeInTheDocument();
  });
});
