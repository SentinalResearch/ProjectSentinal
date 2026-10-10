import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";

import { AuthProvider } from "../../../context/AuthContext";
import { ThemeProvider } from "../../../context/ThemeContext";
import { resetViewport, setViewport } from "../../../test-viewport";
import { AppShell } from "../AppShell";

function Where() {
  return <div data-testid="where">{useLocation().pathname}</div>;
}

function renderShell(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <ThemeProvider>
        <AuthProvider>
          <Routes>
            <Route path="/app" element={<AppShell />}>
              <Route path="*" element={<Where />} />
            </Route>
          </Routes>
        </AuthProvider>
      </ThemeProvider>
    </MemoryRouter>,
  );
}

describe("phone shell", () => {
  afterEach(resetViewport);

  it("shows the bottom tab bar and never the sidebar or its icon rail below 768 px", () => {
    setViewport(390);
    renderShell("/app/overview");
    const tabs = screen.getByRole("navigation", { name: "Tabs" });
    expect(within(tabs).getAllByRole("link").map((link) => link.textContent?.replace(/\W/g, ""))).toEqual([
      "Overview", "Terminals", "History", "Recipes", "More",
    ]);
    expect(screen.queryByRole("button", { name: /Search history/ })).not.toBeInTheDocument();
    expect(screen.queryByTitle("Expand sidebar")).not.toBeInTheDocument();
  });

  it("keeps the desktop sidebar and no tab bar at 768 px and wider", () => {
    setViewport(1440);
    renderShell("/app/overview");
    expect(screen.queryByRole("navigation", { name: "Tabs" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Search history/ })).toBeInTheDocument();
  });

  it("marks the tab that owns the current screen", () => {
    setViewport(390);
    const { unmount } = renderShell("/app/sessions/rtx:1");
    expect(screen.getByRole("link", { name: /Overview/ })).toHaveAttribute("aria-current", "page");
    unmount();
    renderShell("/app/settings/notifications");
    expect(screen.getByRole("link", { name: /More/ })).toHaveAttribute("aria-current", "page");
  });

  it("keeps the phone layout when a phone turns to landscape, but not for a short desktop window", () => {
    setViewport(844, { height: 390, coarse: true });
    const { unmount } = renderShell("/app/overview");
    expect(screen.getByRole("navigation", { name: "Tabs" })).toBeInTheDocument();
    unmount();
    setViewport(844, { height: 390, coarse: false });
    renderShell("/app/overview");
    expect(screen.queryByRole("navigation", { name: "Tabs" })).not.toBeInTheDocument();
  });

  it("navigates between tabs", async () => {
    setViewport(390);
    renderShell("/app/overview");
    await userEvent.setup().click(screen.getByRole("link", { name: /Terminals/ }));
    expect(screen.getByTestId("where")).toHaveTextContent("/app/split");
  });
});
