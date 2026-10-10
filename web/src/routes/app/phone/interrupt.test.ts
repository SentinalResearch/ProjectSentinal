import { describe, expect, it } from "vitest";

import { needsInterruptConfirm, runningSeconds } from "./interrupt";

describe("Ctrl-C confirmation threshold", () => {
  it("reads how long a Session has run from createdAt or its duration label", () => {
    expect(runningSeconds({ createdAt: 1_000 } as never, 61_000)).toBe(60);
    expect(runningSeconds({ runningFor: "1h31m" } as never, 0)).toBe(5_460);
    expect(runningSeconds({ runningFor: "45s" } as never, 0)).toBe(45);
    expect(runningSeconds({ runningFor: "2d3h" } as never, 0)).toBe(183_600);
    expect(runningSeconds({} as never, 0)).toBeNull();
  });

  it("confirms after 60 s, and when the running time is unknown", () => {
    expect(needsInterruptConfirm({ runningFor: "45s" } as never, 0)).toBe(false);
    expect(needsInterruptConfirm({ runningFor: "61s" } as never, 0)).toBe(true);
    expect(needsInterruptConfirm({} as never, 0)).toBe(true);
  });
});
