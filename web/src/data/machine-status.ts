import type { Machine } from "./types";

/**
 * Online by heartbeat, but the control's copy of its Sessions is more than a
 * few seconds old. Routed actions still work; only the summary is behind.
 */
export function isDelayed(machine: Pick<Machine, "status" | "freshness">): boolean {
  return machine.status === "ONLINE" && machine.freshness === "STALE";
}

function age(milliseconds: number): string {
  const seconds = Math.max(0, Math.floor(milliseconds / 1_000));
  if (seconds < 60) return `${seconds} s`;
  const minutes = Math.floor(seconds / 60);
  return minutes < 60 ? `${minutes} min` : `${Math.floor(minutes / 60)} h`;
}

/** Lower-case status for compact surfaces: "online", "online · updated 14 s ago", "offline". */
export function machineStatusText(machine: Pick<Machine, "status" | "freshness" | "syncedAt">, now: number): string {
  if (isDelayed(machine)) {
    return machine.syncedAt === undefined ? "online · delayed" : `online · updated ${age(now - machine.syncedAt)} ago`;
  }
  return machine.status.toLowerCase().replace("_", " ");
}

/** The same text, capitalized, for table cells and headings. */
export function machineStatusLabel(machine: Pick<Machine, "status" | "freshness" | "syncedAt">, now: number): string {
  const text = machineStatusText(machine, now);
  return text.charAt(0).toUpperCase() + text.slice(1);
}
