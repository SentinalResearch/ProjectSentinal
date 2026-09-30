import { describe, expect, it } from "vitest";

import type { Session } from "./types";
import {
  HISTORY_OUTPUT_BUDGET_BYTES,
  HISTORY_WINDOW_GENERATIONS,
  resolveSessionLineage,
} from "./lineage";

function generation(id: number, continuedFrom?: number, machine = "machine-a"): Session {
  return {
    id: String(id),
    machine,
    name: "Stable history",
    shell: "/bin/sh",
    cwd: "/work",
    state: id === 40 ? "IDLE" : "COMPLETED",
    capture: "FULL",
    origin: "UI",
    continuedFrom: continuedFrom === undefined ? undefined : String(continuedFrom),
    createdAt: id,
  };
}

describe("resolveSessionLineage", () => {
  it("orders generations from root to latest from any selected generation", () => {
    const sessions = [generation(3, 2), generation(1), generation(2, 1)];

    const result = resolveSessionLineage(sessions, "2");

    expect(result.ordered.map((session) => session.id)).toEqual(["1", "2", "3"]);
    expect(result.latest.id).toBe("3");
    expect(result.visible.map((session) => session.id)).toEqual(["1", "2", "3"]);
    expect(result.bytesPerGeneration).toBe(Math.floor(HISTORY_OUTPUT_BUDGET_BYTES / 3));
  });

  it("rejects missing ancestors, branches, cycles, and cross-Machine edges", () => {
    expect(() => resolveSessionLineage([generation(2, 99)], "2")).toThrow(/missing Session 99/);
    expect(() =>
      resolveSessionLineage([generation(1), generation(2, 1), generation(3, 1)], "1"),
    ).toThrow(/more than one successor/);
    expect(() =>
      resolveSessionLineage([generation(1), generation(2, 1), generation(3, 1)], "2"),
    ).toThrow(/more than one successor/);
    expect(() => resolveSessionLineage([generation(1, 2), generation(2, 1)], "1")).toThrow(/cycle/);
    expect(() =>
      resolveSessionLineage([generation(1), generation(2, 1, "machine-b")], "1"),
    ).toThrow(/crosses Machines/);
  });

  it("windows long histories and reports omitted generations without deleting them", () => {
    const sessions = Array.from({ length: 40 }, (_, index) =>
      generation(index + 1, index === 0 ? undefined : index),
    );

    const latest = resolveSessionLineage(sessions, "40");
    expect(latest.visible).toHaveLength(HISTORY_WINDOW_GENERATIONS);
    expect(latest.visible[0].id).toBe("9");
    expect(latest.omittedBefore).toBe(8);
    expect(latest.omittedAfter).toBe(0);

    const older = resolveSessionLineage(sessions, "4");
    expect(older.visible[0].id).toBe("1");
    expect(older.visible.at(-1)?.id).toBe("32");
    expect(older.omittedAfter).toBe(8);
  });
});
