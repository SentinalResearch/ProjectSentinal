import { describe, expect, it, vi } from "vitest";

import { trailing } from "./trailing";

describe("trailing (T1.2)", () => {
  it("runs once after the last of many calls", () => {
    vi.useFakeTimers();
    try {
      const action = vi.fn();
      const fit = trailing(action, 100);
      for (let i = 0; i < 30; i++) {
        fit.call();
        vi.advanceTimersByTime(10);
      }
      expect(action).not.toHaveBeenCalled();
      vi.advanceTimersByTime(100);
      expect(action).toHaveBeenCalledTimes(1);
      fit.call();
      fit.cancel();
      vi.advanceTimersByTime(200);
      expect(action).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });
});
