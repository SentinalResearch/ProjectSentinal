import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useEffect, useRef, useState } from "react";

import { dataAdapter, isMockData } from "../data";
import type { Session, SessionEvent } from "../data/types";
import { useRealTerminalSession } from "./useRealTerminalSession";
import { isReadOnlyTerminalState } from "./lifecycle";
import styles from "./TerminalPane.module.css";

const TERM_BG = "#0C0C0C";
const TERM_FG = "#D4D4D0";

// Resolved hex for the event kinds a scrollback line might carry — xterm needs literal
// colors (24-bit ANSI escapes), not CSS custom properties. Mirrors data/format.ts's
// eventColor() categories; kept separate because that one returns var(...) strings.
const LINE_HEX: Record<string, string> = {
  COMMAND_STARTED: "#F2F2F0",
  INPUT_SENT: "#F2F2F0",
  WARNING: "#FFD000",
  RESOURCE_HIGH: "#FFD000",
  INPUT_REQUESTED: "#FFD000",
  STALL_SUSPECTED: "#FFD000",
  COMMAND_FAILED: "#FF3B2F",
  ERROR: "#FF3B2F",
  COMMAND_INTERRUPTED: "#FF5CA8",
  INTERRUPTED: "#FF5CA8",
};

function hexToAnsi(hex: string): string {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  return `\x1b[38;2;${r};${g};${b}m`;
}

function writeEventLine(terminal: Terminal, event: SessionEvent) {
  const [time, kind, detail] = event;
  const color = LINE_HEX[kind] ?? TERM_FG;
  terminal.write(`\x1b[38;2;138;138;134m[${time}]\x1b[0m ${hexToAnsi(color)}${kind}\x1b[0m  ${detail}\r\n`);
}

/** Matches SentinalSession.dc.html's `promptStr = (s.cwd || '~') + ' $ '`. */
function promptString(session: Session): string {
  return `${session.cwd ?? "~"} $ `;
}

export interface TerminalPaneProps {
  sessionId: string;
  session: Session;
  /** A pixel height, or `"fill"` to stretch to the parent's available space (Split View panes — the parent must establish a definite height via flex). */
  height?: number | "fill";
  showKeyBar?: boolean;
  onAttachedChange?: (attached: boolean) => void;
  /** The key bar's Ctrl-C key always opens a confirm dialog — it never sends the byte directly (rule 5). */
  onRequestInterrupt?: () => void;
  onDimensionsChange?: (cols: number, rows: number) => void;
  className?: string;
}

const KEYS: [label: string, bytes: number[]][] = [
  ["Esc", [27]],
  ["Tab", [9]],
  ["↑", [27, 91, 65]],
  ["↓", [27, 91, 66]],
];

/**
 * Real xterm.js terminal used by every terminal area (Session detail, Split view).
 * In mock mode there's no live PTY to attach to, so it replays the session's events
 * as scrollback and turns typed lines into `dataAdapter.sendInput` calls — still a
 * real xterm instance, not a fake styled `<div>` (see QUESTIONS.md for the full
 * architecture note). In real/API mode it attaches over the daemon's WebSocket via
 * `useRealTerminalSession`.
 */
