import type { Terminal } from "@xterm/xterm";
import { publishChat } from "../data/agentChat";
import { useCallback, useEffect, useRef, useState } from "react";

import { dataAdapter } from "../data";
import { machineId } from "../data/api/machine-connections";
import { createEchoTimer, health } from "../data/health";
import { decodeTerminalFrame, encodeTerminalFrame } from "../frame";
import type { ClientRequest } from "../protocol/ClientRequest";
import type { DaemonMessage } from "../protocol/DaemonMessage";
import type { Envelope } from "../protocol/Envelope";
import type { SessionSize } from "./sessionSize";

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
  /**
   * The Session's size from a daemon that owns it (T3), or null from an
   * older one, whose terminal still sizes the PTY itself.
   */
  sessionSize: SessionSize | null;
  /** Report this viewer's box (at the base font) and whether it is in use. */
  report: (cols: number, rows: number, active: boolean) => void;
}

export interface RealTerminalOptions {
  /**
   * Earlier parts of this terminal are already drawn above: keep them, and let
   * the snapshot's history scroll them up instead of resetting the terminal.
   */
  keepContent?: boolean;
  /** This viewer's box in cells at the base font (T3); the terminal's size when absent. */
  boxSize?: () => { cols: number; rows: number } | null;
}

export function useRealTerminalSession(
  sessionId: string,
  terminal: Terminal | null,
  options: RealTerminalOptions = {},
): RealTerminalSession {
  const keepContent = options.keepContent ?? false;
  const boxSizeRef = useRef(options.boxSize);
  boxSizeRef.current = options.boxSize;
  const [attached, setAttached] = useState(false);
  const [sessionSize, setSessionSize] = useState<SessionSize | null>(null);
  const socketRef = useRef<WebSocket | null>(null);
  const sizedRef = useRef(false);
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

  const report = useCallback(
    (cols: number, rows: number, active: boolean) => {
      const socket = socketRef.current;
      if (!sizedRef.current || numericId === undefined || socket?.readyState !== WebSocket.OPEN) return;
      const envelope: Envelope<ClientRequest> = {
        version: PROTOCOL_VERSION,
        payload: { type: "VIEWER_STATE", session_id: numericId, cols, rows, active },
      };
      socket.send(JSON.stringify(envelope));
    },
    [numericId],
  );

  useEffect(() => {
    if (!terminal || numericId === undefined || websocketUrl === undefined) return;
    sizedRef.current = false;
    setSessionSize(null);

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

    // T3: the daemon owns the size. ATTACH says this viewer's box; a daemon
    // that owns the size answers SESSION_SIZE before the screen, and this
    // terminal draws at that size. An older daemon (a worker behind the
    // control may be one) ignores the box: then this terminal sizes the PTY
    // with RESIZE once the snapshot arrives, as before.
    const box = () => boxSizeRef.current?.() ?? { cols: terminal.cols, rows: terminal.rows };
    const resizeToTerminal = () =>
      sendRequest({ type: "RESIZE", session_id: numericId, cols: terminal.cols, rows: terminal.rows });
    const focused = () => document.activeElement === terminal.textarea;
    let snapshotReceived = false;

    socket.onopen = () => {
      const { cols, rows } = box();
      // History: the daemon sends the scrollback before the screen, so a
      // viewer can scroll back through what it missed.
      sendRequest({ type: "ATTACH", session_id: numericId, history: true, cols, rows, mode: "INTERACTIVE" });
      setAttached(true);
    };
    socket.onclose = () => setAttached(false);
    socket.onerror = () => setAttached(false);
    socket.onmessage = async (event: MessageEvent<string | ArrayBuffer | Blob>) => {
      if (typeof event.data === "string") {
        try {
          const envelope = JSON.parse(event.data) as Envelope<DaemonMessage>;
          // AC2: the conversation travels on the attach, gated like output.
          if (envelope.payload.type === "CHAT" && envelope.payload.session_id === numericId) {
            publishChat({
              sessionId,
              linked: envelope.payload.linked,
              fromStart: envelope.payload.from_start,
              entries: envelope.payload.entries,
            });
          }
          if (envelope.payload.type === "SESSION_SIZE" && envelope.payload.session_id === numericId) {
            const { cols, rows, owner } = envelope.payload;
            sizedRef.current = true;
            // A reset and a whole screen at this size follow at once.
            if (terminal.cols !== cols || terminal.rows !== rows) terminal.resize(cols, rows);
            setSessionSize({ cols, rows, owner });
          }
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
        // The first frame redraws the whole screen; start from a clean one
        // unless earlier parts are drawn above it.
        snapshotReceived = true;
        health.record("TERMINAL_OPEN_MS", machine, performance.now() - openedAt);
        if (!keepContent) terminal.reset();
        terminal.write(frame.payload);
        // The attachment acknowledgement: VIEWER_STATE to a daemon that owns
        // the size (attaching does not take it from others), else RESIZE.
        if (sizedRef.current) {
          const { cols, rows } = box();
          sendRequest({ type: "VIEWER_STATE", session_id: numericId, cols, rows, active: focused() });
        } else {
          resizeToTerminal();
        }
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
    // An older daemon: this terminal's size is the PTY's. With a daemon that
    // owns the size, the terminal follows SESSION_SIZE instead.
    const resizeDisposable = terminal.onResize(({ cols, rows }) => {
      if (!sizedRef.current) sendRequest({ type: "RESIZE", session_id: numericId, cols, rows });
    });
    // Focus means "I'm using this one" (owner decision 2): it takes the size.
    const textarea = terminal.textarea;
    const onFocus = () => {
      if (!sizedRef.current) return;
      const { cols, rows } = box();
      sendRequest({ type: "VIEWER_STATE", session_id: numericId, cols, rows, active: true });
    };
    textarea?.addEventListener("focus", onFocus);

    return () => {
      dataDisposable.dispose();
      resizeDisposable.dispose();
      textarea?.removeEventListener("focus", onFocus);
      sendRequest({ type: "DETACH", session_id: numericId });
      socket.close();
    };
  }, [terminal, numericId, websocketUrl, sessionId, keepContent]);

  return { attached, sendBytes, sessionSize, report };
}
