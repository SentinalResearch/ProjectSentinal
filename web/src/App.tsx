import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useCallback, useEffect, useRef, useState } from "react";

import { decodeTerminalFrame, encodeTerminalFrame } from "./frame";
import { KeyBar } from "./keybar";
import type { ClientRequest } from "./protocol/ClientRequest";
import type { DaemonMessage } from "./protocol/DaemonMessage";
import type { Envelope } from "./protocol/Envelope";
import type { Session } from "./protocol/Session";

const PROTOCOL_VERSION = 1;
const DEFAULT_URL =
  import.meta.env.VITE_DAEMON_URL ??
  `${window.location.protocol === "https:" ? "wss" : "ws"}://${window.location.hostname}:7420/api/v1/ws`;
const encoder = new TextEncoder();

function upsertSession(sessions: Session[], next: Session): Session[] {
  const existing = sessions.findIndex((session) => session.id === next.id);
  if (existing < 0) return [...sessions, next];
  return sessions.map((session) => (session.id === next.id ? next : session));
}

export default function App() {
  const [url, setUrl] = useState(DEFAULT_URL);
  const [token, setToken] = useState(() => localStorage.getItem("ta-token") ?? "");
  const [rememberToken, setRememberToken] = useState(
    () => localStorage.getItem("ta-token") !== null,
  );
  const [status, setStatus] = useState("Disconnected");
  const [error, setError] = useState<string | null>(null);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<number | null>(null);
  const socketRef = useRef<WebSocket | null>(null);
  const activeSessionRef = useRef<number | null>(null);
  const terminalHostRef = useRef<HTMLDivElement | null>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);

  const sendRequest = useCallback((request: ClientRequest) => {
    const socket = socketRef.current;
    if (socket?.readyState !== WebSocket.OPEN) {
      setError("Connect to the daemon first");
      return;
    }
    const envelope: Envelope<ClientRequest> = {
      version: PROTOCOL_VERSION,
      payload: request,
    };
    socket.send(JSON.stringify(envelope));
  }, []);

  const sendTerminalBytes = useCallback((bytes: Uint8Array) => {
    const socket = socketRef.current;
    const sessionId = activeSessionRef.current;
    if (socket?.readyState === WebSocket.OPEN && sessionId !== null) {
      socket.send(encodeTerminalFrame(sessionId, bytes));
    }
  }, []);

  useEffect(() => {
    const host = terminalHostRef.current;
    if (host === null) return;

    const terminal = new Terminal({
      cursorBlink: true,
      convertEol: false,
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
      theme: { background: "#0b0f14", foreground: "#e5edf5" },
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(host);
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => webgl.dispose());
      terminal.loadAddon(webgl);
    } catch (error) {
      console.warn("WebGL terminal renderer is unavailable", error);
    }
    fit.fit();
    const input = terminal.onData((data) => sendTerminalBytes(encoder.encode(data)));
    terminalRef.current = terminal;
    fitRef.current = fit;

    const resize = () => {
      fit.fit();
      const sessionId = activeSessionRef.current;
      if (sessionId !== null) {
        sendRequest({
          type: "RESIZE",
          session_id: sessionId,
          cols: terminal.cols,
          rows: terminal.rows,
        });
      }
    };
    window.addEventListener("resize", resize);
    return () => {
      window.removeEventListener("resize", resize);
      input.dispose();
      terminal.dispose();
      terminalRef.current = null;
      fitRef.current = null;
    };
  }, [sendRequest, sendTerminalBytes]);

  const connect = () => {
    socketRef.current?.close();
    setError(null);
    setStatus("Connecting…");
    if (rememberToken) localStorage.setItem("ta-token", token);
    else localStorage.removeItem("ta-token");

    let endpoint: URL;
    try {
      endpoint = new URL(url);
    } catch {
      setStatus("Disconnected");
      setError(`Invalid WebSocket URL: ${url}`);
      return;
    }
    endpoint.searchParams.set("token", token);
    const socket = new WebSocket(endpoint);
    socket.binaryType = "arraybuffer";
    socketRef.current = socket;
    socket.onopen = () => {
      setStatus("Connected");
      sendRequest({ type: "LIST_SESSIONS" });
    };
    socket.onclose = () => {
      if (socketRef.current === socket) {
        setStatus("Disconnected");
        socketRef.current = null;
      }
    };
    socket.onerror = () => setError("WebSocket connection failed");
    socket.onmessage = async (event: MessageEvent<string | ArrayBuffer | Blob>) => {
      if (typeof event.data === "string") {
        try {
          const envelope = JSON.parse(event.data) as Envelope<DaemonMessage>;
          if (envelope.payload.type === "SESSIONS") {
            setSessions(envelope.payload.sessions);
          } else if (envelope.payload.type === "SESSION_UPDATED") {
            const updated = envelope.payload.session;
            setSessions((current) => upsertSession(current, updated));
          } else if (envelope.payload.type === "ERROR") {
            setError(envelope.payload.message);
          }
        } catch {
          setError("Daemon sent invalid JSON");
        }
        return;
      }
      const data = event.data instanceof Blob ? await event.data.arrayBuffer() : event.data;
      try {
        const frame = decodeTerminalFrame(new Uint8Array(data));
        if (frame.sessionId === activeSessionRef.current) {
          terminalRef.current?.write(frame.payload);
        }
      } catch (frameError) {
        setError(frameError instanceof Error ? frameError.message : "Invalid terminal frame");
      }
    };
  };

  const attach = (session: Session) => {
    const previous = activeSessionRef.current;
    if (previous !== null && previous !== session.id) {
      sendRequest({ type: "DETACH", session_id: previous });
    }
    activeSessionRef.current = session.id;
    setActiveSessionId(session.id);
    terminalRef.current?.clear();
    sendRequest({ type: "ATTACH", session_id: session.id });
    requestAnimationFrame(() => fitRef.current?.fit());
  };

  const newSession = () => {
    const terminal = terminalRef.current;
    sendRequest({
      type: "CREATE_SESSION",
      shell: null,
      cwd: null,
      cols: terminal?.cols ?? 80,
      rows: terminal?.rows ?? 24,
    });
  };

  return (
    <main>
      <header>
        <div>
          <p className="eyebrow">terminal-agent</p>
          <h1>Persistent sessions</h1>
        </div>
        <span className={`status ${status === "Connected" ? "online" : ""}`}>{status}</span>
      </header>

      <section className="connection" aria-label="Daemon connection">
        <label>
          WebSocket URL
          <input value={url} onChange={(event) => setUrl(event.target.value)} />
        </label>
        <label>
          Token
          <input
            aria-label="Token"
            type="password"
            value={token}
            onChange={(event) => setToken(event.target.value)}
          />
        </label>
        <label className="remember">
          <input
            type="checkbox"
            checked={rememberToken}
            onChange={(event) => setRememberToken(event.target.checked)}
          />
          Remember on this device
        </label>
        <button type="button" onClick={connect}>Connect</button>
      </section>

      {error !== null && <p role="alert" className="error">{error}</p>}

      <div className="workspace">
        <aside>
          <div className="session-heading">
            <h2>Sessions</h2>
            <button type="button" onClick={newSession}>New</button>
          </div>
          {sessions.length === 0 ? (
            <p className="empty">No sessions yet.</p>
          ) : (
            <ul>
              {sessions.map((session) => (
                <li key={session.id}>
                  <button
                    type="button"
                    className={activeSessionId === session.id ? "active" : ""}
                    onClick={() => attach(session)}
                  >
                    <strong>{session.title ?? `Session ${session.id}`}</strong>
                    <span>{session.state} · {session.cwd}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </aside>
        <section className="terminal-panel" aria-label="Terminal">
          <div ref={terminalHostRef} className="terminal" data-testid="terminal" />
          <KeyBar onBytes={sendTerminalBytes} />
        </section>
      </div>
    </main>
  );
}
