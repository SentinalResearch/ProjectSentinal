import { describe, expect, it } from "vitest";

import type { ProgressEstimate } from "../protocol/ProgressEstimate";
import { estimateShort, estimateText, finishClock, paceText } from "./estimate";

const counts = (over: Partial<ProgressEstimate> = {}): ProgressEstimate => ({
  basis: "COUNTS",
  confidence: "HIGH",
  left_secs: 240,
  per_minute: 40,
  ...over,
});

describe("estimate text (B1)", () => {
  it("says the time left, roughly when unsure", () => {
    expect(estimateText(counts())).toBe("about 4 m left");
    expect(estimateText(counts({ confidence: "LOW", left_secs: 45 }))).toBe("roughly 45 s left");
    expect(estimateText(counts({ left_secs: 0 }))).toBe("almost done");
    expect(estimateShort(counts())).toBe("4 m left");
  });

  it("calls a stall a stall instead of counting down", () => {
    const stalled = counts({ left_secs: undefined, stalled_secs: 180 });
    expect(estimateText(stalled)).toBe("no progress for 3 m");
    expect(estimateShort(stalled)).toBe("stalled?");
    expect(finishClock(stalled, 0)).toBeUndefined();
  });

  it("uses history when there are no counts", () => {
    const history: ProgressEstimate = { basis: "HISTORY", confidence: "MEDIUM", typical_secs: 360, runs: 4, left_secs: 120 };
    expect(estimateText(history)).toBe("usually 6 m · about 2 m left");
    expect(estimateText({ ...history, left_secs: undefined, longer_than_usual: true })).toBe(
      "taking longer than usual (usually 6 m)",
    );
  });

  it("gives the pace in the program's own unit", () => {
    expect(paceText(counts(), "epoch")).toBe("40 epoch/min");
    expect(paceText(counts({ per_minute: 0.5 }), "file")).toBe("30 file/h");
    expect(paceText(counts(), "%")).toBeUndefined();
  });
});
