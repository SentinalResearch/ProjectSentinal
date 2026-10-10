import { useNavigate } from "react-router-dom";

import { StateTag } from "../../../components/StateTag";
import { dataAdapter } from "../../../data";
import { captureCapabilities, captureOf } from "../../../data/capture";
import type { Machine, Session } from "../../../data/types";
import styles from "./phoneSession.module.css";

const STATE_LINE: Record<string, string> = {
  WAITING: "var(--s-waiting)",
  FAILED: "var(--s-failed)",
  STALLED: "var(--s-stalled)",
};

/** Answer a prompt: the key and Enter, as the design's waiting screens promise. */
export function answer(sessionId: string, key: "y" | "n") {
  void dataAdapter.sendInput(sessionId, `${key}\r`);
}

/** A "Needs you" card with a 3 px top line in the state colour. */
export function NeedsYouCard({ session, machine }: { session: Session; machine?: Machine }) {
  const navigate = useNavigate();
  const canAnswer = session.state === "WAITING" && Boolean(session.prompt) && captureCapabilities(captureOf(session)).canSendInput;
  return (
    <article
      className={styles.needsCard}
      aria-label={session.name}
      style={{ boxShadow: `inset 0 3px 0 ${STATE_LINE[session.state] ?? "var(--line-strong)"}` }}
    >
      <button type="button" className={styles.needsOpen} onClick={() => navigate(`/app/sessions/${session.id}`)}>
        <span className={styles.rowStatus}>
          <StateTag state={session.state} size="sm" />
          <span className={styles.machineName}>{machine?.name ?? session.machine}</span>
        </span>
        <span className={styles.rowTitle}>{session.name}</span>
        <span className={styles.needsFact}>{session.result?.summary ?? session.details ?? session.command ?? ""}</span>
      </button>
      {session.state === "WAITING" && session.prompt && (
        <div className={styles.promptRow}>
          <span className={`${styles.promptText} mono`}>{session.prompt}</span>
          {canAnswer && (
            <>
              <button type="button" className={styles.answerKey} onClick={() => answer(session.id, "y")}>y</button>
              <button type="button" className={styles.answerKey} onClick={() => answer(session.id, "n")}>n</button>
            </>
          )}
        </div>
      )}
    </article>
  );
}
