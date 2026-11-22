import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { FIT_DEBOUNCE_MS, trailing, activateUnicode11 } from "./trailing";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useEffect, useRef, useState } from "react";

import { dataAdapter, isMockData } from "../data";
import type { Session, SessionEvent } from "../data/types";
import { TermKeyboard, type KeyboardChip } from "./keyboard/TermKeyboard";
import { outputTokens } from "./keyboard/outputTokens";
import { useRealTerminalSession } from "./useRealTerminalSession";
import { LivePill } from "./LivePill";
import { usePriorParts, type PriorParts } from "./usePriorParts";
import { isReadOnlyTerminalState } from "./lifecycle";
import styles from "./TerminalPane.module.css";
import { attachTouchScroll } from "./touchScroll";
import { BASE_FONT, baseCells, fitFont, sizedForOther } from "./sessionSize";

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
  /** Force a historical generation read-only even if persisted state is malformed. */
  forceReadOnly?: boolean;
  /** Add the phone key bar's y / n answer keys (MOBILE_SPEC.md C1). */
  withAnswerKeys?: boolean;
  /** Maximum decompressed saved bytes this pane may fetch. */
  replayLimitBytes?: number;
  /** Earlier parts of this terminal, drawn above it with Resumed tags (design H1). */
  prior?: PriorParts;
  /** Scroll to the start of one part; a new `nonce` repeats the jump. */
  jumpTo?: { id: string; nonce: number };
  className?: string;
  /**
   * Phones: the built-in keyboard (MOBILE_GLASS_SPEC §4) replaces the key bar
   * and the OS keyboard. ^C from it goes through onRequestInterrupt.
   */
  keyboard?: {
    visible: boolean;
    /** A hardware keyboard is attached: show only the accessory row. */
    hardware: boolean;
    chips?: KeyboardChip[];
    /** Every byte the built-in keyboard sends, for line-aware chips (5.A). */
    onTyped?: (bytes: string) => void;
  };
}

const encoder = new TextEncoder();

const KEYS: [label: string, bytes: number[]][] = [
  ["Esc", [27]],
  ["Tab", [9]],
  ["↑", [27, 91, 65]],
  ["↓", [27, 91, 66]],
];

