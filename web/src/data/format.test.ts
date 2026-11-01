import { describe, expect, it } from "vitest";

import { formatEventTime, machineSummary, sessionElapsedMs } from "./format";

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

describe("Machine summary", () => {
  it("prefers hardware, then the Sentinal version, then the OS", () => {
    expect(machineSummary({ hardware: "M2 Pro, 32 GB", daemonVersion: "0.2.8", os: "macOS" })).toBe("M2 Pro, 32 GB");
    expect(machineSummary({ hardware: "", daemonVersion: "0.2.8", os: "macOS" })).toBe("Sentinal 0.2.8");
    expect(machineSummary({ hardware: "", os: "Linux" })).toBe("Linux");
  });
});

describe("Session elapsed time", () => {
  const hour = 3_600_000;
  const now = 300 * hour;

  it("counts to now while a Session is live", () => {
    expect(sessionElapsedMs({ state: "RUNNING", createdAt: 10 * hour, updatedAt: 11 * hour }, now)).toBe(290 * hour);
  });

  it("stops at the end for a finished Session instead of growing forever", () => {
    // An INTERRUPTED Session from days ago used to show 219h and counting.
    const interrupted = { state: "INTERRUPTED", createdAt: 10 * hour, updatedAt: 12 * hour };
    expect(sessionElapsedMs(interrupted, now)).toBe(2 * hour);
    expect(sessionElapsedMs({ ...interrupted, finishedAt: 11 * hour }, now)).toBe(1 * hour);
    expect(sessionElapsedMs({ state: "COMPLETED", createdAt: 10 * hour, updatedAt: 10 * hour }, now)).toBe(0);
  });

  it("is never negative when clocks disagree", () => {
    expect(sessionElapsedMs({ state: "RUNNING", createdAt: now + hour, updatedAt: now + hour }, now)).toBe(0);
  });
});
