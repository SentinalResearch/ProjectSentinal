import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

import { RecipesPage } from "./RecipesPage";
import { SettingsPage } from "./SettingsPage";

const apiCalls = vi.hoisted(() => ({
  getRecipes: vi.fn(),
  registryList: vi.fn(),
  getNotificationRules: vi.fn(),
  getNotificationJournal: vi.fn(),
}));

vi.mock("../../data", () => ({
  isMockData: false,
  dataAdapter: {
    getRecipes: apiCalls.getRecipes,
    registry: { list: apiCalls.registryList },
    notifications: {
      getRules: apiCalls.getNotificationRules,
      getJournal: apiCalls.getNotificationJournal,
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
  it("explains that Recipes are not implemented instead of loading forever", () => {
    renderRoute("/app/recipes");

    expect(screen.getByRole("heading", { name: "Recipes are not available yet" })).toBeInTheDocument();
    expect(screen.queryByText("Loading…")).not.toBeInTheDocument();
    expect(apiCalls.getRecipes).not.toHaveBeenCalled();
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

  it("explains that Notifications are not implemented instead of loading forever", () => {
    renderRoute("/app/settings/notifications");

    expect(screen.getByRole("heading", { name: "Notifications are not available yet" })).toBeInTheDocument();
    expect(screen.queryByText("Loading…")).not.toBeInTheDocument();
    expect(apiCalls.getNotificationRules).not.toHaveBeenCalled();
    expect(apiCalls.getNotificationJournal).not.toHaveBeenCalled();
  });
});