const ANSWER_KEYS: [label: string, bytes: number[]][] = [
  ["y", [121]],
  ["n", [110]],
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
  forceReadOnly = false,
  replayLimitBytes,
  prior,
  jumpTo,
  className,
  withAnswerKeys = false,
  keyboard,
}: TerminalPaneProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  /** T3: what happens when the box changes; replaced once the Session's size is known. */
  const onBoxChange = useRef<() => void>(() => undefined);
  const inputLineRef = useRef("");
  const [terminal, setTerminal] = useState<Terminal | null>(null);
  const seenEventCount = useRef(0);
  const [replayWarning, setReplayWarning] = useState<string | null>(null);
  const [replayError, setReplayError] = useState<string | null>(null);
  const [replayTruncated, setReplayTruncated] = useState(false);
  const readOnly = forceReadOnly || isReadOnlyTerminalState(session.state);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const term = new Terminal({
      cursorBlink: !readOnly,
      disableStdin: readOnly,
      // Only mock lines need \n → \r\n; real PTY bytes already carry their own
      // carriage returns, and converting breaks shells' cursor movement.
      convertEol: isMockData,
      scrollback: prior && prior.parts.length > 0 ? 20_000 : 4000,
      // Resumed tags are xterm decorations, still a proposed API in xterm 6.
      allowProposedApi: true,
      fontFamily: "'JetBrains Mono', ui-monospace, monospace",
      fontSize: BASE_FONT,
      lineHeight: 19 / 13,
      theme: { background: TERM_BG, foreground: TERM_FG, cursor: TERM_FG },
    });
    const fit = new FitAddon();
    fitRef.current = fit;
    term.loadAddon(fit);
    term.open(host);
    // Browser automation reads terminal text from the DOM renderer.
    if (!navigator.webdriver) {
      try {
        const webgl = new WebglAddon();
        webgl.onContextLoss(() => webgl.dispose());
        term.loadAddon(webgl);
      } catch (err) {
        console.warn("WebGL terminal renderer is unavailable", err);
      }
    }
    // The built-in keyboard replaces the OS one: xterm's input never opens it.
    if (keyboard) term.textarea?.setAttribute("inputmode", "none");
    activateUnicode11(term, new Unicode11Addon());
    fit.fit();
    // The first fit may measure the fallback font; refit once the real one loads.
    void document.fonts?.ready.then(() => {
      if (terminalRef.current === term) onBoxChange.current();
    });
    terminalRef.current = term;
    setTerminal(term);

    onBoxChange.current = () => fit.fit();
    const refit = trailing(() => {
      if (terminalRef.current === term) onBoxChange.current();
    }, FIT_DEBOUNCE_MS);
    const onResize = refit.call;
    window.addEventListener("resize", onResize);
    const resizeObserver = new ResizeObserver(onResize);
    resizeObserver.observe(host);

    return () => {
      refit.cancel();
      window.removeEventListener("resize", onResize);
      resizeObserver.disconnect();
      term.dispose();
      terminalRef.current = null;
      fitRef.current = null;
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

  const { ready: priorReady, markers } = usePriorParts(terminal, session, prior);
  const keepContent = (prior?.parts.length ?? 0) > 0;
  // T3: this box in cells at the base font, whatever font the terminal shows.
  const boxSize = useRef(() => {
    const term = terminalRef.current;
    const proposed = fitRef.current?.proposeDimensions();
    if (!term || !proposed || !proposed.cols || !proposed.rows) return null;
    return baseCells(proposed.cols, proposed.rows, term.options.fontSize ?? BASE_FONT);
  }).current;
  const {
    attached: realAttached,
    sendBytes: sendRealBytes,
    sessionSize,
    report,
  } = useRealTerminalSession(
    isMockData || readOnly ? "" : sessionId,
    isMockData || readOnly || !priorReady ? null : terminal,
    { keepContent, boxSize },
  );
  const [box, setBox] = useState<{ cols: number; rows: number } | null>(null);

  // T3: with a daemon that owns the size, the terminal stays at the Session's
  // size; a box change is reported (and takes effect only while this viewer
  // sets the size), and the font shrinks to fit the width, then the view pans.
  useEffect(() => {
    if (!terminal) return;
    if (!sessionSize) {
      onBoxChange.current = () => fitRef.current?.fit();
      return;
    }
    const apply = () => {
      const current = boxSize();
      if (!current) return;
      setBox(current);
      const font = fitFont(sessionSize.cols, current.cols);
      if (terminal.options.fontSize !== font) terminal.options.fontSize = font;
      if (terminal.cols !== sessionSize.cols || terminal.rows !== sessionSize.rows) {
        terminal.resize(sessionSize.cols, sessionSize.rows);
      }
    };
    onBoxChange.current = () => {
      if (sessionSize.owner) {
        // This window sets the size: fit at once, as before T3, and report;
        // the daemon's SESSION_SIZE then confirms what is already drawn.
        if (terminal.options.fontSize !== BASE_FONT) terminal.options.fontSize = BASE_FONT;
        fitRef.current?.fit();
        const current = boxSize();
        if (current) {
          setBox(current);
          report(current.cols, current.rows, document.activeElement === terminal.textarea);
        }
        return;
      }
      apply();
      const current = boxSize();
      if (current) report(current.cols, current.rows, document.activeElement === terminal.textarea);
    };
    apply();
  }, [boxSize, report, sessionSize, terminal]);

  const useThisSize = () => {
    const current = boxSize();
    if (current) report(current.cols, current.rows, true);
    terminal?.focus();
  };
  const sizedElsewhere = sizedForOther(sessionSize, box);

  useEffect(() => {
    if (!terminal || !jumpTo) return;
    const marker = markers.find((entry) => entry.id === jumpTo.id)?.marker;
    if (marker && !marker.isDisposed) terminal.scrollToLine(marker.line);
    else if (jumpTo.id === sessionId) terminal.scrollToBottom();
  }, [jumpTo, markers, sessionId, terminal]);

  // Full-screen on a touch screen: a drag scrolls the scrollback (or sends
  // arrows to a full-screen program). Small embedded panes leave the drag to the page.
  const touchKeys = useRef<(bytes: string) => void>(() => undefined);
  touchKeys.current = (bytes) => {
    if (!readOnly && !isMockData) sendRealBytes(encoder.encode(bytes));
  };
  useEffect(() => {
    const host = hostRef.current;
    if (!terminal || !host || height !== "fill") return;
    return attachTouchScroll(host, terminal, { sendKeys: (bytes) => touchKeys.current(bytes) });
  }, [terminal, height]);

  useEffect(() => {
    onAttachedChange?.(isMockData ? true : realAttached);
  }, [isMockData, realAttached, onAttachedChange]);

  useEffect(() => {
    if (isMockData || !readOnly || !terminal || !priorReady) return;
    let cancelled = false;
    setReplayWarning(null);
    setReplayError(null);
    setReplayTruncated(false);
    void dataAdapter
      .replayOutput(sessionId, (chunk) => {
        if (!cancelled) terminal.write(chunk);
      }, replayLimitBytes === undefined ? undefined : { maxBytes: replayLimitBytes })
      .then((result) => {
        if (!cancelled) {
          setReplayWarning(result.warning ?? null);
          setReplayTruncated(result.truncated ?? false);
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) setReplayError(error instanceof Error ? error.message : "Saved output could not be loaded.");
      });
    return () => {
      cancelled = true;
    };
  }, [readOnly, replayLimitBytes, sessionId, terminal, priorReady]);

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
  const mockInput = useRef<(data: string) => void>(() => undefined);
  mockInput.current = (data: string) => {
    const term = terminalRef.current;
    if (!term) return;
    for (const ch of data) {
      const code = ch.charCodeAt(0);
      if (code === 3) {
        // QUESTION: a raw Ctrl-C keystroke typed into the terminal passes straight
        // through as terminal input (normal terminal behavior); only the explicit
        // key-bar/header "Ctrl-C…" buttons are gated behind the confirm dialog
        // (rule 5 reads as gating the app's action affordances, not every keystroke
        // a user can type into an attached terminal). See QUESTIONS.md.
        term.write(`^C\r\n${promptString(session)}`);
        inputLineRef.current = "";
        continue;
      }
      if (code === 13) {
        const line = inputLineRef.current;
        term.write(`\r\n${promptString(session)}`);
        inputLineRef.current = "";
        if (line.length > 0) void dataAdapter.sendInput(sessionId, line);
        continue;
      }
      if (code === 127) {
        if (inputLineRef.current.length > 0) {
          inputLineRef.current = inputLineRef.current.slice(0, -1);
          term.write("\b \b");
        }
        continue;
      }
      if (code === 27) continue; // no cursor movement in the mock line editor
      inputLineRef.current += ch;
      term.write(ch);
    }
  };
  useEffect(() => {
    if (!isMockData || !terminal || readOnly) return;
    const disposable = terminal.onData((data) => mockInput.current(data));
    return () => disposable.dispose();
  }, [isMockData, readOnly, terminal, sessionId]);

  /** Built-in keyboard bytes: ^C asks first (§5), the rest goes to the PTY. */
  function typed(bytes: string) {
    keyboard?.onTyped?.(bytes);
    if (bytes === "\x03" && onRequestInterrupt) {
      onRequestInterrupt();
      return;
    }
    if (isMockData) mockInput.current(bytes);
    else sendRealBytes(encoder.encode(bytes));
  }

  // Tokens from recent output for the symbols layer's chips, refreshed while the keyboard shows.
  const [tokens, setTokens] = useState<KeyboardChip[]>([]);
  useEffect(() => {
    if (!keyboard?.visible || !terminal) return;
    const read = () => setTokens(outputTokens(terminal).map((token) => ({ label: token, text: token })));
    read();
    const timer = window.setInterval(read, 3_000);
    return () => window.clearInterval(timer);
  }, [keyboard?.visible, terminal]);

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
      <div className={fill ? `${styles.hostWrap} ${styles.hostWrapFill}` : styles.hostWrap}>
        <div
          ref={hostRef}
          className={fill ? `${styles.host} ${styles.hostFill}` : styles.host}
          style={fill ? undefined : { height }}
          data-testid="terminal-host"
          data-readonly={readOnly}
        />
        <LivePill terminal={terminal} />
      </div>
      {sizedElsewhere && sessionSize && (
        <div className={styles.sizeBar} role="status">
          <span>
            Sized for another device ({sessionSize.cols}×{sessionSize.rows})
          </span>
          <button type="button" className={styles.sizeBarButton} onClick={useThisSize}>
            Use this size
          </button>
        </div>
      )}
      {replayWarning && <p className={styles.replayWarning} role="alert">Saved output warning: {replayWarning}</p>}
      {replayTruncated && (
        <p className={styles.replayWarning} role="status">
          Output truncated in this view; the remaining saved output was not deleted.
        </p>
      )}
      {replayError && <p className={styles.replayWarning} role="alert">{replayError}</p>}
      {keyboard && !readOnly && (keyboard.visible || keyboard.hardware) && (
        <TermKeyboard onBytes={typed} chips={keyboard.chips} symbolChips={tokens} accessoryOnly={keyboard.hardware} />
      )}
      {showKeyBar && !keyboard && !readOnly && (
        <div className={styles.keyBar} aria-label="Terminal keys">
          <button type="button" className={`${styles.key} ${styles.ctrlC}`} onClick={onRequestInterrupt}>
            Ctrl‑C
          </button>
          {(withAnswerKeys ? [...KEYS, ...ANSWER_KEYS] : KEYS).map(([label, bytes]) => (
            <button key={label} type="button" className={styles.key} onClick={() => pressKey(label, bytes)}>
              {label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
