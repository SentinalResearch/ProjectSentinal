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

type Sent = { type: string; cols?: number; rows?: number; history?: boolean; mode?: string; active?: boolean } | "binary";

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
    resize: (cols: number, rows: number) => {
      terminal.cols = cols;
      terminal.rows = rows;
      log.push(`resize ${cols}x${rows}`);
    },
    textarea: undefined,
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

  it("tells an older daemon its size with RESIZE once the snapshot arrives", async () => {
    const { terminal, log } = fakeTerminal();
    renderHook(() => useRealTerminalSession("pop:7", terminal));
    const socket = StubWebSocket.last!;
    socket.onopen!();
    expect(socket.sent).toEqual([
      { type: "ATTACH", session_id: 7, history: true, cols: 132, rows: 40, mode: "INTERACTIVE" },
    ]);

    await socket.onmessage!(frame("snapshot"));
    expect(log).toEqual(["reset", "snapshot"]);
    expect(socket.sent.at(-1)).toEqual({ type: "RESIZE", session_id: 7, cols: 132, rows: 40 });

    await socket.onmessage!(frame("live"));
    expect(log).toEqual(["reset", "snapshot", "live"]);
    expect(socket.sent).toHaveLength(2);
  });

  it("draws at the size a daemon that owns it says, and never resizes the PTY itself (T3)", async () => {
    const { terminal, log } = fakeTerminal();
    const { result } = renderHook(() =>
      useRealTerminalSession("pop:7", terminal, { boxSize: () => ({ cols: 100, rows: 30 }) }),
    );
    const socket = StubWebSocket.last!;
    socket.onopen!();
    expect(socket.sent[0]).toEqual({ type: "ATTACH", session_id: 7, history: true, cols: 100, rows: 30, mode: "INTERACTIVE" });

    await socket.onmessage!({
      data: JSON.stringify({ version: 1, payload: { type: "SESSION_SIZE", session_id: 7, cols: 132, rows: 40, owner: false } }),
    });
    await socket.onmessage!(frame("snapshot"));
    expect(log).toEqual(["reset", "snapshot"]);
    // Attaching never takes the size from another viewer.
    expect(socket.sent.at(-1)).toEqual({ type: "VIEWER_STATE", session_id: 7, cols: 100, rows: 30, active: false });
    expect(socket.sent.some((sent) => sent !== "binary" && sent.type === "RESIZE")).toBe(false);

    await socket.onmessage!({
      data: JSON.stringify({ version: 1, payload: { type: "SESSION_SIZE", session_id: 7, cols: 100, rows: 30, owner: true } }),
    });
    expect(log.at(-1)).toBe("resize 100x30");
    await vi.waitFor(() => expect(result.current.sessionSize).toEqual({ cols: 100, rows: 30, owner: true }));
  });

  it("keeps earlier parts already drawn instead of resetting the terminal", async () => {
    const { terminal, log } = fakeTerminal();
    renderHook(() => useRealTerminalSession("pop:7", terminal, { keepContent: true }));
    const socket = StubWebSocket.last!;
    socket.onopen!();
    await socket.onmessage!(frame("history and screen"));
    expect(log).toEqual(["history and screen"]);
  });
});
