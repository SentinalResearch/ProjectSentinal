import { captureCapabilities, captureOf } from "./capture";
import { dataAdapter } from "./index";
import type { PromptChoice, Session } from "./types";

/** States whose program can still take Ctrl-C. */
export const INTERRUPTIBLE: ReadonlySet<string> = new Set(["RUNNING", "WAITING", "STALLED"]);

/**
 * Answer a waiting prompt: the key and Enter. A line-reading `[y/N]` prompt
 * ignores a bare "y" until Enter arrives.
 */
export function answerPrompt(sessionId: string, key: "y" | "n"): Promise<void> {
  return dataAdapter.sendInput(sessionId, `${key}\r`);
}

/**
 * Keys an offered answer may send: one letter or digit (optionally with
 * Enter), Enter, or Escape. The same rule as the daemon's
 * `prompt_keys_allowed`; the labels came from terminal output.
 */
export function choiceKeysAllowed(keys: string): boolean {
  return /^(?:\r|\u001b|[0-9A-Za-z]\r?)$/.test(keys);
}

/** Send one offered answer. Anything but a choice key is refused here too. */
export function answerChoice(sessionId: string, choice: PromptChoice): Promise<void> {
  if (!choiceKeysAllowed(choice.keys)) return Promise.reject(new Error("not an answer key"));
  return dataAdapter.sendInput(sessionId, choice.keys);
}

/** The offered answers worth showing, or none when the program listed none. */
export function offeredChoices(session: Session): PromptChoice[] {
  return (session.promptOptions ?? []).filter((choice) => choiceKeysAllowed(choice.keys)).slice(0, 6);
}

/** A WAITING Session with a prompt whose capture lets the owner type into it. */
export function canAnswer(session: Session): boolean {
  return session.state === "WAITING" && Boolean(session.prompt) && captureCapabilities(captureOf(session)).canSendInput;
}

/** A live Session that accepts Ctrl-C (the daemon still enforces it). */
export function canInterrupt(session: Session): boolean {
  return INTERRUPTIBLE.has(session.state) && captureCapabilities(captureOf(session)).canSendInput;
}
