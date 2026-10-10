import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Session } from "../data/types";
import { MAX_PEEKS, usePeekLines } from "./peek";

vi.mock("../data", () => ({
  isMockData: false,
  dataAdapter: {
    resolveTerminal: (id: string) => ({ websocketUrl: `ws://control.test/${id}`, localSessionId: 1 }),
  },
}));

let sockets: { url: string; closed: boolean }[] = [];

class StubWebSocket {
  readyState = 0;
  binaryType = "blob";
  onopen = null;
  onmessage = null;
  onclose = null;
  onerror = null;
  record: { url: string; closed: boolean };
  constructor(url: string) {
    this.record = { url, closed: false };
    sockets.push(this.record);
  }
  send() {}
  close() {
    this.record.closed = true;
  }
}

function session(id: string): Session {
  return { id, capture: "FULL", events: [] } as unknown as Session;
}

const open = () => sockets.filter((socket) => !socket.closed);

describe("usePeekLines", () => {
  beforeEach(() => {
    sockets = [];
    vi.stubGlobal("WebSocket", StubWebSocket);
  });
  afterEach(() => vi.unstubAllGlobals());

  it("peeks only while the card is on screen", () => {
    const view = renderHook(({ visible }) => usePeekLines(session("mac:1"), visible), { initialProps: { visible: false } });
    expect(sockets).toHaveLength(0);
    view.rerender({ visible: true });
    expect(open()).toHaveLength(1);
    view.rerender({ visible: false });
    expect(open()).toHaveLength(0);
    view.unmount();
  });

  it(`never holds more than ${MAX_PEEKS} peeks, and frees a slot when one ends`, () => {
    const views = Array.from({ length: MAX_PEEKS + 2 }, (_, index) => renderHook(() => usePeekLines(session(`mac:${index}`), true)));
    expect(open()).toHaveLength(MAX_PEEKS);
    views[0].unmount();
    expect(open()).toHaveLength(MAX_PEEKS - 1);
    views.slice(1).forEach((view) => view.unmount());
    expect(open()).toHaveLength(0);
  });
});
