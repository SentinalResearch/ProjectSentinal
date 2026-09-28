import type { CaptureLevel } from "../protocol/CaptureLevel";
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

/** One-line explanation of what is unavailable, for reduced capture. */
export function captureExplanation(capture: CaptureLevel): string | null {
  switch (capture) {
    case "FULL":
      return null;
    case "COMMANDS":
      return "COMMANDS capture: Sentinal records this shell's Commands and Metrics. There is no terminal output, input, resize, or kill.";
    case "PROCESS_LOG":
      return "PROCESS_LOG capture: Sentinal reads this process's log and Metrics only. There is no terminal, input, resize, or kill.";
    case "PROCESS":
      return "PROCESS capture: Sentinal observes this process's Metrics and lifecycle only. There is no output, input, resize, or kill.";
  }
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

/** "Log detected" hint while a known log is not being tailed yet (before 1b.D). */
export function logHint(session: Session): string | null {
  if (!session.log || captureOf(session) === "PROCESS_LOG") return null;
  return `Log detected: ${session.log.path} (${LOG_SOURCES[session.log.source]}) — tailing not enabled yet.`;
}
