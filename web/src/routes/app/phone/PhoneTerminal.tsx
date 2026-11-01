import { useEffect, useState } from "react";
import { Navigate, useParams } from "react-router-dom";

import { StateTag } from "../../../components/StateTag";
import { dataAdapter } from "../../../data";
import { canAnswer } from "../../../data/actions";
import { useLiveData } from "../../../data/LiveDataContext";
import { useIsPhone } from "../../../hooks/useViewport";
import type { KeyboardChip } from "../../../terminal/keyboard/TermKeyboard";
import { TerminalPane } from "../../../terminal/TerminalPane";
import { nextTypedLine } from "../../../terminal/keyboard/typedLine";
import { BackBar } from "./BackBar";
import { ConfirmSignalSheet } from "./ConfirmSignalSheet";
import { needsInterruptConfirm } from "./interrupt";
import styles from "./phoneSession.module.css";

/** `/app/sessions/:id/terminal`: C1 on phones; desktop keeps the terminal on the Session page. */
export function SessionTerminalRoute() {
  const { id = "" } = useParams<{ id: string }>();
  return useIsPhone() ? <PhoneTerminal /> : <Navigate to={`/app/sessions/${id}`} replace />;
}

/** Typing pauses this long before the Machine is asked. */
const COMPLETE_DEBOUNCE_MS = 150;
const COMPLETION_CHIPS = 3;

/**
 * Completions for the line being typed, from the Session's own Machine
 * (5.A). An empty line gets its recent habits; an unknown line (after arrows
 * or Tab) gets none.
 */
function useCompletions(sessionId: string, line: string | null): string[] {
  const [found, setFound] = useState<{ line: string; texts: string[] }>();
  useEffect(() => {
    if (line === null) return;
    let live = true;
    const timer = setTimeout(() => {
      dataAdapter
        .complete(sessionId, line, COMPLETION_CHIPS)
        .then((completions) => live && setFound({ line, texts: completions.map((completion) => completion.text) }))
        .catch(() => live && setFound({ line, texts: [] }));
    }, COMPLETE_DEBOUNCE_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [sessionId, line]);
  return found !== undefined && found.line === line ? found.texts : [];
}

/** A physical key press means a hardware keyboard (MOBILE_GLASS_SPEC §4). */
function useHardwareKeyboard(): boolean {
  const [hardware, setHardware] = useState(false);
  useEffect(() => {
    if (hardware) return;
    const onKeyDown = () => setHardware(true);
    window.addEventListener("keydown", onKeyDown, { once: true });
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [hardware]);
  return hardware;
}

/** C1 · full-screen phone terminal with the built-in keyboard (MOBILE_GLASS_SPEC §4, M3–M5). */
export function PhoneTerminal() {
  const { id = "" } = useParams<{ id: string }>();
  const { machines, sessions, loading } = useLiveData();
  const [sheetOpen, setSheetOpen] = useState(false);
  const [keyboardShown, setKeyboardShown] = useState(true);
  const hardware = useHardwareKeyboard();
  // Assume a fresh prompt when the terminal opens.
  const [line, setLine] = useState<string | null>("");
  const completions = useCompletions(id, line);
  const session = sessions.find((candidate) => candidate.id === id);
  const machineName = machines.find((candidate) => candidate.id === session?.machine)?.name ?? session?.machine ?? "";

  const offered: KeyboardChip[] = [
    ...(session && canAnswer(session) ? [{ label: "y", text: "y" }, { label: "n", text: "n" }] : []),
    // A chip adds only what is missing: the completion starts with the line.
    ...completions.map((text) => ({ label: text, text: text.slice(line?.length ?? 0), accent: true })),
    { label: "clear", text: "clear" },
    { label: "exit", text: "exit" },
  ];
  // A completion such as `clear` would repeat a fixed chip.
  const chips = offered.filter((chip, index) => offered.findIndex((other) => other.label === chip.label) === index);

  const trailing = session ? (
    <span className={styles.islandTools}>
      <StateTag state={session.state} size="sm" />
      <button
        type="button"
        className={keyboardShown ? styles.keyboardToggle : styles.keyboardChip}
        aria-label={keyboardShown ? "Hide keyboard" : "Show keyboard"}
        aria-pressed={keyboardShown}
        onClick={() => setKeyboardShown((shown) => !shown)}
      >
        {keyboardShown ? "⌨︎" : "keyboard"}
      </button>
    </span>
  ) : undefined;

  return (
    <div className={styles.terminalScreen}>
      <BackBar parentLabel="Session" parentPath={`/app/sessions/${id}`} trailing={trailing} />
      {loading || !session ? (
        <p className={styles.terminalMissing}>{loading ? "Loading…" : "This Session is not available."}</p>
      ) : (
        <TerminalPane
          className={styles.terminalBody}
          sessionId={session.id}
          session={session}
          height="fill"
          keyboard={{
            visible: keyboardShown,
            hardware,
            chips,
            onTyped: (bytes) => setLine((current) => nextTypedLine(current, bytes)),
          }}
          onRequestInterrupt={() => {
            if (needsInterruptConfirm(session)) setSheetOpen(true);
            else void dataAdapter.sendSignal(session.id, "SIGINT");
          }}
        />
      )}
      {session && (
        <ConfirmSignalSheet
          open={sheetOpen}
          session={session}
          machineName={machineName}
          onClose={() => setSheetOpen(false)}
          onConfirm={() => {
            setSheetOpen(false);
            void dataAdapter.sendSignal(session.id, "SIGINT");
          }}
        />
      )}
    </div>
  );
}
