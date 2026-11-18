import { describe, expect, it } from "vitest";

import { agentPercent, agentProgressParts, mapAgentState, type AgentStateView } from "./agentState";

const MIN = 60_000;

describe("mapAgentState (6.5)", () => {
  it("maps task, steps and both ETAs, and leaves a 6.4 report without them", () => {
    expect(
      mapAgentState({
        status: "WORKING",
        task: "migrate tests",
        step: 3,
        steps: 8,
        eta_secs: 720,
        task_started_at: 1_000,
        estimated_left_secs: 900,
        reported_at: 5_000,
      }),
    ).toEqual({
      status: "WORKING",
      task: "migrate tests",
      step: 3,
      steps: 8,
      etaSecs: 720,
      taskStartedAt: 1_000,
      estimatedLeftSecs: 900,
      reportedAt: 5_000,
    });
    expect(mapAgentState({ status: "DONE", reported_at: 1 })).toEqual({ status: "DONE", reportedAt: 1 });
  });
});

describe("agentProgressParts", () => {
  const state: AgentStateView = {
    status: "WORKING",
    task: "migrate tests",
    step: 3,
    steps: 8,
    etaSecs: 720,
    taskStartedAt: 0,
    reportedAt: 0,
  };

  it("labels the agent's claim and Sentinal's estimate apart", () => {
    expect(agentProgressParts(state, "claude", 4 * MIN)).toEqual({
      task: "migrate tests",
      steps: "step 3 of 8",
      said: "claude says ~12 m",
      pace: undefined,
      onTask: "4 m on task",
    });
    expect(agentProgressParts({ ...state, estimatedLeftSecs: 900 }, "claude", 4 * MIN).pace).toBe("about 15 m at this pace");
  });

  it("writes short and long times plainly and leaves out what is unknown", () => {
    const parts = agentProgressParts(
      { status: "WORKING", step: 2, etaSecs: 45, estimatedLeftSecs: 2 * 3600 + 5 * 60, reportedAt: 0 },
      "agent",
      0,
    );
    expect(parts).toEqual({ task: undefined, steps: "step 2", said: "agent says ~45 s", pace: "about 2 h 5 m at this pace", onTask: undefined });
    expect(agentProgressParts({ status: "WORKING", taskStartedAt: 0, task: "x", reportedAt: 0 }, "agent", 20_000).onTask).toBe(
      "under a minute on task",
    );
  });

  it("gives a ring percent only with a plan of at least one step", () => {
    expect(agentPercent(state)).toBe(38);
    expect(agentPercent({ ...state, steps: 0, step: 0 })).toBeUndefined();
    expect(agentPercent({ ...state, step: undefined })).toBeUndefined();
  });
});
