import type { Terminal } from "@xterm/xterm";
import { useCallback, useEffect, useRef, useState } from "react";

import { dataAdapter } from "../data";
import { machineId } from "../data/api/machine-connections";
import { createEchoTimer, health } from "../data/health";
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

    // Measured for Settings → Diagnostics (4.H): durations only, never the bytes.
    const machine = machineId(sessionId);
    const openedAt = performance.now();
    const echo = createEchoTimer();
    const socket = new WebSocket(websocketUrl);
    socket.binaryType = "arraybuffer";
    socketRef.current = socket;

    function sendRequest(request: ClientRequest) {
      if (socket.readyState !== WebSocket.OPEN) return;
      const envelope: Envelope<ClientRequest> = { version: PROTOCOL_VERSION, payload: request };
      socket.send(JSON.stringify(envelope));
    }

    // The shell lays out its output for the PTY's size, so the PTY must match
    // this terminal: size it before the snapshot is taken, and send RESIZE
    // again once the snapshot arrives (that RESIZE is also the attachment
    // acknowledgement, as in the CLI).
    const resizeToTerminal = () =>
      sendRequest({ type: "RESIZE", session_id: numericId, cols: terminal.cols, rows: terminal.rows });
    let snapshotReceived = false;

    socket.onopen = () => {
      resizeToTerminal();
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
            health.count("ERRORS", machine, "ERROR_ATTACH");
            terminal.write(`\r\n\x1b[38;2;255;59;47m[error] ${envelope.payload.message}\x1b[0m\r\n`);
          }
        } catch {
          // ignore malformed control messages
        }
        return;
      }
      const data = event.data instanceof Blob ? await event.data.arrayBuffer() : event.data;
      const frame = decodeTerminalFrame(new Uint8Array(data));
      if (frame.sessionId !== numericId) return;
      const echoed = echo.output();
      if (echoed !== undefined) health.record("TYPING_ECHO_MS", machine, echoed);
      if (!snapshotReceived) {
        // The first frame redraws the whole screen; start from a clean one.
        snapshotReceived = true;
        health.record("TERMINAL_OPEN_MS", machine, performance.now() - openedAt);
        terminal.reset();
        terminal.write(frame.payload);
        resizeToTerminal();
        return;
      }
      // The daemon resets and redraws a viewer that fell behind (ESC c).
      if (frame.payload[0] === 0x1b && frame.payload[1] === 0x63) health.count("VIEWER_RESYNCS", machine);
      terminal.write(frame.payload);
    };

    const dataDisposable = terminal.onData((data) => {
      echo.keystroke(data);
      sendBytes(encoder.encode(data));
    });
    const resizeDisposable = terminal.onResize(({ cols, rows }) =>
      sendRequest({ type: "RESIZE", session_id: numericId, cols, rows }),
    );

    return () => {
      dataDisposable.dispose();
      resizeDisposable.dispose();
      sendRequest({ type: "DETACH", session_id: numericId });
      socket.close();
    };
  }, [terminal, numericId, websocketUrl, sessionId]);

  return { attached, sendBytes };
}
