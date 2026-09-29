import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

import { RecipesPage } from "./RecipesPage";
import { SettingsPage } from "./SettingsPage";

const apiCalls = vi.hoisted(() => ({
  getRecipes: vi.fn(),
  getMachineTokens: vi.fn(),
  getNotificationRules: vi.fn(),
  getNotificationJournal: vi.fn(),
}));

vi.mock("../../data", () => ({
  isMockData: false,
  dataAdapter: {
    getRecipes: apiCalls.getRecipes,
    getMachineTokens: apiCalls.getMachineTokens,
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

  it("shows the connected Machine without pretending pairing and tokens work", () => {
    renderRoute("/app/settings/machines");

    expect(screen.getByRole("heading", { name: "Machines" })).toBeInTheDocument();
    expect(screen.getByText("Development Mac")).toBeInTheDocument();
    expect(screen.getByText(/Pairing and token management are not available yet/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Pair new machine" })).not.toBeInTheDocument();
    expect(screen.queryByText("Loading…")).not.toBeInTheDocument();
    expect(apiCalls.getMachineTokens).not.toHaveBeenCalled();
  });

  it("explains that Notifications are not implemented instead of loading forever", () => {
    renderRoute("/app/settings/notifications");

    expect(screen.getByRole("heading", { name: "Notifications are not available yet" })).toBeInTheDocument();
    expect(screen.queryByText("Loading…")).not.toBeInTheDocument();
    expect(apiCalls.getNotificationRules).not.toHaveBeenCalled();
    expect(apiCalls.getNotificationJournal).not.toHaveBeenCalled();
  });
});
