import { useEffect, useState } from "react";
import { Navigate, useParams } from "react-router-dom";

import { StateTag } from "../../../components/StateTag";
import { dataAdapter } from "../../../data";
import { canAnswer } from "../../../data/actions";
import { useLiveData } from "../../../data/LiveDataContext";
import { useIsPhone } from "../../../hooks/useViewport";
import type { KeyboardChip } from "../../../terminal/keyboard/TermKeyboard";
import { TerminalPane } from "../../../terminal/TerminalPane";
import { BackBar } from "./BackBar";
import { ConfirmSignalSheet } from "./ConfirmSignalSheet";
import { needsInterruptConfirm } from "./interrupt";
import styles from "./phoneSession.module.css";

/** `/app/sessions/:id/terminal`: C1 on phones; desktop keeps the terminal on the Session page. */
export function SessionTerminalRoute() {
  const { id = "" } = useParams<{ id: string }>();
  return useIsPhone() ? <PhoneTerminal /> : <Navigate to={`/app/sessions/${id}`} replace />;
}

/** The most recent distinct Commands of a Session, newest first. */
function useRecentCommands(sessionId: string): string[] {
  const [commands, setCommands] = useState<string[]>([]);
  useEffect(() => {
    let live = true;
    dataAdapter
      .getHistory({ sessionId })
      .then((results) => {
        if (!live) return;
        const texts = results.map((entry) => entry.command).filter(Boolean);
        setCommands(Array.from(new Set(texts)).slice(0, 3));
      })
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [sessionId]);
  return commands;
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
  const recent = useRecentCommands(id);
  const session = sessions.find((candidate) => candidate.id === id);
  const machineName = machines.find((candidate) => candidate.id === session?.machine)?.name ?? session?.machine ?? "";

  const chips: KeyboardChip[] = [
    ...(session && canAnswer(session) ? [{ label: "y", text: "y" }, { label: "n", text: "n" }] : []),
    ...recent.map((command) => ({ label: `↑ ${command}`, text: command, accent: true })),
    { label: "clear", text: "clear" },
    { label: "exit", text: "exit" },
  ];

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
          keyboard={{ visible: keyboardShown, hardware, chips }}
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
