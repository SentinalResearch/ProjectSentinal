import { describe, expect, it } from "vitest";

import type { SessionState } from "../protocol/SessionState";
import { isReadOnlyTerminalState } from "./lifecycle";

describe("terminal lifecycle", () => {
  it("makes every terminal Session state read-only", () => {
    for (const state of ["COMPLETED", "FAILED", "INTERRUPTED"] satisfies SessionState[]) {
      expect(isReadOnlyTerminalState(state)).toBe(true);
    }
  });

  it("keeps every live or reconnectable state interactive", () => {
    for (const state of [
      "CREATED",
      "IDLE",
      "RUNNING",
      "WAITING",
      "STALLED",
      "DISCONNECTED",
    ] satisfies SessionState[]) {
      expect(isReadOnlyTerminalState(state)).toBe(false);
    }
  });
});
