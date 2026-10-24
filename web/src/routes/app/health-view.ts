/**
 * Words and thresholds for the health page (4.H §2.2). Thresholds are
 * documented here and in docs/SETUP.md; a value past one is highlighted
 * with the reason.
 */
import type { HealthIncident } from "../../protocol/HealthIncident";
import type { HealthMetric } from "../../protocol/HealthMetric";
import type { MachineHealth } from "../../protocol/MachineHealth";
import type { MetricStat } from "../../protocol/MetricStat";

export type Level = "ok" | "warn" | "bad";

export interface Reading {
  value: string;
  level: Level;
  reason?: string;
}

const MB = 1024 * 1024;

export function formatValue(metric: HealthMetric, value: number): string {
  switch (metric) {
    case "DAEMON_RSS_BYTES":
    case "LOG_DISK_BYTES":
      return value >= 1024 * MB ? `${(value / 1024 / MB).toFixed(1)} GB` : `${Math.round(value / MB)} MB`;
    case "DAEMON_CPU_PERCENT":
      return `${value.toFixed(1)} %`;
    case "LIVE_SESSIONS":
    case "UPLOAD_FAILURES":
    case "WS_RECONNECTS":
    case "VIEWER_RESYNCS":
    case "ERRORS":
      return `${Math.round(value)}`;
    default:
      return value >= 1_000 ? `${(value / 1_000).toFixed(1)} s` : `${Math.round(value)} ms`;
  }
}

/** Highlight thresholds on the median. */
const LIMITS: Partial<Record<HealthMetric, { warn: number; bad: number; reason: (value: string) => string }>> = {
  TYPING_ECHO_MS: {
    warn: 50,
    bad: 100,
    reason: (value) => `Typing feels late: ${value} per key. Usually distance or a relayed Tailscale path.`,
  },
  TERMINAL_OPEN_MS: { warn: 500, bad: 1_500, reason: (value) => `Terminals take ${value} to open.` },
  LINK_RTT_MS: { warn: 60, bad: 120, reason: (value) => `${value} from the control: far away or a slow path.` },
  UPLOAD_DELAY_MS: { warn: 3_000, bad: 10_000, reason: (value) => `Dashboard updates arrive ${value} late.` },
  DAEMON_RSS_BYTES: { warn: 300 * MB, bad: 1_024 * MB, reason: (value) => `sentinald uses ${value} of memory.` },
};

export function find(stats: MetricStat[], metric: HealthMetric): MetricStat | undefined {
  return stats.find((entry) => entry.metric === metric && entry.label === undefined);
}

export function reading(stats: MetricStat[], metric: HealthMetric): Reading | undefined {
  const entry = find(stats, metric);
  if (!entry || entry.stat.count === 0) return undefined;
  const value = formatValue(metric, entry.stat.p50);
  const limit = LIMITS[metric];
  if (!limit) return { value, level: "ok" };
  const level: Level = entry.stat.p50 >= limit.bad ? "bad" : entry.stat.p50 >= limit.warn ? "warn" : "ok";
  return level === "ok" ? { value, level } : { value, level, reason: limit.reason(value) };
}

/** The open relay incident for a Machine, as "Relayed · fra". */
export function pathText(incidents: HealthIncident[], machineId: string): string | undefined {
  const relayed = incidents.find((incident) =>
    incident.machine_id === machineId && incident.kind === "RELAYED" && incident.ended_at === null);
  if (!relayed) return undefined;
  return relayed.detail ? `Relayed · ${relayed.detail}` : "Relayed";
}

function clock(at: number): string {
  return new Date(at).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

function lasted(incident: HealthIncident, now: number): string {
  const ms = (incident.ended_at ?? now) - incident.started_at;
  const minutes = Math.round(ms / 60_000);
  return minutes < 1 ? `${Math.max(1, Math.round(ms / 1_000))} s` : minutes < 120 ? `${minutes} min` : `${Math.round(minutes / 60)} h`;
}

/** "RTX-PC offline 2 min at 09:14 (ongoing)". */
export function incidentText(incident: HealthIncident, name: string, now: number): string {
  const at = clock(incident.started_at);
  const ongoing = incident.ended_at === null ? " (ongoing)" : "";
  switch (incident.kind) {
    case "OFFLINE":
      return `${name} offline ${lasted(incident, now)} at ${at}${ongoing}`;
    case "DELAYED":
      return `${name} delayed ${lasted(incident, now)} at ${at}${ongoing}`;
    case "RELAYED": {
      const end = incident.ended_at === null ? " (ongoing)" : `–${clock(incident.ended_at)}`;
      return `${name} relayed through ${incident.detail ?? "a relay"} ${at}${end}`;
    }
    case "RESTARTED":
      return `${name} restarted${incident.detail ? ` with ${incident.detail}` : ""} at ${at}`;
    case "UPDATE_FAILED":
      return `${name} update${incident.detail ? ` to ${incident.detail}` : ""} failed at ${at}`;
    case "ERROR_BURST":
      return `${name} had many errors (${incident.detail ?? "unknown"}) at ${at}${ongoing}`;
  }
}

/** Empty-state text for a Machine with nothing to show, or undefined. */
export function emptyText(machine: MachineHealth): string | undefined {
  if (machine.stats.length > 0) return undefined;
  if (machine.role === "WORKER" && !machine.reports_health) {
    return "This worker runs an older Sentinal and does not report health. Update it from Settings → Machines.";
  }
  return "No data yet: health data appears after a few minutes of use.";
}
