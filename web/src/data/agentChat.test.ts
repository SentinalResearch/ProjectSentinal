import { describe, expect, it } from "vitest";

import type { ChatEntry } from "../protocol/ChatEntry";
import { chatRows, inlineMarkdown } from "./agentChat";

const at = 1_795_083_900_000;
const entries: ChatEntry[] = [
  { at, item: { kind: "USER", text: "Create a.txt" } },
  { at, item: { kind: "TOOL", id: "s1", tool: "ToolSearch", summary: "select:TaskCreate" } },
  { at, item: { kind: "TOOL_RESULT", id: "s1", ok: true, preview: "loaded" } },
  { at, item: { kind: "TOOL", id: "b1", tool: "Bash", summary: "touch a.txt" } },
  { at, item: { kind: "TOOL", id: "b2", tool: "Bash", summary: "ls" } },
  { at, item: { kind: "TOOL_RESULT", id: "b1", ok: false, preview: "denied" } },
  { at, item: { kind: "AGENT", text: "Done." } },
  { at, item: { kind: "TURN_FINISHED", tool_uses: 2 } },
];

describe("chatRows (AC2)", () => {
  it("pairs each tool call with its result and leaves plan bookkeeping out", () => {
    const rows = chatRows(entries);
    expect(rows.map((row) => row.kind)).toEqual(["user", "tool", "tool", "agent", "turn"]);
    expect(rows[1]).toMatchObject({ summary: "touch a.txt", result: { ok: false, preview: "denied" } });
    expect(rows[2]).toMatchObject({ summary: "ls" });
    expect(rows[2]).not.toHaveProperty("result");
  });
});

describe("inlineMarkdown", () => {
  it("reads code and bold, and leaves everything else as text", () => {
    expect(inlineMarkdown("Run `cargo test` **now**, then <b>x</b>")).toEqual([
      { kind: "text", text: "Run " },
      { kind: "code", text: "cargo test" },
      { kind: "text", text: " " },
      { kind: "bold", text: "now" },
      { kind: "text", text: ", then <b>x</b>" },
    ]);
  });
});
