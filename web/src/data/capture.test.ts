import { describe, expect, it } from "vitest";

import {
  captureCapabilities,
  captureExplanation,
  exitUnknown,
  logHint,
  originLabel,
  shellQuote,
  watchHint,
} from "./capture";
import type { Session } from "./types";

const base: Session = { id: "1", machine: "m", name: "job", state: "RUNNING" };

describe("capture capabilities", () => {
  it("allow terminal control only for FULL capture", () => {
    expect(captureCapabilities("FULL")).toMatchObject({ canAttach: true, canSendInput: true, canKill: true });
    for (const capture of ["COMMANDS", "PROCESS_LOG", "PROCESS"] as const) {
      const caps = captureCapabilities(capture);
      expect([caps.canAttach, caps.canSendInput, caps.canResize, caps.canKill]).toEqual([false, false, false, false]);
      expect(caps.exposesMetrics).toBe(true);
    }
    expect(captureCapabilities("COMMANDS").exposesCommands).toBe(true);
    expect(captureCapabilities("PROCESS").exposesCommands).toBe(false);
  });

  it("explain every capture level in one concise line", () => {
    expect(captureExplanation("FULL")).toBe("Terminal and Commands captured.");
    expect(captureExplanation("COMMANDS")).toBe("Commands and Metrics only — no terminal.");
    expect(captureExplanation("PROCESS_LOG")).toBe("Read-only log and Metrics — no terminal input.");
    expect(captureExplanation("PROCESS")).toBe("Metrics only.");
  });

  it("names every origin, treating an absent origin as UI", () => {
    expect(originLabel(undefined)).toBe("UI");
    expect(originLabel("SHELL_HOOK")).toBe("SHELL HOOK");
    expect(originLabel("AUTO_WRAP")).toBe("AUTO-WRAP");
    expect(originLabel("ADOPTED")).toBe("ADOPTED");
  });

  it("marks only ended external Sessions without an exit code as unknown", () => {
    expect(exitUnknown({ ...base, state: "COMPLETED", capture: "PROCESS", exitCode: null })).toBe(true);
    expect(exitUnknown({ ...base, state: "COMPLETED", capture: "FULL", exitCode: null })).toBe(false);
    expect(exitUnknown({ ...base, state: "RUNNING", capture: "PROCESS", exitCode: null })).toBe(false);
  });

  it("describes the log as watched or unreadable, never as a future feature", () => {
    const log = { path: "/w/train.log", source: "STDOUT" as const };
    expect(logHint({ ...base, capture: "PROCESS_LOG", log })).toBe("Watching /w/train.log (stdout), read-only.");
    expect(logHint({ ...base, capture: "PROCESS", log })).toBe(
      "Log found at /w/train.log (stdout), but Sentinal cannot read it.",
    );
    expect(logHint({ ...base, capture: "PROCESS" })).toBeNull();
  });
});

describe("Watch fully next time", () => {
  it("builds a quoted command from cwd and title only", () => {
    const session = { ...base, name: "Qwen's run", cwd: "/work/my project", capture: "PROCESS" as const };
    expect(watchHint(session)).toBe("cd '/work/my project' && sentinal watch -t 'Qwen'\\''s run'");
  });

  it("keeps a leading ~ expandable", () => {
    expect(watchHint({ ...base, cwd: "~/Projects/TIM", capture: "PROCESS_LOG" })).toBe(
      "cd ~/'Projects/TIM' && sentinal watch -t 'job'",
    );
  });

  it("is never offered for FULL Sessions or without a cwd", () => {
    expect(watchHint({ ...base, cwd: "/w", capture: "FULL" })).toBeNull();
    expect(watchHint({ ...base, capture: "PROCESS" })).toBeNull();
  });

  it("is withheld when metadata contains control characters", () => {
    expect(watchHint({ ...base, cwd: "/w\nrm -rf ~", capture: "PROCESS" })).toBeNull();
    expect(watchHint({ ...base, name: "x\u001b]0;evil\u0007", cwd: "/w", capture: "PROCESS" })).toBeNull();
  });

  it("quotes single quotes so they cannot end the string", () => {
    expect(shellQuote("a'b")).toBe("'a'\\''b'");
    expect(shellQuote("$(id)")).toBe("'$(id)'");
  });
});
