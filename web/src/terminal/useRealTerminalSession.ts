import type { Terminal } from "@xterm/xterm";
import { useCallback, useEffect, useRef, useState } from "react";

import { dataAdapter } from "../data";
import { SESSION_TOKEN_KEY } from "../data/api";
import { decodeTerminalFrame, encodeTerminalFrame } from "../frame";
import type { ClientRequest } from "../protocol/ClientRequest";
import type { DaemonMessage } from "../protocol/DaemonMessage";
import type { Envelope } from "../protocol/Envelope";

const PROTOCOL_VERSION = 1;
const encoder = new TextEncoder();
// QUESTION: each TerminalPane opens its own WebSocket to the daemon, so Split
// View (multiple panes) opens N connections instead of sharing one — acceptable
// for a first pass, flagged in FUTURE.md if pooling is wanted later.
/**
 * Real (non-mock) terminal attach: WebSocket-connects to the daemon, attaches to
 * one session, and streams bytes in/out of the given xterm `Terminal`. Extracted
 * from the old src/App.tsx (Milestone 1 dashboard) so a single-session screen can
 * reuse the same working WS/frame logic without the whole-dashboard UI around it.
 */
export interface RealTerminalSession {
  attached: boolean;
  /** Send raw bytes to the attached session (used by key-bar Esc/Tab/↑/↓ presses). */
  sendBytes: (bytes: Uint8Array) => void;
}

export function useRealTerminalSession(sessionId: string, terminal: Terminal | null): RealTerminalSession {
  const [attached, setAttached] = useState(false);
  const socketRef = useRef<WebSocket | null>(null);
  let route: ReturnType<typeof dataAdapter.resolveTerminal> | null = null;
  try {
    route = dataAdapter.resolveTerminal(sessionId);
  } catch {
    // The live-data bootstrap has not registered the owning Machine yet.
  }
  const numericId = route?.localSessionId;
  const websocketUrl = route?.websocketUrl;

  const sendBytes = useCallback(
    (bytes: Uint8Array) => {
      const socket = socketRef.current;
      if (numericId !== undefined && socket?.readyState === WebSocket.OPEN) {
        socket.send(encodeTerminalFrame(numericId, bytes));
      }
    },
    [numericId],
  );

  useEffect(() => {
    if (!terminal || numericId === undefined || websocketUrl === undefined) return;

    const endpoint = new URL(websocketUrl);
    const token = localStorage.getItem(SESSION_TOKEN_KEY) ?? "";
    endpoint.searchParams.set("token", token);
    const socket = new WebSocket(endpoint);
    socket.binaryType = "arraybuffer";
    socketRef.current = socket;

    function sendRequest(request: ClientRequest) {
      if (socket.readyState !== WebSocket.OPEN) return;
      const envelope: Envelope<ClientRequest> = { version: PROTOCOL_VERSION, payload: request };
      socket.send(JSON.stringify(envelope));
    }

    socket.onopen = () => {
      sendRequest({ type: "ATTACH", session_id: numericId });
      setAttached(true);
    };
    socket.onclose = () => setAttached(false);
    socket.onerror = () => setAttached(false);
    socket.onmessage = async (event: MessageEvent<string | ArrayBuffer | Blob>) => {
      if (typeof event.data === "string") {
        try {
          const envelope = JSON.parse(event.data) as Envelope<DaemonMessage>;
          if (envelope.payload.type === "ERROR") {
            terminal.write(`\r\n\x1b[38;2;255;59;47m[error] ${envelope.payload.message}\x1b[0m\r\n`);
          }
        } catch {
          // ignore malformed control messages
        }
        return;
      }
      const data = event.data instanceof Blob ? await event.data.arrayBuffer() : event.data;
      const frame = decodeTerminalFrame(new Uint8Array(data));
      if (frame.sessionId === numericId) terminal.write(frame.payload);
    };

    const dataDisposable = terminal.onData((data) => sendBytes(encoder.encode(data)));
    const resizeDisposable = terminal.onResize(({ cols, rows }) =>
      sendRequest({ type: "RESIZE", session_id: numericId, cols, rows }),
    );

    return () => {
      dataDisposable.dispose();
      resizeDisposable.dispose();
      sendRequest({ type: "DETACH", session_id: numericId });
      socket.close();
    };
  }, [terminal, numericId, websocketUrl]);

  return { attached, sendBytes };
}
