import type { CaptureLevel } from "../protocol/CaptureLevel";
import type { SessionOrigin } from "../protocol/SessionOrigin";
import type { Session } from "./types";

/**
 * What an interface may offer for a Session, derived from capture alone.
 * Mirrors `CaptureLevel::capabilities` in crates/protocol; the daemon enforces it.
 */
export interface CaptureCapabilities {
  canAttach: boolean;
  canSendInput: boolean;
  canResize: boolean;
  canKill: boolean;
  exposesCommands: boolean;
  exposesTerminalOutput: boolean;
  exposesLogOutput: boolean;
  exposesMetrics: boolean;
}

const CAPABILITIES: Record<CaptureLevel, CaptureCapabilities> = {
  FULL: {
    canAttach: true,
    canSendInput: true,
    canResize: true,
    canKill: true,
    exposesCommands: true,
    exposesTerminalOutput: true,
    exposesLogOutput: false,
    exposesMetrics: true,
  },
  COMMANDS: {
    canAttach: false,
    canSendInput: false,
    canResize: false,
    canKill: false,
    exposesCommands: true,
    exposesTerminalOutput: false,
    exposesLogOutput: false,
    exposesMetrics: true,
  },
  PROCESS_LOG: {
    canAttach: false,
    canSendInput: false,
    canResize: false,
    canKill: false,
    exposesCommands: false,
    exposesTerminalOutput: false,
    exposesLogOutput: true,
    exposesMetrics: true,
  },
  PROCESS: {
    canAttach: false,
    canSendInput: false,
    canResize: false,
    canKill: false,
    exposesCommands: false,
    exposesTerminalOutput: false,
    exposesLogOutput: false,
    exposesMetrics: true,
  },
};

export function captureOf(session: Session): CaptureLevel {
  return session.capture ?? "FULL";
}

export function captureCapabilities(capture: CaptureLevel): CaptureCapabilities {
  return CAPABILITIES[capture];
}

const EXPLANATIONS: Record<CaptureLevel, string> = {
  FULL: "Terminal and Commands captured.",
  COMMANDS: "Commands and Metrics only — no terminal.",
  PROCESS_LOG: "Read-only log and Metrics — no terminal input.",
  PROCESS: "Metrics only.",
};

/** One concise line saying what Sentinal captures, and what it cannot do. */
export function captureExplanation(capture: CaptureLevel): string {
  return EXPLANATIONS[capture];
}

const ORIGINS: Record<SessionOrigin, string> = {
  UI: "UI",
  WATCH: "WATCH",
  SHELL_HOOK: "SHELL HOOK",
  AUTO_WRAP: "AUTO-WRAP",
  ADOPTED: "ADOPTED",
};

/** Display name of how a Session entered Sentinal; absent means UI. */
export function originLabel(origin: SessionOrigin | undefined): string {
  return ORIGINS[origin ?? "UI"];
}

/** An ended Session whose exit status Sentinal could not know (external processes). */
export function exitUnknown(session: Session): boolean {
  return session.state === "COMPLETED" && captureOf(session) !== "FULL" && session.exitCode == null;
}

const LOG_SOURCES = {
  EXPLICIT: "named with --log",
  STDOUT: "stdout",
  STDERR: "stderr",
  STDOUT_AND_STDERR: "stdout and stderr",
} as const;

/** Whether a known log is being read, or was found but cannot be read. */
export function logHint(session: Session): string | null {
  if (!session.log) return null;
  const where = `${session.log.path} (${LOG_SOURCES[session.log.source]})`;
  return captureOf(session) === "PROCESS_LOG"
    ? `Watching ${where}, read-only.`
    : `Log found at ${where}, but Sentinal cannot read it.`;
}

/** POSIX single-quote `value` so a shell reads it as one literal word. */
export function shellQuote(value: string): string {
  return `'${value.replace(/'/g, "'\\''")}'`;
}

// eslint-disable-next-line no-control-regex
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

/**
 * A copyable `sentinal watch` line for next time, built only from the
 * Session's cwd and title — never from Commands or output — and withheld
 * when that metadata contains control characters.
 */
export function watchHint(session: Session): string | null {
  if (captureOf(session) === "FULL" || !session.cwd) return null;
  if (CONTROL.test(session.cwd) || CONTROL.test(session.name)) return null;
  const cwd = session.cwd.startsWith("~/") ? `~/${shellQuote(session.cwd.slice(2))}` : shellQuote(session.cwd);
  return `cd ${cwd} && sentinal watch -t ${shellQuote(session.name)}`;
}
