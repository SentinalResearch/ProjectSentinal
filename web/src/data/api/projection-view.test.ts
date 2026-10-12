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
    expect(view?.machines.get("machine-1")?.machine.status).toBe("OFFLINE");
    expect(view?.machines.get("machine-1")?.machine.freshness).toBe("STALE");
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
