import { needsAttention } from "../../../data/format";
import type { Session } from "../../../data/types";

/** One live item in the island's ticker. */
export interface TickerItem {
  sessionId: string;
  state: Session["state"];
  text: string;
}

/** GLASS_SPEC.md §4: what the island shows first. */
const PRIORITY: Partial<Record<Session["state"], number>> = { FAILED: 0, WAITING: 1, STALLED: 2, RUNNING: 3 };

const WORD: Partial<Record<Session["state"], string>> = { FAILED: "failed", WAITING: "waiting", STALLED: "stalled" };

const MAX_NAME = 22;

function shortName(name: string): string {
  return name.length > MAX_NAME ? `${name.slice(0, MAX_NAME - 1)}…` : name;
}

function progressText(session: Session): string | null {
  const progress = session.progress;
  if (!progress || progress.total <= 0) return null;
  return `${Math.round((progress.current / progress.total) * 100)} %`;
}

/**
 * Up to `limit` items, FAILED > WAITING > STALLED > RUNNING (only RUNNING
 * Sessions that report progress), newest first within a state.
 */
export function tickerItems(terminals: Session[], limit = 3): TickerItem[] {
  return terminals
    .filter((session) => session.state in PRIORITY && !session.dismissed && (session.state !== "RUNNING" || progressText(session) !== null))
    .sort(
      (left, right) =>
        (PRIORITY[left.state] ?? 9) - (PRIORITY[right.state] ?? 9) || (right.updatedAt ?? 0) - (left.updatedAt ?? 0),
    )
    .slice(0, limit)
    .map((session) => ({
      sessionId: session.id,
      state: session.state,
      text: `${shortName(session.name)} ${session.state === "RUNNING" ? progressText(session) : WORD[session.state]}`,
    }));
}

/** How many Sessions need the owner (the collapsed "N need you" chip). */
export function needCount(terminals: Session[]): number {
  return terminals.filter(needsAttention).length;
}
