import { useEffect, useState } from "react";
import { Navigate, useLocation, useNavigate, useParams } from "react-router-dom";

import { StateTag } from "../../../components/StateTag";
import { dataAdapter } from "../../../data";
import { useLiveData } from "../../../data/LiveDataContext";
import { useIsPhone } from "../../../hooks/useViewport";
import { TerminalPane } from "../../../terminal/TerminalPane";
import { ConfirmSignalSheet } from "./ConfirmSignalSheet";
import { needsInterruptConfirm } from "./interrupt";
import styles from "./phoneSession.module.css";

/** `/app/sessions/:id/terminal`: C1 on phones; desktop keeps the terminal on the Session page. */
export function SessionTerminalRoute() {
  const { id = "" } = useParams<{ id: string }>();
  return useIsPhone() ? <PhoneTerminal /> : <Navigate to={`/app/sessions/${id}`} replace />;
}

/** Height of the visible viewport, so the key bar sits above the software keyboard. */
function useVisibleHeight(): number | null {
  const [height, setHeight] = useState<number | null>(() => window.visualViewport?.height ?? null);
  useEffect(() => {
    const viewport = window.visualViewport;
    if (!viewport) return;
    const update = () => setHeight(viewport.height);
    viewport.addEventListener("resize", update);
    viewport.addEventListener("scroll", update);
    return () => {
      viewport.removeEventListener("resize", update);
      viewport.removeEventListener("scroll", update);
    };
  }, []);
  return height;
}

/** C1 · full-screen phone terminal with the phone key bar. */
export function PhoneTerminal() {
  const { id = "" } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const location = useLocation();
  const { machines, sessions, loading } = useLiveData();
  const [sheetOpen, setSheetOpen] = useState(false);
  const height = useVisibleHeight();
  const session = sessions.find((candidate) => candidate.id === id);
  const machineName = machines.find((candidate) => candidate.id === session?.machine)?.name ?? session?.machine ?? "";

  function detach() {
    if (location.key !== "default") navigate(-1);
    else navigate(`/app/sessions/${id}`, { replace: true });
  }

  return (
    <div className={styles.terminalScreen} style={height ? { height } : undefined}>
      <header className={styles.terminalHeader}>
        <button type="button" className={styles.detach} onClick={detach}>
          ‹ Detach
        </button>
        <span className={styles.terminalTitle}>
          <strong>{session?.name ?? "Session"}</strong>
          <span className="mono">
            {machineName} · {session?.cwd ?? "~"}
          </span>
        </span>
        {session && <StateTag state={session.state} size="sm" iconOnly />}
      </header>
      {loading || !session ? (
        <p className={styles.terminalMissing}>{loading ? "Loading…" : "This Session is not available."}</p>
      ) : (
        <TerminalPane
          className={styles.terminalBody}
          sessionId={session.id}
          session={session}
          height="fill"
          withAnswerKeys
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
