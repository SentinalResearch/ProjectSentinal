import { describe, expect, it } from "vitest";

import { afterKey, encodeChar, encodeSpecial, tapModifier } from "./keys";

describe("terminal keyboard codes (MOBILE_GLASS_SPEC §4)", () => {
  it("sends Ctrl+letter as its control byte, for any case", () => {
    expect(encodeChar("c", { ctrl: true, alt: false })).toBe("\x03");
    expect(encodeChar("C", { ctrl: true, alt: false })).toBe("\x03");
    expect(encodeChar("d", { ctrl: true, alt: false })).toBe("\x04");
    expect(encodeChar("l", { ctrl: true, alt: false })).toBe("\x0c");
    expect(encodeChar("r", { ctrl: true, alt: false })).toBe("\x12");
    expect(encodeChar("[", { ctrl: true, alt: false })).toBe("\x1b");
  });

  it("prefixes Esc for Alt, after Ctrl", () => {
    expect(encodeChar("b", { ctrl: false, alt: true })).toBe("\x1bb");
    expect(encodeChar("c", { ctrl: true, alt: true })).toBe("\x1b\x03");
    expect(encodeSpecial("left", { ctrl: false, alt: true })).toBe("\x1b\x1b[D");
  });

  it("names the special keys with the standard sequences", () => {
    const none = { ctrl: false, alt: false };
    expect(encodeSpecial("up", none)).toBe("\x1b[A");
    expect(encodeSpecial("down", none)).toBe("\x1b[B");
    expect(encodeSpecial("right", none)).toBe("\x1b[C");
    expect(encodeSpecial("left", none)).toBe("\x1b[D");
    expect(encodeSpecial("esc", none)).toBe("\x1b");
    expect(encodeSpecial("tab", none)).toBe("\t");
    expect(encodeSpecial("enter", none)).toBe("\r");
    expect(encodeSpecial("backspace", none)).toBe("\x7f");
    expect(encodeChar("x", none)).toBe("x");
  });
});

describe("modifier latch and lock", () => {
  it("tap latches for one key; a key uses the latch up", () => {
    const latched = tapModifier("off", 0, 1_000);
    expect(latched).toBe("latched");
    expect(afterKey(latched)).toBe("off");
  });

  it("double-tap locks until tapped again; keys do not release a lock", () => {
    const locked = tapModifier("latched", 1_000, 1_200);
    expect(locked).toBe("locked");
    expect(afterKey(locked)).toBe("locked");
    expect(tapModifier(locked, 1_200, 5_000)).toBe("off");
  });

  it("a slow second tap turns a latch off", () => {
    expect(tapModifier("latched", 1_000, 2_000)).toBe("off");
  });
});
