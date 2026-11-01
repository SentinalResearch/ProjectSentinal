import { describe, expect, it } from "vitest";

import { nextTypedLine } from "./typedLine";

describe("the line being typed (5.A phone completion)", () => {
  const type = (start: string | null, ...keys: string[]) => keys.reduce(nextTypedLine, start);

  it("follows printable keys and Backspace", () => {
    expect(type("", "p", "y")).toBe("py");
    expect(type("py", "\x7f")).toBe("p");
    expect(type("", "\x7f")).toBe("");
    expect(type("", "git status")).toBe("git status");
    expect(type("é", "\x7f")).toBe("");
  });

  it("starts over on Enter, ^C, ^U, and ^L", () => {
    for (const key of ["\r", "\x03", "\x15"]) expect(type("python tr", key), JSON.stringify(key)).toBe("");
    expect(type("ls", "\x0c")).toBe("ls");
  });

  it("stops guessing after keys that move the cursor or recall history", () => {
    for (const key of ["\x1b[A", "\x1b[D", "\t", "\x12", "\x1b"]) expect(type("py", key), JSON.stringify(key)).toBeNull();
    // An unknown line is known again after Enter.
    expect(type(null, "x")).toBeNull();
    expect(type(null, "\r")).toBe("");
  });
});
