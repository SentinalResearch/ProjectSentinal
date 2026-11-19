import { describe, expect, it } from "vitest";

import type { Session } from "../data/types";
import { resumeTag } from "./resumeTags";

function part(fields: Partial<Session>): Session {
  return { id: "pop:1", machine: "pop", name: "Session 1", state: "INTERRUPTED", ...fields } as Session;
}

// 18 Nov 2026, 19:42 local time.
const RESUMED_AT = new Date(2026, 10, 18, 19, 42).getTime();

describe("resumeTag", () => {
  it("says when the terminal resumed, how the part before ended, and which part this is", () => {
    const tag = resumeTag(part({ state: "INTERRUPTED", exitCode: null }), part({ createdAt: RESUMED_AT }), 6, 7);
    expect(tag).toEqual({
      label: "Resumed",
      when: "18 Nov 2026 · 19:42",
      before: "before: interrupted · exit ?",
      tone: "interrupted",
      part: "part 7 / 7",
      ariaLabel: "Resumed 18 Nov 2026 · 19:42. Before: interrupted, exit unknown. Part 7 of 7.",
    });
  });

  it("marks a clean exit green and a failure red", () => {
    expect(resumeTag(part({ state: "COMPLETED", exitCode: 0 }), part({}), 1, 2)).toMatchObject({
      before: "before: exit 0",
      tone: "ok",
    });
    expect(resumeTag(part({ state: "FAILED", exitCode: 2 }), part({}), 1, 2)).toMatchObject({
      before: "before: failed · exit 2",
      tone: "failed",
    });
    expect(resumeTag(part({ state: "COMPLETED", exitCode: 130 }), part({}), 1, 2)).toMatchObject({
      before: "before: exit 130",
      tone: "failed",
    });
  });

  it("never invents a time it does not have", () => {
    expect(resumeTag(part({}), part({ createdAt: undefined }), 1, 2).when).toBe("time unknown");
  });
});
