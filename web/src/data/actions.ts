import { captureCapabilities, captureOf } from "./capture";
import { dataAdapter } from "./index";
import type { Session } from "./types";

/** States whose program can still take Ctrl-C. */
export const INTERRUPTIBLE: ReadonlySet<string> = new Set(["RUNNING", "WAITING", "STALLED"]);

/**
 * Answer a waiting prompt: the key and Enter. A line-reading `[y/N]` prompt
 * ignores a bare "y" until Enter arrives.
 */
export function answerPrompt(sessionId: string, key: "y" | "n"): Promise<void> {
  return dataAdapter.sendInput(sessionId, `${key}\r`);
}

/** A WAITING Session with a prompt whose capture lets the owner type into it. */
export function canAnswer(session: Session): boolean {
  return session.state === "WAITING" && Boolean(session.prompt) && captureCapabilities(captureOf(session)).canSendInput;
}

/** A live Session that accepts Ctrl-C (the daemon still enforces it). */
export function canInterrupt(session: Session): boolean {
  return INTERRUPTIBLE.has(session.state) && captureCapabilities(captureOf(session)).canSendInput;
}
