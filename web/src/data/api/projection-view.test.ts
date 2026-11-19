import { describe, expect, it } from "vitest";

import type { ControlProjectionSnapshot } from "../../protocol/ControlProjectionSnapshot";
import { applyProjectionMessage, projectionView } from "./projection-view";

function snapshot(machineCount = 1, sessionsPerMachine = 1): ControlProjectionSnapshot {
  return {
    schema_version: 1,
    control_machine_id: "machine-0",
    view_revision: 4,
    generated_at: 1_000,
    machines: Array.from({ length: machineCount }, (_, machineIndex) => ({
      machine_id: `machine-${machineIndex}`,
      projection_available: true,
      display_name: `Machine ${machineIndex}`,
      os: machineIndex % 2 === 0 ? "MACOS" : "LINUX",
      daemon_version: "0.4.1",
      capabilities: ["SESSIONS", "EVENTS", "METRICS"],
      registry_status: "ONLINE",
      freshness: machineIndex === 0 ? "LIVE" : "STALE",
      last_seen: 900,
      observed_at: 900,
      synced_at: 950,
      source_cursor: 10,
      sessions: Array.from({ length: sessionsPerMachine }, (_, sessionIndex) => ({
        id: sessionIndex + 1,
        continued_from: null,
        title: `Session ${sessionIndex + 1}`,
        state: "RUNNING",
        origin: "WATCH",
        capture: "FULL",
        capabilities: {
          can_attach: true,
          can_send_input: true,
          can_resize: true,
          can_kill_daemon_pty: true,
          exposes_commands: true,
          exposes_terminal_output: true,
          exposes_log_output: false,
          exposes_metrics: true,
        },
        health: null,
        created_at: 100,
        updated_at: 900,
        finished_at: null,
        exit_code: null,
      })),
      events: [],
      metrics: [],
      omitted_sessions: 0,
    })),
    omitted_machines: 0,
    omitted_sessions: 0,
    truncated: false,
  };
}

describe("control projection view", () => {
  it("maps 10 Machines and 1,000 Session summaries without any query callback", () => {
    const view = projectionView(snapshot(10, 100), 1_000);
    expect(view?.machines.size).toBe(10);
    expect([...view!.machines.values()].flatMap((machine) => machine.sessions)).toHaveLength(1_000);
    // A delayed projection is not an offline Machine: the heartbeat still says ONLINE.
    expect(view?.machines.get("machine-1")?.machine.status).toBe("ONLINE");
    expect(view?.machines.get("machine-1")?.machine.freshness).toBe("STALE");
  });

  it("never reports the daemon version as a Machine's hardware", () => {
    const machine = projectionView(snapshot(), 1_000)?.machines.get("machine-0")?.machine;
    expect(machine?.hardware).toBe("");
    expect(machine?.daemonVersion).toBe("0.4.1");
  });

  it("stops a finished Session's duration when it ended", () => {
    const ended = snapshot();
    const session = ended.machines[0].sessions[0];
    ended.machines[0].sessions[0] = { ...session, state: "INTERRUPTED", created_at: 0, updated_at: 120_000, finished_at: 60_000 };
    const now = 900 * 3_600_000;
    const mapped = projectionView(ended, now)?.machines.get("machine-0")?.sessions[0];
    expect(mapped?.runningFor).toBe("1m");
  });

  it("applies ordered Machine-scoped deltas and rejects revision gaps", () => {
    const view = projectionView(snapshot(), 1_000)!;
    const events = applyProjectionMessage(view, {
      type: "PROJECTION_DELTA",
      machine_id: "machine-0",
      from_revision: 4,
      to_revision: 5,
      changes: [{
        type: "METRICS_REPLACE",
        cursor: 11,
        metrics: {
          scope: "SESSION",
          session_id: 1,
          at: 1_100,
          cpu_percent: 50,
          ram_used_bytes: 1_073_741_824,
          ram_total_bytes: null,
          gpu_percent: null,
          vram_used_bytes: null,
          vram_total_bytes: null,
        },
      }],
    }, 1_100);
    expect(events?.some((event) => event.type === "SESSION_METRICS")).toBe(true);
    expect(view.machines.get("machine-0")?.sessions[0].metrics?.ramGB).toBe(1);
    expect(view.revision).toBe(5);

    expect(applyProjectionMessage(view, {
      type: "PROJECTION_DELTA",
      machine_id: "machine-0",
      from_revision: 7,
      to_revision: 8,
      changes: [],
    }, 1_200)).toBeNull();
  });

  it("requires a snapshot when a legacy delta has no Machine identity", () => {
    const view = projectionView(snapshot(), 1_000)!;
    expect(applyProjectionMessage(view, {
      type: "PROJECTION_DELTA",
      from_revision: 4,
      to_revision: 5,
      changes: [],
    }, 1_100)).toBeNull();
  });
});

