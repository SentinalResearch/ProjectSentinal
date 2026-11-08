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

/** `[y/N]`, `(Y/n)`, `[yes/no]`: a key and Enter answers it. A bare `(yes/no)` wants the word typed. */
const YES_NO = /\[\s*y(es)?\s*\/\s*n(o)?\s*\]|\(\s*y\s*\/\s*n\s*\)/i;

/**
 * A WAITING Session whose answers the UI can offer: the program's own list, or
 * y and n for a yes-or-no question. Never y or n for a password or free text.
 */
export function canAnswer(session: Session): boolean {
  if (session.state !== "WAITING" || !session.prompt) return false;
  if (!captureCapabilities(captureOf(session)).canSendInput) return false;
  return offeredChoices(session).length > 0 || YES_NO.test(session.prompt);
}

/** A live Session that accepts Ctrl-C (the daemon still enforces it). */
export function canInterrupt(session: Session): boolean {
  return INTERRUPTIBLE.has(session.state) && captureCapabilities(captureOf(session)).canSendInput;
}
