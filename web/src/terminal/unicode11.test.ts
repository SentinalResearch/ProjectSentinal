import { Unicode11Addon } from "@xterm/addon-unicode11";
import { Terminal } from "@xterm/xterm";
import { describe, expect, it } from "vitest";

import { activateUnicode11 } from "./trailing";

function cursorAfter(text: string, unicode11: boolean): Promise<number> {
  const term = new Terminal({ cols: 40, rows: 4, allowProposedApi: true });
  if (unicode11) activateUnicode11(term, new Unicode11Addon());
  return new Promise((resolve) => {
    term.write(text, () => {
      resolve(term.buffer.active.cursorX);
      term.dispose();
    });
  });
}

describe("Unicode 11 widths (T1.1)", () => {
  it("gives emoji and CJK the two cells a program assumes", async () => {
    // A program that printed "✅ ok" expects the "o" in column 3.
    expect(await cursorAfter("✅", true)).toBe(2);
    expect(await cursorAfter("漢字", true)).toBe(4);
    expect(await cursorAfter("🧪 test", true)).toBe(7);
  });

  it("differs from the old Unicode 6 widths for newer emoji", async () => {
    expect(await cursorAfter("🧪", false)).not.toBe(await cursorAfter("🧪", true));
  });
});
