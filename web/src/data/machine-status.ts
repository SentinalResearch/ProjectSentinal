import { FINISHED_STATES } from "./format";
import type { Machine, Session } from "./types";

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

/** "last seen 5 min ago" / "3 h ago" / "2 days ago"; the exact time belongs in a tooltip. */
export function lastSeenText(at: number, now: number): string {
  const seconds = Math.max(0, Math.floor((now - at) / 1_000));
  if (seconds < 60) return "last seen just now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `last seen ${minutes} min ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return `last seen ${hours} h ago`;
  return `last seen ${Math.floor(hours / 24)} days ago`;
}

/**
 * What each Session's state can honestly be said to be. A Session that had not
 * finished on a Machine that is not online has an unknown state, so it shows
 * DISCONNECTED (X24) rather than the last state its Machine reported. Only the
 * view changes: the Machine's next report shows the real state again. A Session
 * whose Machine is not listed yet is left as it is.
 */
export function withMachineReach(sessions: Session[], machines: Pick<Machine, "id" | "status">[]): Session[] {
  const unreachable = new Set(machines.filter((machine) => machine.status !== "ONLINE").map((machine) => machine.id));
  if (unreachable.size === 0) return sessions;
  return sessions.map((session) =>
    unreachable.has(session.machine) && !FINISHED_STATES.has(session.state) && session.state !== "DISCONNECTED"
      ? { ...session, state: "DISCONNECTED", prompt: undefined, promptOptions: undefined }
      : session,
  );
}
