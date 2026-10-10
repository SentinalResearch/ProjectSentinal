import { useEffect, useRef, useState } from "react";
import { Link, useNavigate, useSearchParams } from "react-router-dom";

import { StateTag } from "../../../components/StateTag";
import { dataAdapter } from "../../../data";
import { useLiveData } from "../../../data/LiveDataContext";
import type { Session } from "../../../data/types";
import { usePeekLines } from "../../../terminal/peek";
import { isReadOnlyTerminalState } from "../../../terminal/lifecycle";
import shell from "./phone.module.css";
import styles from "./phoneLists.module.css";

/** True while the element is on screen (always true where IntersectionObserver is missing). */
function useOnScreen<T extends Element>(): [React.RefObject<T | null>, boolean] {
  const ref = useRef<T | null>(null);
  const [visible, setVisible] = useState(typeof IntersectionObserver === "undefined");
  useEffect(() => {
    if (!ref.current || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver(([entry]) => setVisible(entry.isIntersecting));
    observer.observe(ref.current);
    return () => observer.disconnect();
  }, []);
  return [ref, visible];
}

function TerminalCard({ session, machineName, focused }: { session: Session; machineName: string; focused: boolean }) {
  const [ref, visible] = useOnScreen<HTMLAnchorElement>();
  const lines = usePeekLines(session, visible);
  return (
    <Link
      ref={ref}
      to={`/app/sessions/${session.id}/terminal`}
      className={styles.terminalCard}
      data-focused={focused ? "true" : "false"}
    >
      <span className={styles.cardHead}>
        <StateTag state={session.state} size="sm" iconOnly />
        <strong className={styles.cardTitle}>{session.name}</strong>
        <span className={styles.cardMachine}>{machineName}</span>
      </span>
      <span className={`${styles.cardLines} mono`} aria-label="Latest output">
        {lines.length > 0 ? lines.map((line, index) => <span key={index}>{line}</span>) : <span>No output yet</span>}
      </span>
    </Link>
  );
}

/** C3 · Terminals on phones: live terminal cards; tap one for full screen. */
export function PhoneTerminals() {
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const { machines, sessions, loading } = useLiveData();
  const live = sessions.filter((session) => !isReadOnlyTerminalState(session.state));
  const focus = params.get("focus");

  async function newSession() {
    const session = await dataAdapter.createSession();
    navigate(`/app/sessions/${session.id}/terminal`);
  }

  if (loading) return <div className={shell.page}>Loading…</div>;
  return (
    <div className={shell.page}>
      <header className={styles.headerRow}>
        <div>
          <h1 className={shell.pageTitle}>Terminals</h1>
          <span className={styles.subtitle}>{live.length} open · tap one to go full screen</span>
        </div>
        <button type="button" className={styles.plus} aria-label="New session" onClick={() => void newSession()}>
          +
        </button>
      </header>
      <div className={styles.cardStack}>
        {live.map((session) => (
          <TerminalCard
            key={session.id}
            session={session}
            machineName={machines.find((machine) => machine.id === session.machine)?.name ?? session.machine}
            focused={session.id === focus}
          />
        ))}
        {live.length === 0 && <p className={styles.subtitle}>No live terminals.</p>}
      </div>
    </div>
  );
}
