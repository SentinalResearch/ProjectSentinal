import { captureCapabilities, captureOf } from "./capture";
import { dataAdapter } from "./index";
import { isBlanketAnswer, riskOf } from "./risk";
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

/**
 * Agents whose menus are chosen with arrows and Enter, where a digit is not a
 * verified answer. The watcher does not report which option the marker is on,
 * so arrows cannot be counted safely either: these are answered in the terminal.
 */
const ARROW_MENU_AGENTS = new Set(["VIBE"]);

function arrowMenu(session?: Session): boolean {
  return session?.codingAgent !== undefined && ARROW_MENU_AGENTS.has(session.codingAgent.agent);
}

/** Send one offered answer. Anything but a choice key is refused here too. */
export function answerChoice(sessionId: string, choice: PromptChoice, session?: Session): Promise<void> {
  if (!choiceKeysAllowed(choice.keys)) return Promise.reject(new Error("not an answer key"));
  if (arrowMenu(session)) return Promise.reject(new Error("answer this menu in the terminal"));
  return dataAdapter.sendInput(sessionId, choice.keys);
}

/** The offered answers worth showing, or none when the program listed none. */
export function offeredChoices(session: Session): PromptChoice[] {
  // A hard-to-undo command never gets a blanket "don't ask again" from Sentinal.
  const risky = riskOf(session.prompt) !== null;
  return (session.promptOptions ?? [])
    .filter((choice) => choiceKeysAllowed(choice.keys))
    .filter((choice) => !(risky && isBlanketAnswer(choice.label)))
    .slice(0, 6);
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
  if (arrowMenu(session)) return false;
  return offeredChoices(session).length > 0 || YES_NO.test(session.prompt);
}

/**
 * Why a WAITING Session cannot be answered from a card, in one line, or null
 * when it can (or is not waiting). Without it the card looks broken.
 */
export function noAnswerReason(session: Session): string | null {
  if (session.state !== "WAITING" || canAnswer(session)) return null;
  if (!captureCapabilities(captureOf(session)).canSendInput) {
    return "Sentinal only watches this one. Answer it in its own terminal.";
  }
  if (arrowMenu(session)) return "This menu needs the arrow keys. Answer it in the terminal.";
  return "Answer it in the terminal.";
}

/** A live Session that accepts Ctrl-C (the daemon still enforces it). */
export function canInterrupt(session: Session): boolean {
  return INTERRUPTIBLE.has(session.state) && captureCapabilities(captureOf(session)).canSendInput;
}

/** How long End waits for Ctrl-C to stop the program before closing the terminal. */
export const END_GRACE_MS = 2_000;

/**
 * End a terminal (design L2): Ctrl-C first so the program can stop cleanly,
 * then, if it is still live, close the shell. Its output is kept.
 */
export async function endSession(
  sessionId: string,
  stillLive: () => boolean,
  wait: (ms: number) => Promise<void> = (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
): Promise<void> {
  await dataAdapter.sendSignal(sessionId, "SIGINT");
  await wait(END_GRACE_MS);
  if (stillLive()) await dataAdapter.sendSignal(sessionId, "KILL");
}
