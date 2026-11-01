import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";

import { AuthProvider } from "../../../context/AuthContext";
import { ThemeProvider } from "../../../context/ThemeContext";
import { resetViewport, setViewport } from "../../../test-viewport";
import { AppShell } from "../AppShell";
import { PageTitle } from "./PageTitle";

function Where() {
  const location = useLocation();
  return <div data-testid="where">{`${location.pathname}${location.search}`}</div>;
}

function TopLevel({ title }: { title: string }) {
  return (
    <>
      <PageTitle>{title}</PageTitle>
      <Where />
    </>
  );
}

function renderShell(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <ThemeProvider>
        <AuthProvider>
          <Routes>
            <Route path="/app" element={<AppShell />}>
              <Route path="overview" element={<TopLevel title="Overview" />} />
              <Route path="split" element={<TopLevel title="Terminals" />} />
              <Route path="history" element={<TopLevel title="History" />} />
              <Route path="*" element={<Where />} />
            </Route>
          </Routes>
        </AuthProvider>
      </ThemeProvider>
    </MemoryRouter>,
  );
}

describe("phone shell (MOBILE_GLASS_SPEC.md §1)", () => {
  afterEach(resetViewport);

  it("has a glass island (logo, page, one live item, needs-you count) and a prompt pill — no top bar, tab bar, or menu", async () => {
    setViewport(390);
    renderShell("/app/overview");
    const island = screen.getByRole("navigation", { name: "Sentinal" });
    expect(within(island).getByRole("link", { name: "Dashboard" })).toBeInTheDocument();
    expect(within(island).getByText("Dashboard", { selector: "span" })).toBeInTheDocument();
    expect(await within(island).findByRole("link", { name: /^\d+ need you$/ })).toBeInTheDocument();
    expect(within(island).getByRole("link", { name: /failed$/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open the prompt" })).toHaveTextContent("go, run, find, y / n…");
    expect(screen.queryByRole("banner")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Search" })).not.toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Tabs" })).not.toBeInTheDocument();
  });

  it("uses the desktop glass shell from 768 px (START_HERE.md)", () => {
    setViewport(900);
    renderShell("/app/overview");
    expect(screen.getByRole("navigation", { name: "Sentinal" })).toBeInTheDocument();
    expect(screen.queryByText("go, run, find, y / n…")).not.toBeInTheDocument();
  });

  it("has the glass island and prompt at 1440 px (GLASS_SPEC.md)", () => {
    setViewport(1440);
    renderShell("/app/overview");
    expect(screen.getByRole("navigation", { name: "Sentinal" })).toBeInTheDocument();
    expect(screen.queryByText("go, run, find, y / n…")).not.toBeInTheDocument();
  });

  it("keeps the phone layout when a phone turns to landscape, but not for a short desktop window", () => {
    setViewport(844, { height: 390, coarse: true });
    const { unmount } = renderShell("/app/overview");
    expect(screen.getByText("go, run, find, y / n…")).toBeInTheDocument();
    unmount();
    setViewport(844, { height: 390, coarse: false });
    renderShell("/app/overview");
    expect(screen.queryByText("go, run, find, y / n…")).not.toBeInTheDocument();
  });

  it("the prompt pill opens the prompt; go navigates and the menu's job is done by the prompt", async () => {
    setViewport(390);
    renderShell("/app/overview");
    const user = userEvent.setup();
    const pill = screen.getByRole("button", { name: "Open the prompt" });
    await user.click(pill);
    await user.keyboard("go history");
    await user.keyboard("{Enter}");
    expect(screen.getByTestId("where")).toHaveTextContent("/app/history");
    await user.click(pill);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Prompt" })).not.toBeInTheDocument();
    expect(pill).toHaveFocus();
  });

  it("the needs-you count opens Overview filtered to Sessions that need you", async () => {
    setViewport(390);
    renderShell("/app/history");
    await userEvent.setup().click(await screen.findByRole("link", { name: /^\d+ need you$/ }));
    expect(screen.getByTestId("where")).toHaveTextContent("/app/overview?needs=1");
  });

  it("detail screens put ‹ back in the island", () => {
    setViewport(390);
    renderShell("/app/sessions/rtx:1");
    expect(screen.queryByRole("banner")).not.toBeInTheDocument();
  });
});
