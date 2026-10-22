import { renderHook } from "@testing-library/react";
import type { Terminal } from "@xterm/xterm";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { encodeTerminalFrame } from "../frame";
import { useRealTerminalSession } from "./useRealTerminalSession";

vi.mock("../data", () => ({
  isMockData: false,
  dataAdapter: {
    resolveTerminal: (id: string) => ({ websocketUrl: `ws://control.test/${id}`, localSessionId: 7 }),
  },
}));

type Sent = { type: string; cols?: number; rows?: number } | "binary";

class StubWebSocket {
  static OPEN = 1;
  static last: StubWebSocket | null = null;
  readyState = 1;
  binaryType = "blob";
  sent: Sent[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: unknown }) => Promise<void>) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  constructor() {
    StubWebSocket.last = this;
  }
  send(data: unknown) {
    this.sent.push(typeof data === "string" ? JSON.parse(data).payload : "binary");
  }
  close() {}
}

function fakeTerminal() {
  const log: string[] = [];
  const terminal = {
    cols: 132,
    rows: 40,
    reset: () => log.push("reset"),
    write: (data: Uint8Array | string) => log.push(typeof data === "string" ? data : new TextDecoder().decode(data)),
    onData: () => ({ dispose() {} }),
    onResize: () => ({ dispose() {} }),
  };
  return { terminal: terminal as unknown as Terminal, log };
}

const frame = (text: string) => ({ data: encodeTerminalFrame(7, new TextEncoder().encode(text)).buffer });

describe("useRealTerminalSession", () => {
  beforeEach(() => vi.stubGlobal("WebSocket", StubWebSocket));
  afterEach(() => vi.unstubAllGlobals());

  it("sizes the PTY to this terminal before the snapshot and acknowledges it with this size", async () => {
    const { terminal, log } = fakeTerminal();
    renderHook(() => useRealTerminalSession("pop:7", terminal));
    const socket = StubWebSocket.last!;
    socket.onopen!();
    expect(socket.sent).toEqual([
      { type: "RESIZE", session_id: 7, cols: 132, rows: 40 },
      { type: "ATTACH", session_id: 7 },
    ]);

    await socket.onmessage!(frame("snapshot"));
    expect(log).toEqual(["reset", "snapshot"]);
    expect(socket.sent.at(-1)).toEqual({ type: "RESIZE", session_id: 7, cols: 132, rows: 40 });

    await socket.onmessage!(frame("live"));
    expect(log).toEqual(["reset", "snapshot", "live"]);
    expect(socket.sent).toHaveLength(3);
  });
});
