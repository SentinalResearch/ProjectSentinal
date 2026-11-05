import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { dataAdapter } from "../../data";
import { LiveDataProvider, useLiveData } from "../../data/LiveDataContext";
import { HiddenSessionsPage } from "./HiddenSessionsPage";

function Lists() {
  const { terminals, hidden } = useLiveData();
  return <output aria-label="counts">{`${terminals.length}/${hidden.length}`}</output>;
}

function renderPage() {
  render(
    <MemoryRouter>
      <LiveDataProvider>
        <HiddenSessionsPage />
        <Lists />
      </LiveDataProvider>
    </MemoryRouter>,
  );
}

const counts = () => screen.getByLabelText("counts").textContent ?? "";

describe("hidden Sessions", () => {
  beforeEach(() => vi.restoreAllMocks());

  it("moves a hidden Session out of lists, restores it, and deletes it for good", async () => {
    renderPage();
    const user = userEvent.setup();
    await waitFor(() => expect(counts()).not.toBe("0/0"));
    const [total] = counts().split("/").map(Number);
    const sessions = await dataAdapter.getSessions();
    const finished = sessions.find((session) => ["COMPLETED", "FAILED", "INTERRUPTED"].includes(session.state));
    if (!finished) throw new Error("the fixtures have no finished Session");

    await dataAdapter.dismissSession(finished.id);
    await waitFor(() => expect(counts()).toBe(`${total - 1}/1`));

    await user.click(await screen.findByRole("button", { name: "Restore" }));
    await waitFor(() => expect(counts()).toBe(`${total}/0`));

    await dataAdapter.dismissSession(finished.id);
    await user.click(await screen.findByRole("button", { name: "Delete all 1…" }));
    await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Delete all" }));
    await waitFor(() => expect(counts()).toBe(`${total - 1}/0`));
  });

  it("shows a refusal instead of dropping it", async () => {
    vi.spyOn(dataAdapter, "restoreSession").mockRejectedValue(new Error("worker is offline"));
    renderPage();
    const sessions = await dataAdapter.getSessions();
    const finished = sessions.find((session) => ["COMPLETED", "FAILED", "INTERRUPTED"].includes(session.state));
    if (!finished) throw new Error("the fixtures have no finished Session");
    await dataAdapter.dismissSession(finished.id);
    await userEvent.setup().click(await screen.findByRole("button", { name: "Restore" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("worker is offline");
  });
});
