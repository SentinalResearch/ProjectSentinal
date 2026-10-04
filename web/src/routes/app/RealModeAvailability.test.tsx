import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { RecipesPage } from "./RecipesPage";
import { SettingsPage } from "./SettingsPage";

const apiCalls = vi.hoisted(() => ({
  getRecipes: vi.fn(),
  registryList: vi.fn(),
  getNotificationRules: vi.fn(),
  getNotificationJournal: vi.fn(),
  getNotificationStatus: vi.fn(),
  setNotificationRule: vi.fn(),
}));

vi.mock("../../data", () => ({
  isMockData: false,
  dataAdapter: {
    getRecipes: apiCalls.getRecipes,
    registry: { list: apiCalls.registryList },
    notifications: {
      getRules: apiCalls.getNotificationRules,
      getJournal: apiCalls.getNotificationJournal,
      getStatus: apiCalls.getNotificationStatus,
      setRule: apiCalls.setNotificationRule,
    },
  },
}));

vi.mock("../../data/LiveDataContext", () => ({
  useLiveData: () => ({
    machines: [
      {
        id: "local-machine",
        name: "Development Mac",
        os: "macOS",
        hardware: "Apple Silicon",
        status: "ONLINE",
      },
    ],
    sessions: [],
    loading: false,
  }),
}));

function renderRoute(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route path="/app/recipes" element={<RecipesPage />} />
        <Route path="/app/settings/:tab" element={<SettingsPage />} />
      </Routes>
    </MemoryRouter>,
  );
}

describe("real-data feature availability", () => {
  beforeEach(() => {
    apiCalls.getRecipes.mockResolvedValue([]);
    apiCalls.getNotificationRules.mockResolvedValue({});
    apiCalls.getNotificationJournal.mockResolvedValue([]);
    apiCalls.getNotificationStatus.mockResolvedValue([]);
  });

  it("loads the real Recipe catalog without demonstration data", async () => {
    renderRoute("/app/recipes");

    expect(await screen.findByText("No recipes yet.")).toBeInTheDocument();
    expect(apiCalls.getRecipes).toHaveBeenCalledOnce();
  });

  it("shows the connected Machine and explains enrollment needs a control Machine", async () => {
    const { RegistryError } = await import("../../data/adapter");
    apiCalls.registryList.mockRejectedValue(new RegistryError("NOT_CONTROL", "this daemon is not a control Machine"));
    renderRoute("/app/settings/machines");

    expect(await screen.findByText(/This daemon is not a control Machine/)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Machines" })).toBeInTheDocument();
    expect(screen.getByText("Development Mac")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add Machine" })).not.toBeInTheDocument();
  });

  it("loads real notification rules, status, and journal", async () => {
    const user = userEvent.setup();
    renderRoute("/app/settings/notifications");

    expect(await screen.findByRole("heading", { name: "Notifications" })).toBeInTheDocument();
    expect(apiCalls.getNotificationRules).toHaveBeenCalledOnce();
    expect(apiCalls.getNotificationJournal).toHaveBeenCalledOnce();
    expect(apiCalls.getNotificationStatus).toHaveBeenCalledOnce();
    await user.click(screen.getByTitle("Default for new sessions: waiting"));
    expect(apiCalls.setNotificationRule).toHaveBeenCalledWith(
      "local-machine:default",
      expect.objectContaining({ waiting: false }),
    );
  });
});
