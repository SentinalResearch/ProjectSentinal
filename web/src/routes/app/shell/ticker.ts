import { canAnswer, offeredChoices } from "../../../data/actions";
import { agentPercent, reporterName } from "../../../data/agentState";
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

/** A running agent's reported steps as a percent (6.5), while it says it is working. */
function reportedPercent(session: Session): number | undefined {
  return session.state === "RUNNING" && session.agentState?.status === "WORKING" ? agentPercent(session.agentState) : undefined;
}

function progressText(session: Session): string | null {
  const progress = session.progress;
  if (!progress || progress.total <= 0) {
    const reported = reportedPercent(session);
    return reported === undefined ? null : `${reported} %`;
  }
  return `${Math.round((progress.current / progress.total) * 100)} %`;
}

/**
 * Up to `limit` items, FAILED > WAITING > STALLED > RUNNING (only RUNNING
 * Sessions that report progress), newest first within a state.
 */
export function tickerItems(terminals: Session[], limit = 3): TickerItem[] {
  // 6.4: a running agent that reports itself blocked is shown like a question
  // (display only: it is not a need and sends no notification).
  const blocked = (session: Session) => session.state === "RUNNING" && session.agentState?.status === "BLOCKED";
  const rank = (session: Session) => (blocked(session) ? 1.5 : (PRIORITY[session.state] ?? 9));
  return terminals
    .filter(
      (session) =>
        session.state in PRIORITY && !session.dismissed && (session.state !== "RUNNING" || blocked(session) || progressText(session) !== null),
    )
    .sort((left, right) => rank(left) - rank(right) || (right.updatedAt ?? 0) - (left.updatedAt ?? 0))
    .slice(0, limit)
    .map((session) => ({
      sessionId: session.id,
      state: blocked(session) ? "WAITING" : session.state,
      text: `${shortName(session.name)} ${blocked(session) ? "blocked" : session.state === "RUNNING" ? progressText(session) : WORD[session.state]}`,
    }));
}

/** What the island shows in its middle, by priority. */
export type IslandFocus =
  | { kind: "answer"; session: Session }
  | { kind: "ticker" }
  | {
      kind: "progress";
      session: Session;
      percent: number;
      /** Set when the percent is the agent's own step count (6.5), not measured. */
      reported?: { who: string; label: string };
    }
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
  const measured = terminals
    .filter((session) => session.state === "RUNNING" && session.progress && session.progress.total > 0)
    .map((session) => ({ session, percent: Math.round((session.progress!.current / session.progress!.total) * 100) }));
  const reported = terminals
    .filter((session) => !session.dismissed && !(session.progress && session.progress.total > 0))
    .flatMap((session) => {
      const percent = reportedPercent(session);
      const state = session.agentState;
      return percent === undefined || !state
        ? []
        : [
            {
              session,
              percent,
              reported: { who: reporterName(session.codingAgent), label: `step ${state.step} of ${state.steps}` },
            },
          ];
    });
  const top = [...measured, ...reported].sort((left, right) => right.percent - left.percent)[0];
  return top ? { kind: "progress", ...top } : { kind: "idle" };
}

/** How many Sessions need the owner (the collapsed "N need you" chip). */
export function needCount(terminals: Session[]): number {
  return terminals.filter(needsAttention).length;
}
