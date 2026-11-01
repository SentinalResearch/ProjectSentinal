import { describe, expect, it } from "vitest";

import { formatEventTime } from "./format";

describe("Event time", () => {
  it("is 24-hour HH:mm:ss in local time, with zero padding", () => {
    expect(formatEventTime(new Date(2026, 9, 28, 9, 5, 7))).toBe("09:05:07");
    expect(formatEventTime(new Date(2026, 9, 28, 21, 30, 0))).toBe("21:30:00");
  });

  it("accepts epoch milliseconds", () => {
    const at = new Date(2026, 9, 28, 0, 0, 59).getTime();
    expect(formatEventTime(at)).toBe("00:00:59");
  });
});
