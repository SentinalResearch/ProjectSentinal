import { describe, expect, it } from "vitest";

import { runGroups } from "./groups";
import type { Session } from "./types";

const session = (id: string, group: string | undefined, state: Session["state"], extra: Partial<Session> = {}): Session =>
  ({ id, machine: "m", name: `Session ${id}`, state, group, ...extra }) as Session;

describe("runGroups", () => {
  it("gathers a run under its first part, with its lead, live members and finished ones", () => {
    const groups = runGroups([
      session("1", "layout-tests", "RUNNING", { progress: { current: 59, total: 100, unit: "gpu-min" } }),
      session("2", "layout-tests/gemma", "RUNNING", {
        estimate: { basis: "COUNTS", confidence: "HIGH", left_secs: 600 },
      }),
      session("3", "layout-tests/gemma/job 12", "RUNNING", {
        estimate: { basis: "COUNTS", confidence: "HIGH", left_secs: 900 },
      }),
      session("4", "layout-tests/qwen", "COMPLETED"),
      session("5", "layout-tests/ministral", "FAILED"),
      session("6", undefined, "RUNNING"),
    ]);
    expect(groups).toHaveLength(1);
    const run = groups[0];
    expect(run.name).toBe("layout-tests");
    expect(run.lead?.session.id).toBe("1");
    expect(run.percent).toBe(59);
    expect(run.live.map((entry) => entry.label)).toEqual(["gemma", "gemma / job 12"]);
    expect(run.finished.map((entry) => entry.id)).toEqual(["4", "5"]);
    // An ended lead is finished too.
    const ended = runGroups([session("9", "done-run", "COMPLETED"), session("10", "done-run/a", "COMPLETED")])[0];
    expect(ended.finished.map((entry) => entry.id)).toEqual(["9", "10"]);
    expect([run.running, run.failed]).toEqual([3, 1]);
    expect(run.leftSecs).toBe(900);
  });

  it("counts an IDLE part as finished, and failed when its Command failed", () => {
    const run = runGroups([
      session("1", "q/a", "IDLE", { health: { overall: "FAILED" } as Session["health"] }),
      session("2", "q/b", "IDLE", { health: { overall: "HEALTHY" } as Session["health"] }),
      session("3", "q/c", "RUNNING"),
    ])[0];
    expect(run.live.map((entry) => entry.session.id)).toEqual(["3"]);
    expect(run.finished.map((entry) => entry.id)).toEqual(["1", "2"]);
    expect([run.running, run.failed]).toEqual([1, 1]);
  });

  it("leaves hidden Sessions out", () => {
    expect(runGroups([session("1", "x", "COMPLETED", { dismissed: true })])).toEqual([]);
  });
});
