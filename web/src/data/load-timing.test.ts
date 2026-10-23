import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { loadTimeline, markLoad, resetLoadTiming } from "./load-timing";

describe("page load timeline", () => {
  beforeEach(() => resetLoadTiming());
  afterEach(() => vi.restoreAllMocks());

  it("lists each step once, in load order, from the start of the page load", () => {
    const now = vi.spyOn(performance, "now");
    now.mockReturnValue(120.4);
    markLoad("snapshot");
    now.mockReturnValue(80);
    markLoad("script");
    now.mockReturnValue(340);
    markLoad("live");
    now.mockReturnValue(999);
    markLoad("snapshot");

    expect(loadTimeline().map(({ label, ms }) => [label, Math.round(ms)])).toEqual([
      ["app started", 80],
      ["snapshot", 120],
      ["live updates", 340],
    ]);
  });

  it("adds the control's own time from Server-Timing", () => {
    vi.spyOn(performance, "now").mockReturnValue(150);
    markLoad("snapshot");
    vi.spyOn(performance, "getEntriesByType").mockImplementation((type: string) =>
      type === "resource"
        ? ([
            {
              name: "https://control.example.ts.net/api/v1/control/snapshot",
              serverTiming: [
                { name: "auth", duration: 0.4, description: "" },
                { name: "snapshot", duration: 11.6, description: "" },
              ],
            },
          ] as unknown as PerformanceEntryList)
        : [],
    );
    expect(loadTimeline()).toEqual([{ step: "snapshot", label: "snapshot", ms: 150, serverMs: 12 }]);
  });
});
