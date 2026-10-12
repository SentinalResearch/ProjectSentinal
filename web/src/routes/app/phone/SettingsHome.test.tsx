import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AuthProvider } from "../../../context/AuthContext";
import { ThemeProvider } from "../../../context/ThemeContext";
import { dataAdapter } from "../../../data";
import { resetViewport, setViewport } from "../../../test-viewport";
import { AppShell } from "../AppShell";
import { MoreRedirect, PhoneSettingsHome } from "./SettingsHome";

function Where() {
  return <div data-testid="where">{useLocation().pathname}</div>;
}

function renderSettings(path = "/app/settings") {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <ThemeProvider>
        <AuthProvider>
          <Routes>
            <Route path="/app" element={<AppShell />}>
              <Route path="settings" element={<PhoneSettingsHome />} />
              <Route path="more" element={<MoreRedirect />} />
              <Route path="*" element={<Where />} />
            </Route>
            <Route path="/login" element={<div>login page</div>} />
          </Routes>
        </AuthProvider>
      </ThemeProvider>
    </MemoryRouter>,
  );
}

describe("Settings (E1, formerly More)", () => {
  afterEach(() => {
    resetViewport();
    vi.restoreAllMocks();
    localStorage.clear();
  });

  it("is titled Settings, opens the menu from its title, and old /app/more links land here", async () => {
    setViewport(390);
    renderSettings("/app/more");
    expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Settings" })).toHaveAttribute("aria-haspopup", "dialog");
  });

  it("links to Machines, Notifications, and Account", async () => {
    setViewport(390);
    renderSettings();
    const user = userEvent.setup();
    await user.click(await screen.findByRole("link", { name: /Notifications/ }));
    expect(screen.getByTestId("where")).toHaveTextContent("/app/settings/notifications");
  });

  it("switches theme between Light, Dark, and Auto (following the system)", async () => {
    setViewport(390, { prefersDark: false });
    renderSettings();
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
    renderSettings();
    await userEvent.setup().click(await screen.findByRole("button", { name: "Sign out" }));
    expect(logout).toHaveBeenCalled();
  });
});
