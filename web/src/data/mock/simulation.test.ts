import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  advanceSimulation,
  applyScenario,
  getSimulationState,
  resetSimulation,
  setPlayback,
  simulationScenarios,
} from "./simulation";
import { store } from "./store";

describe("mock user-testing simulation", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    sessionStorage.clear();
    resetSimulation();
  });

  it("provides repeatable presets for the main research situations", () => {
    expect(simulationScenarios.map((scenario) => scenario.id)).toEqual([
      "mixed",
      "first-run",
      "needs-attention",
      "healthy-training",
      "capture-limits",
      "offline-worker",
    ]);

    applyScenario("needs-attention");
    const first = structuredClone(store.sessions);
    expect(new Set(first.map((session) => session.state))).toEqual(new Set(["WAITING", "FAILED", "STALLED"]));

    applyScenario("mixed");
    applyScenario("needs-attention");
    expect(store.sessions).toEqual(first);
  });

  it("models first-run and reduced-capture cases from explicit fixtures", () => {
    applyScenario("first-run");
    expect(store.machines).toHaveLength(1);
    expect(store.sessions).toEqual([]);
    expect(store.history).toEqual([]);

    applyScenario("capture-limits");
    expect(store.sessions.length).toBeGreaterThan(0);
    expect(new Set(store.sessions.map((session) => session.capture))).toEqual(
      new Set(["COMMANDS", "PROCESS_LOG", "PROCESS"]),
    );
  });

  it("rejects an unknown preset without changing the current data", () => {
    const before = structuredClone(store.sessions);
    expect(() => applyScenario("made-up" as never)).toThrow("unknown simulation scenario 'made-up'");
    expect(store.sessions).toEqual(before);
  });

  it("can pause and manually advance a deterministic training tick", () => {
    applyScenario("healthy-training");
    setPlayback("paused");
    const before = store.sessions[0]?.progress?.current;

    vi.advanceTimersByTime(60_000);
    expect(store.sessions[0]?.progress?.current).toBe(before);

    advanceSimulation();
    expect(store.sessions[0]?.progress?.current).toBe((before ?? 0) + 1);
    expect(getSimulationState()).toMatchObject({ scenario: "healthy-training", playback: "paused" });
  });

  it("stores settings only for the current browser tab", () => {
    applyScenario("offline-worker");
    setPlayback("fast");
    expect(sessionStorage.getItem("sentinal.simulation.scenario")).toBe("offline-worker");
    expect(sessionStorage.getItem("sentinal.simulation.playback")).toBe("fast");
    expect(localStorage.getItem("sentinal.simulation.scenario")).toBeNull();
  });
});
