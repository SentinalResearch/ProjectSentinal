import { Terminal } from "@xterm/xterm";
import { useEffect, useState } from "react";

import { dataAdapter, isMockData } from "../data";
import type { Session } from "../data/types";
import { decodeTerminalFrame } from "../frame";
import type { ClientRequest } from "../protocol/ClientRequest";
import type { Envelope } from "../protocol/Envelope";

const PROTOCOL_VERSION = 1;

/** At most this many live peeks at once (approved 4.F rule: on-screen cards only). */
export const MAX_PEEKS = 4;
const PEEK_COLS = 120;
const PEEK_ROWS = 24;

export interface PeekSocket {
  readyState: number;
  binaryType: string;
  onopen: (() => void) | null;
  onmessage: ((event: { data: string | ArrayBuffer | Blob }) => void) | null;
  onclose: (() => void) | null;
  onerror: (() => void) | null;
  send(data: string): void;
  close(): void;
}

/**
 * A read-only attach used only to show a terminal card's latest lines. It sends
 * ATTACH and DETACH and nothing else: never input and never RESIZE, so the real
 * terminal's size and contents are untouched.
 */
export class PeekClient {
  private socket: PeekSocket | null = null;

  constructor(
    private readonly url: string,
    private readonly sessionId: number,
    private readonly onBytes: (bytes: Uint8Array) => void,
    private readonly openSocket: (url: string) => PeekSocket = (url) => new WebSocket(url) as unknown as PeekSocket,
  ) {}

  start(): void {
    const socket = this.openSocket(this.url);
    socket.binaryType = "arraybuffer";
    this.socket = socket;
    socket.onopen = () => this.request({ type: "ATTACH", session_id: this.sessionId });
    socket.onmessage = (event) => {
      if (typeof event.data === "string" || !(event.data instanceof ArrayBuffer)) return;
      const frame = decodeTerminalFrame(new Uint8Array(event.data));
      if (frame.sessionId === this.sessionId) this.onBytes(frame.payload);
    };
  }

  stop(): void {
    if (!this.socket) return;
    this.request({ type: "DETACH", session_id: this.sessionId });
    this.socket.close();
    this.socket = null;
  }

  private request(request: ClientRequest): void {
    if (!this.socket || this.socket.readyState !== 1) return;
    const envelope: Envelope<ClientRequest> = { version: PROTOCOL_VERSION, payload: request };
    this.socket.send(JSON.stringify(envelope));
  }
}

export function lastNonEmptyLines(lines: string[], count: number): string[] {
  return lines.filter((line) => line.trim().length > 0).slice(-count);
}

let activePeeks = 0;

/**
 * The last two lines of a FULL Session's screen while `visible`, from a peek
 * attach rendered by an off-screen xterm (no DOM, no dependency). Mock data
 * and reduced capture use the latest Events instead.
 */
export function usePeekLines(session: Session, visible: boolean): string[] {
  const [lines, setLines] = useState<string[]>([]);
  const fallback = lastNonEmptyLines((session.events ?? []).map(([, kind, detail]) => `${kind} ${detail}`), 2);
  const canPeek = !isMockData && (session.capture ?? "FULL") === "FULL";

  useEffect(() => {
    if (!canPeek || !visible || activePeeks >= MAX_PEEKS) return;
    let route;
    try {
      route = dataAdapter.resolveTerminal(session.id);
    } catch {
      return;
    }
    activePeeks += 1;
    const screen = new Terminal({ cols: PEEK_COLS, rows: PEEK_ROWS, allowProposedApi: false, scrollback: 0 });
    const read = () => {
      const buffer = screen.buffer.active;
      const all: string[] = [];
      for (let row = 0; row < buffer.length; row += 1) all.push(buffer.getLine(row)?.translateToString(true) ?? "");
      setLines(lastNonEmptyLines(all, 2));
    };
    const client = new PeekClient(route.websocketUrl, route.localSessionId, (bytes) => screen.write(bytes, read));
    client.start();
    return () => {
      client.stop();
      screen.dispose();
      activePeeks -= 1;
    };
  }, [canPeek, session.id, visible]);

  return canPeek && lines.length > 0 ? lines : fallback;
}
