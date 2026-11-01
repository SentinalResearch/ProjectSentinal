import type { Machine, Session, SessionEvent } from "../../../data/types";

/** One line of "While you were away" (GLASS_SPEC.md §7). */
export interface FeedItem {
  id: string;
  time: string;
  /** Unix ms when known, for ordering. */
  at?: number;
  /** CSS colour of the state square. */
  square: string;
  headline: string;
  detail: string;
  sessionId?: string;
}

/** Process churn and plain output are not news. */
const QUIET = new Set(["PROCESS_STARTED", "PROCESS_EXITED", "OUTPUT", "COMMAND_UPDATED"]);

const RESOURCE = /^(CPU|GPU|RAM|VRAM|Disk|Cpu|Gpu|Ram|Vram)\b/;

function line(session: Session, machine: Machine | undefined, event: SessionEvent, index: number): FeedItem | null {
  const [time, kind, detail, at] = event;
  if (QUIET.has(kind)) return null;
  const where = machine?.name ?? session.machine;
  const base = { id: `${session.id}:${index}:${kind}`, time, at, sessionId: session.id };
  switch (kind) {
    case "PROGRESS":
    case "PROGRESS_UPDATED":
    case "TRAINING_PROGRESS":
      return { ...base, square: "var(--s-running)", headline: session.name, detail };
    case "COMMAND_STARTED":
      return { ...base, square: "var(--s-idle)", headline: `${session.name} started`, detail };
    case "CHECKPOINT_CREATED":
      return { ...base, square: "var(--s-completed)", headline: "Checkpoint saved", detail };
    case "WAITING_FOR_INPUT":
      return { ...base, square: "var(--s-waiting)", headline: `${session.name} waiting`, detail };
    case "POSSIBLY_STALLED":
    case "STALLED":
      return { ...base, square: "var(--s-stalled)", headline: `${session.name} stalled`, detail: `${where} · ${detail}` };
    case "RESOURCE_HIGH": {
      const resource = RESOURCE.exec(detail)?.[1]?.toUpperCase() ?? "Resource";
      return { ...base, square: "var(--s-waiting)", headline: `${resource} usage high`, detail: `${where} · ${detail}` };
    }
    case "WARNING":
    case "WARNING_DETECTED":
      return { ...base, square: "var(--s-waiting)", headline: `${session.name} warning`, detail };
    case "ERROR":
    case "ERROR_DETECTED":
    case "COMMAND_FAILED":
    case "FAILED":
      return { ...base, square: "var(--s-failed)", headline: `${session.name} failed`, detail };
    case "COMMAND_FINISHED":
      return /^exit 0\b/.test(detail)
        ? { ...base, square: "var(--s-completed)", headline: `${session.name} finished`, detail }
        : { ...base, square: "var(--s-failed)", headline: `${session.name} failed`, detail };
    case "COMMAND_INTERRUPTED":
    case "INTERRUPTED":
      return { ...base, square: "var(--s-interrupted)", headline: `${session.name} interrupted`, detail };
    default:
      return { ...base, square: "var(--line-strong)", headline: session.name, detail: detail || kind.toLowerCase() };
  }
}

/** Sort key: absolute time when known, else today's HH:mm[:ss]. */
function order(item: FeedItem): string {
  return item.at !== undefined ? String(item.at).padStart(16, "0") : `T${item.time}`;
}

/**
 * The newest notable Events across every Session, plus Machines that went
 * offline, newest first and at most `limit`.
 */
export function awayFeed(sessions: Session[], machines: Machine[], limit = 10): FeedItem[] {
  const events = sessions.flatMap((session) => {
    const machine = machines.find((entry) => entry.id === session.machine);
    return (session.events ?? []).map((event, index) => line(session, machine, event, index));
  });
  const withTime = events.filter((item): item is FeedItem => item !== null);
  const timed = withTime.some((item) => item.at !== undefined);
  // Mixed sources: order the timed ones by time, the rest by their clock text.
  const sorted = timed
    ? withTime.sort((left, right) => (right.at ?? 0) - (left.at ?? 0))
    : withTime.sort((left, right) => order(right).localeCompare(order(left)));
  const offline: FeedItem[] = machines
    .filter((machine) => machine.status !== "ONLINE")
    .map((machine) => ({
      id: `machine:${machine.id}`,
      time: "",
      square: "var(--s-disconnected-fg)",
      headline: machine.status === "DISCONNECTED" ? `${machine.name} disconnected` : `${machine.name} went offline`,
      detail: machine.lastSeen ? `last seen ${machine.lastSeen} ago` : "not seen yet",
    }));
  return [...sorted.slice(0, Math.max(0, limit - offline.length)), ...offline];
}
