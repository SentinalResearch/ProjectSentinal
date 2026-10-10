import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AuthProvider } from "../../../context/AuthContext";
import { ThemeProvider } from "../../../context/ThemeContext";
import { dataAdapter } from "../../../data";
import { resetViewport, setViewport } from "../../../test-viewport";
import { AppShell } from "../AppShell";
import { MorePage } from "./MorePage";

function Where() {
  return <div data-testid="where">{useLocation().pathname}</div>;
}

function renderMore() {
  return render(
    <MemoryRouter initialEntries={["/app/more"]}>
      <ThemeProvider>
        <AuthProvider>
          <Routes>
            <Route path="/app" element={<AppShell />}>
              <Route path="more" element={<MorePage />} />
              <Route path="*" element={<Where />} />
            </Route>
            <Route path="/login" element={<div>login page</div>} />
          </Routes>
        </AuthProvider>
      </ThemeProvider>
    </MemoryRouter>,
  );
}

describe("More (E1)", () => {
  afterEach(() => {
    resetViewport();
    vi.restoreAllMocks();
    localStorage.clear();
  });

  it("links to Machines, Notifications, and Account", async () => {
    setViewport(390);
    renderMore();
    const user = userEvent.setup();
    await user.click(await screen.findByRole("link", { name: /Notifications/ }));
    expect(screen.getByTestId("where")).toHaveTextContent("/app/settings/notifications");
  });

  it("switches theme between Light, Dark, and Auto (following the system)", async () => {
    setViewport(390, { prefersDark: false });
    renderMore();
    const user = userEvent.setup();
    await user.click(await screen.findByRole("radio", { name: "Dark" }));
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    await user.click(screen.getByRole("radio", { name: "Auto" }));
    expect(screen.getByRole("radio", { name: "Auto" })).toBeChecked();
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
  });

  it("signs out from a destructive text action", async () => {
    setViewport(390);
    const logout = vi.spyOn(dataAdapter.auth, "logout");
    renderMore();
    await userEvent.setup().click(await screen.findByRole("button", { name: "Sign out" }));
    expect(logout).toHaveBeenCalled();
  });
});
