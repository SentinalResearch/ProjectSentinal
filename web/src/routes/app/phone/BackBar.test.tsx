import { fireEvent, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useNavigate } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";

import { resetViewport, setViewport } from "../../../test-viewport";
import { BackBar } from "./BackBar";
import { renderAt, Where } from "./harness";

function Opener() {
  const navigate = useNavigate();
  return <button type="button" onClick={() => navigate("/app/sessions/rtx:1")}>open</button>;
}

const routes = [
  { path: "history", element: <><Where /><Opener /></> },
  { path: "overview", element: <Where /> },
  { path: "sessions/:id", element: <><Where /><BackBar parentLabel="Overview" parentPath="/app/overview" /></> },
];

describe("BackBar (the phone island's ‹ back, MOBILE_GLASS_SPEC §1)", () => {
  afterEach(resetViewport);

  it("after a deep link (no in-app history) goes to the parent, never out of the app", async () => {
    setViewport(390);
    renderAt("/app/sessions/rtx:1", routes);
    await userEvent.setup().click(screen.getByRole("button", { name: "Back to Overview" }));
    expect(screen.getByTestId("where")).toHaveTextContent("/app/overview");
  });

  it("returns to the previous in-app screen when there is one", async () => {
    setViewport(390);
    renderAt("/app/history", routes);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "open" }));
    await user.click(screen.getByRole("button", { name: "Back to Overview" }));
    expect(screen.getByTestId("where")).toHaveTextContent("/app/history");
  });

  it("leaves the island when the detail screen goes away", async () => {
    setViewport(390);
    renderAt("/app/sessions/rtx:1", routes);
    await userEvent.setup().click(screen.getByRole("button", { name: "Back to Overview" }));
    expect(screen.queryByRole("button", { name: "Back to Overview" })).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Dashboard" })).toBeInTheDocument();
  });

  it("a swipe from the left edge goes back; a swipe that starts mid-screen or drifts vertically does not", async () => {
    setViewport(390);
    renderAt("/app/sessions/rtx:1", routes);
    const swipe = (fromX: number, toX: number, toY = 300) => {
      fireEvent.touchStart(window, { touches: [{ clientX: fromX, clientY: 300 }] });
      fireEvent.touchEnd(window, { changedTouches: [{ clientX: toX, clientY: toY }] });
    };
    swipe(160, 320);
    swipe(8, 200, 420);
    swipe(8, 40);
    expect(screen.getByTestId("where")).toHaveTextContent("/app/sessions/rtx:1");
    swipe(8, 160);
    expect(screen.getByTestId("where")).toHaveTextContent("/app/overview");
  });
});
