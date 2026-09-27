import { describe, expect, it } from "vitest";

import { decodeTerminalFrame, encodeTerminalFrame } from "./frame";

describe("terminal frame codec", () => {
  it("round trips a session id and terminal bytes", () => {
    const payload = new Uint8Array([0, 27, 91, 65, 255]);
    const decoded = decodeTerminalFrame(encodeTerminalFrame(42, payload));

    expect(decoded.sessionId).toBe(42);
    expect(decoded.payload).toEqual(payload);
  });

  it("rejects a frame shorter than its eight-byte header", () => {
    expect(() => decodeTerminalFrame(new Uint8Array(7))).toThrow(
      "terminal frame is 7 bytes; expected at least 8",
    );
  });
});
