import { canAnswer, offeredChoices } from "../../../data/actions";
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

/** What the island shows in its middle, by priority. */
export type IslandFocus =
  | { kind: "answer"; session: Session }
  | { kind: "ticker" }
  | { kind: "progress"; session: Session; percent: number }
  | { kind: "idle" };

/**
 * A yes/no question you can answer from the island beats the ticker (it is
 * the one thing you can finish in place); then anything that needs you; then
 * the running job with progress that is closest to done; else nothing.
 */
export function islandFocus(terminals: Session[], skipAnswer: ReadonlySet<string> = new Set()): IslandFocus {
  const asking = terminals.find(
    (session) =>
      !session.dismissed &&
      !skipAnswer.has(session.id) &&
      canAnswer(session) &&
      offeredChoices(session).length === 0,
  );
  if (asking) return { kind: "answer", session: asking };
  if (needCount(terminals) > 0) return { kind: "ticker" };
  const withPercent = terminals
    .filter((session) => session.state === "RUNNING" && session.progress && session.progress.total > 0)
    .map((session) => ({ session, percent: Math.round((session.progress!.current / session.progress!.total) * 100) }))
    .sort((left, right) => right.percent - left.percent);
  const top = withPercent[0];
  return top ? { kind: "progress", session: top.session, percent: top.percent } : { kind: "idle" };
}

/** How many Sessions need the owner (the collapsed "N need you" chip). */
export function needCount(terminals: Session[]): number {
  return terminals.filter(needsAttention).length;
}
