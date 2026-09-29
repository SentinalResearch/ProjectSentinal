import type { SessionState } from "../protocol/SessionState";

const READ_ONLY_STATES = new Set<SessionState>(["COMPLETED", "FAILED", "INTERRUPTED"]);

/** Whether a Session has no live PTY and must only display retained output. */
export function isReadOnlyTerminalState(state: SessionState): boolean {
  return READ_ONLY_STATES.has(state);
}
