import { Outlet } from "react-router-dom";

import { LiveDataProvider } from "./data/LiveDataContext";

/**
 * Test helper: live data around the routed page, without a real shell, so a
 * page test checks the page and not whichever shell the viewport selects.
 */
export function PageShell() {
  return (
    <LiveDataProvider>
      <main>
        <Outlet />
      </main>
    </LiveDataProvider>
  );
}
