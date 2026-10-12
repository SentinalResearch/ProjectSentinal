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

describe("phone shell (MOBILE_SPEC.md → Navigation)", () => {
  afterEach(resetViewport);

  it("has a top bar with the logo, needs-you count, and search — never a tab bar or sidebar", async () => {
    setViewport(390);
    renderShell("/app/overview");
    const bar = await screen.findByRole("banner");
    expect(within(bar).getByRole("link", { name: "Overview" })).toBeInTheDocument();
    expect(within(bar).getByRole("link", { name: /^\d+ need you$/ })).toBeInTheDocument();
    expect(within(bar).getByRole("button", { name: "Search" })).toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Tabs" })).not.toBeInTheDocument();
    expect(screen.queryByTitle("Expand sidebar")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Overview" })).toHaveAttribute("aria-haspopup", "dialog");
  });

  it("keeps the desktop sidebar and no phone top bar at 768 px and wider", () => {
    setViewport(1440);
    renderShell("/app/overview");
    expect(screen.queryByRole("button", { name: "Search" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Search history/ })).toBeInTheDocument();
  });

  it("keeps the phone layout when a phone turns to landscape, but not for a short desktop window", () => {
    setViewport(844, { height: 390, coarse: true });
    const { unmount } = renderShell("/app/overview");
    expect(screen.getByRole("button", { name: "Search" })).toBeInTheDocument();
    unmount();
    setViewport(844, { height: 390, coarse: false });
    renderShell("/app/overview");
    expect(screen.queryByRole("button", { name: "Search" })).not.toBeInTheDocument();
  });

  it("the title opens the menu with search focused; ✕ and Escape close it and return focus", async () => {
    setViewport(390);
    renderShell("/app/overview");
    const user = userEvent.setup();
    const title = screen.getByRole("button", { name: "Overview" });
    await user.click(title);

    const menu = screen.getByRole("dialog", { name: "Menu" });
    expect(within(menu).getByRole("searchbox", { name: "Search" })).toHaveFocus();
    const pages = within(menu).getByRole("navigation", { name: "Pages" });
    expect(within(pages).getAllByRole("link").map((link) => link.querySelector("[data-page]")?.textContent)).toEqual([
      "Overview", "Terminals", "History", "Recipes", "Settings",
    ]);
    expect(within(pages).getByRole("link", { name: /^Overview/ })).toHaveAttribute("aria-current", "page");
    expect(within(menu).getByRole("group", { name: "Machines" })).toBeInTheDocument();
    expect(within(within(menu).getByRole("group", { name: "Projects" })).getByRole("link", { name: "TIM" }))
      .toHaveAttribute("href", "/app/projects/tim");

    await user.click(within(menu).getByRole("button", { name: "Close menu" }));
    expect(screen.queryByRole("dialog", { name: "Menu" })).not.toBeInTheDocument();
    expect(title).toHaveFocus();

    await user.click(screen.getByRole("button", { name: "Search" }));
    expect(screen.getByRole("searchbox", { name: "Search" })).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "Menu" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Search" })).toHaveFocus();
  });

  it("picking a page navigates and closes the menu", async () => {
    setViewport(390);
    renderShell("/app/overview");
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Overview" }));
    await user.click(within(screen.getByRole("navigation", { name: "Pages" })).getByRole("link", { name: /^Terminals/ }));
    expect(screen.getByTestId("where")).toHaveTextContent("/app/split");
    expect(screen.queryByRole("dialog", { name: "Menu" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Terminals" })).toBeInTheDocument();
  });

  it("typing searches Sessions, Recipes, and History, grouped by type", async () => {
    setViewport(390);
    renderShell("/app/overview");
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Search" }));
    await user.type(screen.getByRole("searchbox", { name: "Search" }), "qwen");

    const sessions = await screen.findByRole("list", { name: "Sessions" });
    expect(within(sessions).getByRole("link", { name: /Qwen Training/ })).toHaveAttribute("href", "/app/sessions/qwen");
    const recipes = await screen.findByRole("list", { name: "Recipes" });
    expect(within(recipes).getByRole("link", { name: /Qwen vLLM Server/ })).toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Pages" })).not.toBeInTheDocument();

    await user.clear(screen.getByRole("searchbox", { name: "Search" }));
    await user.type(screen.getByRole("searchbox", { name: "Search" }), "pytest");
    const history = await screen.findByRole("list", { name: "History" });
    const row = within(history).getByRole("link", { name: /pytest tests\// });
    await user.click(row);
    expect(screen.getByTestId("where")).toHaveTextContent("/app/history?q=pytest+tests%2F");

    await user.click(screen.getByRole("button", { name: "Search" }));
    await user.type(screen.getByRole("searchbox", { name: "Search" }), "zzzz-nothing");
    expect(await screen.findByText(/No matches for/)).toBeInTheDocument();
  });

  it("the needs-you count opens Overview filtered to Sessions that need you", async () => {
    setViewport(390);
    renderShell("/app/history");
    await userEvent.setup().click(await screen.findByRole("link", { name: /^\d+ need you$/ }));
    expect(screen.getByTestId("where")).toHaveTextContent("/app/overview?needs=1");
  });

  it("detail screens get no top bar", () => {
    setViewport(390);
    renderShell("/app/sessions/rtx:1");
    expect(screen.queryByRole("banner")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Search" })).not.toBeInTheDocument();
  });
});
