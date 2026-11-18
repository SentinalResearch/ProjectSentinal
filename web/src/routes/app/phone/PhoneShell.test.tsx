import { resetLifecycle } from "../../../data/lifecycle";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AuthProvider } from "../../../context/AuthContext";
import { ThemeProvider } from "../../../context/ThemeContext";
import { dataAdapter } from "../../../data";
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
  afterEach(() => {
    resetLifecycle();
    resetViewport();
    vi.restoreAllMocks();
  });

  it("over the terminal the island docks to the top edge and the prompt pill steps aside", () => {
    setViewport(390);
    renderShell("/app/sessions/qwen/terminal");
    expect(screen.getByRole("navigation", { name: "Sentinal" })).toHaveAttribute("data-docked", "true");
    expect(screen.queryByRole("button", { name: "Open the prompt" })).not.toBeInTheDocument();
  });

  it("elsewhere the island floats", () => {
    setViewport(390);
    renderShell("/app/overview");
    expect(screen.getByRole("navigation", { name: "Sentinal" })).not.toHaveAttribute("data-docked");
  });

  it("has an island (logo, page, one live item, unread count) and a prompt pill — no top bar, tab bar, or menu", async () => {
    setViewport(390);
    renderShell("/app/overview");
    const island = screen.getByRole("navigation", { name: "Sentinal" });
    expect(within(island).getByRole("link", { name: "Overview" })).toBeInTheDocument();
    expect(within(island).getByText("Overview", { selector: "span" })).toBeInTheDocument();
    expect(await within(island).findByRole("button", { name: /^\d+ unread notifications?$/ })).toBeInTheDocument();
    expect(within(island).getByRole("link", { name: /failed$/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open the prompt" })).toHaveTextContent("go, run, find, y / n…");
    expect(screen.queryByRole("banner")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Search" })).not.toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Tabs" })).not.toBeInTheDocument();
    // The prompt is the fast path; tapping the page name shows the main places.
    expect(screen.queryByRole("group", { name: "Places" })).not.toBeInTheDocument();
    await userEvent.setup().click(within(island).getByRole("button", { name: /Go somewhere/ }));
    const places = screen.getByRole("group", { name: "Places" });
    for (const name of ["Overview", "Machines", "History", "Recipes", "Settings"]) {
      expect(within(places).getByRole("link", { name })).toBeInTheDocument();
    }
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

  it("the prompt sheet is typed on the built-in keyboard: y, ⏎ answers the waiting prompt (M2)", async () => {
    setViewport(390);
    const send = vi.spyOn(dataAdapter, "sendInput").mockResolvedValue();
    renderShell("/app/history");
    const user = userEvent.setup();
    await screen.findByRole("button", { name: /^\d+ unread notifications?$/ });
    await user.click(screen.getByRole("button", { name: "Open the prompt" }));
    const command = screen.getByRole("combobox", { name: "Command" });
    expect(command).toHaveAttribute("inputmode", "none");
    const keys = screen.getByRole("group", { name: "Terminal keyboard" });
    expect(within(keys).queryByRole("button", { name: "Ctrl" })).not.toBeInTheDocument();
    await user.click(within(keys).getByRole("button", { name: "y" }));
    expect(command).toHaveValue("y");
    await user.click(within(keys).getByRole("button", { name: "Enter" }));
    expect(send).toHaveBeenCalledWith(expect.any(String), "y\r");
    expect(screen.queryByRole("dialog", { name: "Prompt" })).not.toBeInTheDocument();
  });

  it("the unread count opens the inbox under the island; Open goes to that Session", async () => {
    setViewport(390);
    renderShell("/app/history");
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: /^\d+ unread notifications?$/ }));
    const inbox = screen.getByRole("dialog", { name: "Notifications" });
    expect(within(inbox).getByRole("region", { name: "Needs you" })).toBeInTheDocument();
    await user.click(within(inbox).getAllByRole("button", { name: "Open" })[0]);
    expect(screen.getByTestId("where")).toHaveTextContent(/^\/app\/sessions\//);
    expect(screen.queryByRole("dialog", { name: "Notifications" })).not.toBeInTheDocument();
  });

  it("detail screens put ‹ back in the island", () => {
    setViewport(390);
    renderShell("/app/sessions/rtx:1");
    expect(screen.queryByRole("banner")).not.toBeInTheDocument();
  });
});
