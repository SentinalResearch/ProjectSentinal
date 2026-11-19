import { captureCapabilities, captureExplanation, captureOf } from "../../data/capture";
import { historyWindow, type SessionLineage } from "../../data/lineage";
import { TerminalPane } from "../../terminal/TerminalPane";
import { isReadOnlyTerminalState } from "../../terminal/lifecycle";
import styles from "./SessionDetailPage.module.css";

interface SessionHistoryTerminalsProps {
  lineage: SessionLineage;
  jumpTo?: { id: string; nonce: number };
  onAttachedChange: (attached: boolean) => void;
  onRequestInterrupt: () => void;
}

/**
 * One terminal for the whole Session (design H1): every earlier part's saved
 * output, oldest first, with a Resumed tag before each next part, then the
 * live (or last) part. Each part keeps its own saved output underneath.
 */
export function SessionHistoryTerminals({
  lineage,
  jumpTo,
  onAttachedChange,
  onRequestInterrupt,
}: SessionHistoryTerminalsProps) {
  const window = historyWindow(lineage);
  const latest = window.latest;
  if (!captureCapabilities(captureOf(latest)).canAttach) {
    return (
      <p className={styles.terminalUnavailable} role="note">
        {captureExplanation(captureOf(latest))}
      </p>
    );
  }
  return (
    <div className={styles.history}>
      {window.offset > 0 && (
        <p className={styles.historyOmitted} role="status">
          {window.offset} earlier {window.offset === 1 ? "part is" : "parts are"} not shown here; their saved output is
          kept.
        </p>
      )}
      <TerminalPane
        key={latest.id}
        sessionId={latest.id}
        session={latest}
        height="fill"
        className={styles.fillPane}
        forceReadOnly={isReadOnlyTerminalState(latest.state)}
        replayLimitBytes={window.bytesPerPart}
        prior={{
          parts: window.parts,
          offset: window.offset,
          total: window.total,
          bytesPerPart: window.bytesPerPart,
        }}
        jumpTo={jumpTo}
        onAttachedChange={onAttachedChange}
        onRequestInterrupt={onRequestInterrupt}
      />
    </div>
  );
}
