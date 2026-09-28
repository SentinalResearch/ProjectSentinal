import { describe, expect, it } from "vitest";

import { captureCapabilities, exitUnknown, logHint } from "./capture";
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

  it("marks only ended external Sessions without an exit code as unknown", () => {
    expect(exitUnknown({ ...base, state: "COMPLETED", capture: "PROCESS", exitCode: null })).toBe(true);
    expect(exitUnknown({ ...base, state: "COMPLETED", capture: "FULL", exitCode: null })).toBe(false);
    expect(exitUnknown({ ...base, state: "RUNNING", capture: "PROCESS", exitCode: null })).toBe(false);
  });

  it("hints at a detected log only until it is tailed", () => {
    const log = { path: "/w/train.log", source: "STDOUT" as const };
    expect(logHint({ ...base, capture: "PROCESS", log })).toBe(
      "Log detected: /w/train.log (stdout) — tailing not enabled yet.",
    );
    expect(logHint({ ...base, capture: "PROCESS_LOG", log })).toBeNull();
    expect(logHint({ ...base, capture: "PROCESS" })).toBeNull();
  });
});