export function TerminalPane({
  sessionId,
  session,
  height = 440,
  showKeyBar = true,
  onAttachedChange,
  onRequestInterrupt,
  onDimensionsChange,
  className,
}: TerminalPaneProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const inputLineRef = useRef("");
  const [terminal, setTerminal] = useState<Terminal | null>(null);
  const seenEventCount = useRef(0);
  const [replayWarning, setReplayWarning] = useState<string | null>(null);
  const [replayError, setReplayError] = useState<string | null>(null);
  const readOnly = isReadOnlyTerminalState(session.state);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const term = new Terminal({
      cursorBlink: !readOnly,
      disableStdin: readOnly,
      convertEol: true,
      scrollback: 4000,
      fontFamily: "'JetBrains Mono', ui-monospace, monospace",
      fontSize: 13,
      lineHeight: 19 / 13,
      theme: { background: TERM_BG, foreground: TERM_FG, cursor: TERM_FG },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => webgl.dispose());
      term.loadAddon(webgl);
    } catch (err) {
      console.warn("WebGL terminal renderer is unavailable", err);
    }
    fit.fit();
    terminalRef.current = term;
    setTerminal(term);

    const onResize = () => fit.fit();
    window.addEventListener("resize", onResize);
    const resizeObserver = new ResizeObserver(onResize);
    resizeObserver.observe(host);

    return () => {
      window.removeEventListener("resize", onResize);
      resizeObserver.disconnect();
      term.dispose();
      terminalRef.current = null;
      setTerminal(null);
    };
    // Deliberately mount once per session id — see the session-id effect below for resets.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  useEffect(() => {
    if (!terminal) return;
    terminal.options.disableStdin = readOnly;
    terminal.options.cursorBlink = !readOnly;
  }, [readOnly, terminal]);

  useEffect(() => {
    if (!terminal || !onDimensionsChange) return;
    onDimensionsChange(terminal.cols, terminal.rows);
    const disposable = terminal.onResize(({ cols, rows }) => onDimensionsChange(cols, rows));
    return () => disposable.dispose();
  }, [onDimensionsChange, terminal]);

  const { attached: realAttached, sendBytes: sendRealBytes } = useRealTerminalSession(
    isMockData || readOnly ? "" : sessionId,
    isMockData || readOnly ? null : terminal,
  );

  useEffect(() => {
    onAttachedChange?.(isMockData ? true : realAttached);
  }, [isMockData, realAttached, onAttachedChange]);

  useEffect(() => {
    if (isMockData || !readOnly || !terminal) return;
    let cancelled = false;
    setReplayWarning(null);
    setReplayError(null);
    void dataAdapter
      .replayOutput(sessionId, (chunk) => {
        if (!cancelled) terminal.write(chunk);
      })
      .then((result) => {
        if (!cancelled) setReplayWarning(result.warning ?? null);
      })
      .catch((error: unknown) => {
        if (!cancelled) setReplayError(error instanceof Error ? error.message : "Saved output could not be loaded.");
      });
    return () => {
      cancelled = true;
    };
  }, [readOnly, sessionId, terminal]);

  // Mock mode: seed scrollback from the session's events, then replay only newly
  // appended ones as they arrive (LiveDataContext keeps `session.events` growing).
  useEffect(() => {
    if (!isMockData || !terminal || readOnly) return;
    const events = session.events ?? [];
    if (seenEventCount.current === 0) {
      for (const event of events) writeEventLine(terminal, event);
      if (session.prompt) {
        terminal.write(`\x1b[38;2;255;208;0m${session.prompt}\x1b[0m\r\n`);
      }
      terminal.write(promptString(session));
    } else if (events.length > seenEventCount.current) {
      for (const event of events.slice(seenEventCount.current)) writeEventLine(terminal, event);
    }
    seenEventCount.current = events.length;
  }, [isMockData, terminal, session.events, session.prompt]);

  // Mock mode: local line buffering — bytes typed into the terminal become one
  // sendInput() call per line (Enter), mirroring how a real shell prompt behaves.
  useEffect(() => {
    if (!isMockData || !terminal) return;
    const disposable = terminal.onData((data) => {
      for (const ch of data) {
        const code = ch.charCodeAt(0);
        if (code === 3) {
          // QUESTION: a raw Ctrl-C keystroke typed into the terminal passes straight
          // through as terminal input (normal terminal behavior); only the explicit
          // key-bar/header "Ctrl-C…" buttons are gated behind the confirm dialog
          // (rule 5 reads as gating the app's action affordances, not every keystroke
          // a user can type into an attached terminal). See QUESTIONS.md.
          terminal.write(`^C\r\n${promptString(session)}`);
          inputLineRef.current = "";
          continue;
        }
        if (code === 13) {
          const line = inputLineRef.current;
          terminal.write(`\r\n${promptString(session)}`);
          inputLineRef.current = "";
          if (line.length > 0) void dataAdapter.sendInput(sessionId, line);
          continue;
        }
        if (code === 127) {
          if (inputLineRef.current.length > 0) {
            inputLineRef.current = inputLineRef.current.slice(0, -1);
            terminal.write("\b \b");
          }
          continue;
        }
        inputLineRef.current += ch;
        terminal.write(ch);
      }
    });
    return () => disposable.dispose();
  }, [isMockData, readOnly, terminal, sessionId]);

  function pressKey(label: string, bytes: number[]) {
    if (isMockData) {
      // No raw pty in mock mode — mirror the design's own mock behavior for these
      // two keys (Tab inserts two spaces, Esc clears the line); ↑/↓ have no defined
      // behavior in the source prototype either (its own `hist` state is unused).
      const term = terminalRef.current;
      if (!term) return;
      if (label === "Tab") {
        inputLineRef.current += "  ";
        term.write("  ");
      } else if (label === "Esc") {
        term.write("\b \b".repeat(inputLineRef.current.length));
        inputLineRef.current = "";
      }
      return;
    }
    sendRealBytes(new Uint8Array(bytes));
  }

  const fill = height === "fill";

  return (
    <div className={className}>
      <div
        ref={hostRef}
        className={fill ? `${styles.host} ${styles.hostFill}` : styles.host}
        style={fill ? undefined : { height }}
        data-testid="terminal-host"
        aria-readonly={readOnly}
      />
      {replayWarning && <p className={styles.replayWarning} role="alert">Saved output warning: {replayWarning}</p>}
      {replayError && <p className={styles.replayWarning} role="alert">{replayError}</p>}
      {showKeyBar && !readOnly && (
        <div className={styles.keyBar} aria-label="Terminal keys">
          <button type="button" className={`${styles.key} ${styles.ctrlC}`} onClick={onRequestInterrupt}>
            Ctrl‑C
          </button>
          {KEYS.map(([label, bytes]) => (
            <button key={label} type="button" className={styles.key} onClick={() => pressKey(label, bytes)}>
              {label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
