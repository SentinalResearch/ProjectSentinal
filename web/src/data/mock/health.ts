import type { DataAdapter } from "../adapter";
import type { HealthIncidents } from "../../protocol/HealthIncidents";
import type { HealthRange } from "../../protocol/HealthRange";
import type { HealthSummary } from "../../protocol/HealthSummary";
import type { MachineHealth } from "../../protocol/MachineHealth";
import type { MetricStat } from "../../protocol/MetricStat";
import { store } from "./store";

const RANGE_MS: Record<HealthRange, number> = { "1h": 3_600_000, "24h": 86_400_000, "7d": 604_800_000, "30d": 2_592_000_000 };

function stat(p50: number, p95: number, count = 120): MetricStat["stat"] {
  return { count, sum: p50 * count, min: p50 * 0.7, p50, p95, max: p95 * 1.4 };
}

/** Deterministic demo numbers: the mock "RTX-PC" is far away, "Mac Mini" is the control. */
function machineHealth(id: string, name: string, online: boolean, now: number, range: HealthRange): MachineHealth {
  const far = id === "rtx";
  const echo = far ? 126 : 13;
  const points = Array.from({ length: 24 }, (_, index) => ({
    at: now - RANGE_MS[range] + (index * RANGE_MS[range]) / 24,
    count: 40,
    p50: echo + (index % 5),
    p95: echo * 1.6 + (index % 7),
  }));
  const stats: MetricStat[] = online
    ? [
        { metric: "TYPING_ECHO_MS", stat: stat(echo, echo * 1.6), dropped: 0 },
        { metric: "TERMINAL_OPEN_MS", stat: stat(far ? 480 : 90, far ? 620 : 140, 12), dropped: 0 },
        { metric: "LINK_RTT_MS", stat: stat(far ? 118 : 6, far ? 125 : 8, 60), dropped: 0 },
        { metric: "DAEMON_RSS_BYTES", stat: stat(19_922_944, 21_000_000, 60), dropped: 0 },
      ]
    : [];
  return {
    machine_id: id,
    display_name: name,
    role: id === "mini" ? "CONTROL" : "WORKER",
    daemon_version: online ? "0.3.0" : null,
    reports_health: true,
    latest: stats,
    stats,
    series: online ? [{ metric: "TYPING_ECHO_MS", points }] : [],
  };
}

export const mockHealth: DataAdapter["health"] = {
  async summary(range) {
    const now = Date.now();
    const machines = [...store.machines]
      .sort((left, right) => (left.id === "mini" ? -1 : right.id === "mini" ? 1 : 0))
      .map((machine) => machineHealth(machine.id, machine.name, machine.status === "ONLINE", now, range));
    const summary: HealthSummary = { range, generated_at: now, storage_bytes: 40_960, machines };
    return summary;
  },
  async incidents() {
    const now = Date.now();
    const incidents: HealthIncidents = {
      incidents: [
        { machine_id: "mbp", kind: "OFFLINE", started_at: now - 14 * 60_000, ended_at: null },
        { machine_id: "rtx", kind: "RELAYED", started_at: now - 3_600_000, ended_at: now - 3_000_000, detail: "fra" },
      ],
    };
    return incidents;
  },
  reportUrl() {
    return undefined;
  },
};
