import type { Session } from "./types";

// QUESTION: fixtures only carry a project name, never an id, and the prototype's own
// ids ("tim", "maa") don't obviously derive from the names either. See QUESTIONS.md #9 —
// reuse this function anywhere a project id is needed so they stay consistent.
/** Stable id for a project name, used for `/app/projects/:id` links (sidebar + Overview). */
export function slugifyProject(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/(^-|-$)/g, "");
}

/** The single-line "what's happening" fact shown next to a session (row / needs-you card). */
/** Loss as from → to: the watcher's training summary, else the mock's metrics. */
export function lossOf(session: Session): { from?: number; to: number } | undefined {
  const loss = session.training?.loss;
  if (loss !== undefined) return { from: session.training?.firstLoss, to: loss };
  return session.metrics?.loss;
}

/** Compact learning rate, e.g. 0.0002 → "2e-4". */
export function formatLearningRate(rate: number): string {
  return rate >= 0.01 ? String(rate) : rate.toExponential();
}

export function formatSessionFact(session: Session): string {
  const parts: string[] = [];
  if (session.progress) {
    parts.push(`${session.progress.unit} ${session.progress.current}/${session.progress.total}`);
  }
  const loss = lossOf(session);
  if (loss) parts.push(`loss ${loss.to}`);
  if (session.result) parts.push(`${session.result.failed} failed · ${session.result.passed} passed`);
  if (session.prompt) parts.push(session.prompt);
  if (session.details) parts.push(session.details);
  return parts.length > 0 ? parts.join(" · ") : "—";
}

export function formatSessionDuration(session: Session): string {
  return session.runningFor ?? session.result?.duration ?? "—";
}

/** A session that belongs in Overview's "Needs you" grid and the sidebar's Overview badge count. */
export function needsAttention(session: Session): boolean {
  return session.state === "WAITING" || session.state === "FAILED" || session.state === "STALLED";
}

export type SessionKind = "training" | "failed" | "waiting" | "stalled" | "server" | "idle";

/**
 * Session Detail's top block is kind-specific (training/failed/waiting/stalled/server/idle)
 * per SentinalSession.dc.html, but the data model has no explicit `kind` field — derive it
 * from which fields are present, same signal the design's own demo data was keyed by.
 */
export function sessionKind(session: Session): SessionKind {
  if (session.state === "FAILED" && session.result) return "failed";
  if (session.state === "WAITING" && session.prompt) return "waiting";
  if (session.state === "STALLED") return "stalled";
  if (session.progress && (session.metrics || session.training)) return "training";
  if (extractPort(session) !== null) return "server";
  return "idle";
}

/** Port a session's command is serving on, if any (`vllm serve ... --port 8000` → "8000"). */
export function extractPort(session: Session): string | null {
  const source = `${session.command ?? ""} ${session.details ?? ""}`;
  const match = /--port[ =](\d+)/.exec(source) ?? /:(\d{2,5})\b/.exec(source) ?? /port (\d{2,5})\b/i.exec(source);
  return match ? match[1] : null;
}

/** Color-square + label-color pair for an event row, keyed by event kind (README: info/warning/error/routine). */
export interface EventColor {
  square: string;
  text: string;
}

const EVENT_COLORS: Record<string, EventColor> = {
  COMMAND_STARTED: { square: "var(--text-1)", text: "var(--text-1)" },
  INPUT_SENT: { square: "var(--text-1)", text: "var(--text-1)" },
  PROGRESS: { square: "var(--line-strong)", text: "var(--text-2)" },
  PROGRESS_UPDATED: { square: "var(--line-strong)", text: "var(--text-2)" },
  CHECKPOINT_CREATED: { square: "var(--accent-fg)", text: "var(--accent-fg)" },
  RESOURCE_HIGH: { square: "var(--s-waiting)", text: "var(--s-waiting-fg)" },
  WARNING: { square: "var(--s-waiting)", text: "var(--s-waiting-fg)" },
  WARNING_DETECTED: { square: "var(--s-waiting)", text: "var(--s-waiting-fg)" },
  INPUT_REQUESTED: { square: "var(--s-waiting)", text: "var(--s-waiting-fg)" },
  STALL_SUSPECTED: { square: "var(--s-waiting)", text: "var(--s-waiting-fg)" },
  POSSIBLY_STALLED: { square: "var(--s-waiting)", text: "var(--s-waiting-fg)" },
  COMMAND_FAILED: { square: "var(--s-failed)", text: "var(--s-failed-fg)" },
  ERROR: { square: "var(--s-failed)", text: "var(--s-failed-fg)" },
  ERROR_DETECTED: { square: "var(--s-failed)", text: "var(--s-failed-fg)" },
  COMMAND_INTERRUPTED: { square: "var(--s-interrupted)", text: "var(--s-interrupted-fg)" },
  INTERRUPTED: { square: "var(--s-interrupted)", text: "var(--s-interrupted-fg)" },
};

const DEFAULT_EVENT_COLOR: EventColor = { square: "var(--line-strong)", text: "var(--text-2)" };

/** See SentinalSession.dc.html's `evColor` map — routine events fall back to the neutral pair. */
export function eventColor(kind: string): EventColor {
  return EVENT_COLORS[kind] ?? DEFAULT_EVENT_COLOR;
}

/** `HEALTHY`/`WAITING`/etc. → the `--s-*` token slug used for the health card's fill and accents. */
export function healthColorSlug(overall: string): string {
  const upper = overall.toUpperCase();
  if (upper === "HEALTHY") return "running";
  if (upper === "WAITING") return "waiting";
  if (upper === "WARNING") return "waiting";
  if (upper === "FAILED") return "failed";
  if (upper.includes("STALL")) return "stalled";
  if (upper === "INTERRUPTED") return "interrupted";
  return "idle";
}

/** The most recent CHECKPOINT_CREATED event's path, if any — used in the interrupted notice and Ctrl-C consequence. */
export function lastCheckpointPath(session: Session): string | null {
  const events = session.events ?? [];
  for (let i = events.length - 1; i >= 0; i -= 1) {
    const [, kind, detail] = events[i];
    if (kind === "CHECKPOINT_CREATED") return detail;
  }
  return null;
}

/**
 * The Ctrl-C confirm dialog's consequence sentence — kind-aware, mirroring
 * SentinalSession.dc.html's per-session `cons` copy without hardcoding its
 * literal demo strings. Shared by Session Detail and Split View.
 */
export function confirmConsequence(session: Session, machineName: string): string {
  const kind = sessionKind(session);
  if (kind === "training" && session.progress) {
    const checkpoint = lastCheckpointPath(session);
    const base = `Stops ${session.command ?? session.name} on ${machineName}${
      session.runningFor ? ` after ${session.runningFor}` : ""
    } at ${session.progress.unit} ${session.progress.current}/${session.progress.total}.`;
    return checkpoint ? `${base} Last checkpoint ${checkpoint}.` : base;
  }
  if (kind === "server") {
    const port = extractPort(session);
    return `Stops the server${port ? ` on :${port}` : ""} on ${machineName}. Requests in flight will fail.`;
  }
  if (kind === "waiting") {
    return `Cancels ${session.command ?? session.name} on ${machineName} at the prompt.`;
  }
  if (kind === "stalled") {
    const [duration] = (session.details ?? "").split(", ");
    return `Stops ${session.name} on ${machineName}.${duration ? ` It has produced ${duration}.` : ""}`;
  }
  return `Sends SIGINT to ${session.name} on ${machineName}.`;
}
