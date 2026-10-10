import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";

import { ThemeProvider } from "../../context/ThemeContext";
import { resetViewport, setViewport } from "../../test-viewport";
import { LandingPage } from "./LandingPage";

function Where() {
  return <div data-testid="where">{useLocation().pathname}</div>;
}

function renderLanding() {
  return render(
    <MemoryRouter initialEntries={["/"]}>
      <ThemeProvider>
        <Routes>
          <Route path="/" element={<LandingPage />} />
          <Route path="*" element={<Where />} />
        </Routes>
      </ThemeProvider>
    </MemoryRouter>,
  );
}

describe("A1/A2 · phone landing", () => {
  afterEach(resetViewport);

  it("leads with the headline and stacked Sign in / Set up this server actions", async () => {
    setViewport(390);
    renderLanding();
    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("Every terminal, every machine, at a glance.");
    const actions = screen.getByRole("group", { name: "Get started" });
    expect(within(actions).getAllByRole("button").map((button) => button.textContent)).toEqual(["Sign in", "Set up this server"]);
    await userEvent.setup().click(within(actions).getByRole("button", { name: "Set up this server" }));
    expect(screen.getByTestId("where")).toHaveTextContent("/setup");
  });

  it("shows a native needs-you preview and four stacked feature sections, not desktop mocks", () => {
    setViewport(390);
    renderLanding();
    expect(screen.getByRole("list", { name: "Needs you preview" })).toBeInTheDocument();
    for (const label of ["01 / 04", "02 / 04", "03 / 04", "04 / 04"]) {
      expect(screen.getByText(label)).toBeInTheDocument();
    }
    expect(screen.getByText("68 %")).toBeInTheDocument();
    expect(screen.queryByText("MAC MINI · NPM INSTALL")).not.toBeInTheDocument();
  });
});
