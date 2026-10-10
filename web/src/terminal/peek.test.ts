import { describe, expect, it } from "vitest";

import { encodeTerminalFrame } from "../frame";
import { lastNonEmptyLines, PeekClient, type PeekSocket } from "./peek";

class FakeSocket implements PeekSocket {
  readyState = 0;
  binaryType = "blob";
  sent: string[] = [];
  closed = false;
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string | ArrayBuffer | Blob }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  send(data: string | ArrayBufferLike) {
    if (typeof data !== "string") throw new Error("a peek must never send terminal bytes");
    this.sent.push(data);
  }
  close() {
    this.closed = true;
  }
}

describe("peek attach", () => {
  it("sends only ATTACH and DETACH — never input or RESIZE — and passes on its own Session's bytes", () => {
    const socket = new FakeSocket();
    const received: string[] = [];
    const client = new PeekClient("ws://control.test/api/v1/machines/rtx/ws", 7, (bytes) => received.push(new TextDecoder().decode(bytes)), () => socket);
    client.start();
    socket.readyState = 1;
    socket.onopen?.();
    socket.onmessage?.({ data: encodeTerminalFrame(7, new TextEncoder().encode("epoch 68/100\r\n")).buffer as ArrayBuffer });
    socket.onmessage?.({ data: encodeTerminalFrame(8, new TextEncoder().encode("other session")).buffer as ArrayBuffer });
    client.stop();

    const types = socket.sent.map((text) => JSON.parse(text).payload.type);
    expect(types).toEqual(["ATTACH", "DETACH"]);
    expect(received).toEqual(["epoch 68/100\r\n"]);
    expect(socket.closed).toBe(true);
  });

  it("keeps the last two non-empty lines", () => {
    expect(lastNonEmptyLines(["a", "", "epoch 67/100", "epoch 68/100", "  ", ""], 2)).toEqual(["epoch 67/100", "epoch 68/100"]);
    expect(lastNonEmptyLines(["", ""], 2)).toEqual([]);
  });
});