describe("the prompt of a WAITING Session", () => {
  const base = () => {
    const data = snapshot();
    const machine = data.machines[0];
    machine.sessions[0].state = "WAITING";
    machine.events = [
      { id: 40, session_id: machine.sessions[0].id, at: 1, facts: { category: "WARNING" } },
      { id: 41, session_id: machine.sessions[0].id, at: 2, facts: { category: "WAITING" } },
    ] as typeof machine.events;
    return data;
  };

  it("remembers which Event holds the text the control does not store", () => {
    const view = projectionView(base(), 3)!;
    const session = [...view.machines.values()][0].sessions[0];
    expect(session.promptEventId).toBe(41);
    expect(session.prompt).toBeUndefined();
  });

  it("forgets the old question as soon as the Session is no longer waiting", () => {
    const data = base();
    const view = projectionView(data, 3)!;
    const machine = [...view.machines.values()][0];
    machine.sessions[0] = { ...machine.sessions[0], prompt: "Old question? [y/N]" };
    const events = applyProjectionMessage(view, {
      type: "PROJECTION_DELTA",
      machine_id: data.machines[0].machine_id,
      from_revision: view.revision,
      to_revision: view.revision + 1,
      changes: [{ type: "SESSION_UPSERT", cursor: 9, session: { ...data.machines[0].sessions[0], state: "RUNNING" } }],
    } as never, 4);
    expect(events).not.toBeNull();
    expect(machine.sessions[0].prompt).toBeUndefined();
  });

  it("a new question replaces the Event to fetch and drops the old text", () => {
    const data = base();
    const view = projectionView(data, 3)!;
    const machine = [...view.machines.values()][0];
    machine.sessions[0] = { ...machine.sessions[0], prompt: "Old question? [y/N]" };
    applyProjectionMessage(view, {
      type: "PROJECTION_DELTA",
      machine_id: data.machines[0].machine_id,
      from_revision: view.revision,
      to_revision: view.revision + 1,
      changes: [{
        type: "EVENT_APPEND",
        cursor: 9,
        event: { id: 55, session_id: data.machines[0].sessions[0].id, at: 9, facts: { category: "WAITING" } },
      }],
    } as never, 4);
    expect(machine.sessions[0].promptEventId).toBe(55);
    expect(machine.sessions[0].prompt).toBeUndefined();
  });
  it("keeps the program's own counts when the Session row brings only Health's percentage (QA)", () => {
    const data = base();
    const view = projectionView(data, 3)!;
    const machine = [...view.machines.values()][0];
    const sessionId = data.machines[0].sessions[0].id;
    const appended = applyProjectionMessage(view, {
      type: "PROJECTION_DELTA",
      machine_id: data.machines[0].machine_id,
      from_revision: view.revision,
      to_revision: view.revision + 1,
      changes: [{
        type: "EVENT_APPEND",
        cursor: 9,
        event: { id: 56, session_id: sessionId, at: 9, facts: { category: "PROGRESS", current: 7, total: 40, unit: "EPOCH" } },
      }],
    } as never, 4);
    const counts = { current: 7, total: 40, unit: "epoch" };
    expect(machine.sessions[0].progress).toEqual(counts);
    expect(appended?.find((event) => event.type === "EVENT_APPENDED")).toMatchObject({ sessionPatch: { progress: counts } });

    const health = { progress: 17.5, activity: "GOOD", resources: "GOOD", stability: "UNKNOWN", errors: 0, overall: "HEALTHY" };
    applyProjectionMessage(view, {
      type: "PROJECTION_DELTA",
      machine_id: data.machines[0].machine_id,
      from_revision: view.revision,
      to_revision: view.revision + 1,
      changes: [{ type: "SESSION_UPSERT", cursor: 10, session: { ...data.machines[0].sessions[0], health } }],
    } as never, 5);
    expect(machine.sessions[0].progress).toEqual(counts);
  });
});
