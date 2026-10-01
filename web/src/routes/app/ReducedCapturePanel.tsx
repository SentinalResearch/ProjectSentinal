import { captureExplanation, captureOf, logHint, watchHint } from "../../data/capture";
import type { Session } from "../../data/types";
import { EventsList } from "./EventsList";
import { ResourceBars } from "./ResourceBars";
import { SessionCommands } from "./SessionCommands";
import styles from "./ReducedCapturePanel.module.css";
import { WatchHint } from "./WatchHint";

/**
 * Takes the terminal's place when capture has no terminal: the Command view
 * for COMMANDS, the read-only watched-log Events for PROCESS_LOG, and Metrics
 * for PROCESS. It offers no input, resize, signal, or kill control.
 */
export function ReducedCapturePanel({
  session,
  showSummary = true,
}: {
  session: Session;
  /** False where the page header already shows the explanation and log status. */
  showSummary?: boolean;
}) {
  const capture = captureOf(session);
  const hint = watchHint(session);
  return (
    <div className={styles.panel}>
      {showSummary && (
        <p className={styles.explanation} role="note">
          <strong>{capture.replace("_", " ")}</strong> {captureExplanation(capture)}
        </p>
      )}
      {capture === "COMMANDS" && <SessionCommands sessionId={session.id} />}
      {capture === "PROCESS_LOG" && (
        <section aria-label="Watched log">
          {showSummary && session.log && <p className={`${styles.logPath} mono`}>{logHint(session)}</p>}
          <EventsList events={session.events ?? []} title="Log events" />
        </section>
      )}
      {capture === "PROCESS" &&
        (session.metrics ? (
          <ResourceBars metrics={session.metrics} />
        ) : (
          <p className={styles.waiting}>Waiting for the first Metrics sample.</p>
        ))}
      {hint && <WatchHint command={hint} />}
    </div>
  );
}
