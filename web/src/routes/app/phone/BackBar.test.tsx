import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes, useLocation, useNavigate } from "react-router-dom";
import { describe, expect, it } from "vitest";

import { BackBar } from "./BackBar";

function Where() {
  return <div data-testid="where">{useLocation().pathname}</div>;
}

function Opener() {
  const navigate = useNavigate();
  return <button type="button" onClick={() => navigate("/app/sessions/rtx:1")}>open</button>;
}

function routes() {
  return (
    <Routes>
      <Route path="/app/history" element={<><Where /><Opener /></>} />
      <Route path="/app/overview" element={<Where />} />
      <Route path="/app/sessions/:id" element={<><Where /><BackBar parentLabel="Overview" parentPath="/app/overview" /></>} />
    </Routes>
  );
}

describe("BackBar", () => {
  it("after a deep link (no in-app history) goes to the parent, never out of the app", async () => {
    render(<MemoryRouter initialEntries={["/app/sessions/rtx:1"]}>{routes()}</MemoryRouter>);
    await userEvent.setup().click(screen.getByRole("button", { name: "‹ Overview" }));
    expect(screen.getByTestId("where")).toHaveTextContent("/app/overview");
  });

  it("returns to the previous in-app screen when there is one", async () => {
    render(<MemoryRouter initialEntries={["/app/history"]}>{routes()}</MemoryRouter>);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "open" }));
    await user.click(screen.getByRole("button", { name: "‹ Overview" }));
    expect(screen.getByTestId("where")).toHaveTextContent("/app/history");
  });
});
