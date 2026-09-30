import type { SessionLineage } from "../../data/lineage";
import { captureCapabilities, captureExplanation, captureOf } from "../../data/capture";
import { TerminalPane } from "../../terminal/TerminalPane";
import { isReadOnlyTerminalState } from "../../terminal/lifecycle";
import styles from "./SessionDetailPage.module.css";

interface SessionHistoryTerminalsProps {
  lineage: SessionLineage;
  selectedId: string;
  machineName: string;
  onSelect: (sessionId: string) => void;
  onAttachedChange: (attached: boolean) => void;
  onRequestInterrupt: () => void;
}

function generationTime(session: SessionLineage["ordered"][number]): string {
  if (session.createdAt === undefined) return "time unavailable";
  return new Date(session.createdAt).toLocaleString();
}

/** Render one bounded history window with isolated terminal emulators. */
export function SessionHistoryTerminals({
  lineage,
  selectedId,
  machineName,
  onSelect,
  onAttachedChange,
  onRequestInterrupt,
}: SessionHistoryTerminalsProps) {
  return (
    <div className={styles.history}>
      <nav className={styles.generationNav} aria-label="Session generations">
        {lineage.ordered.map((generation, index) => (
          <button
            type="button"
            key={generation.id}
            aria-current={generation.id === selectedId ? "page" : undefined}
            onClick={() => onSelect(generation.id)}
          >
            {index + 1}
          </button>
        ))}
      </nav>

      {lineage.omittedBefore > 0 && (
        <p className={styles.historyOmitted} role="status">
          {lineage.omittedBefore} earlier generations omitted from this view; their saved data remains available.
        </p>
      )}

      {lineage.visible.map((generation, visibleIndex) => {
        const generationIndex = lineage.visibleStart + visibleIndex;
        const previous = lineage.ordered[generationIndex - 1];
        const isLatest = generation.id === lineage.latest.id;
        const capabilities = captureCapabilities(captureOf(generation));
        const forceReadOnly = !isLatest || isReadOnlyTerminalState(generation.state);
        return (
          <section
            key={generation.id}
            className={styles.generation}
            aria-label={`Generation ${generationIndex + 1} of ${lineage.ordered.length}`}
          >
            {previous && (
              <div className={styles.generationBoundary}>
                Session {previous.id} ended — continued as Session {generation.id} at {generationTime(generation)}
              </div>
            )}
            <div className={styles.generationHeader}>
              <strong>Generation {generationIndex + 1} of {lineage.ordered.length}</strong>
              <span>
                Session {generation.id} · {generation.state}
                {generation.exitCode !== null && generation.exitCode !== undefined
                  ? ` · exit code ${generation.exitCode}`
                  : ""}
                {` · started ${generationTime(generation)}`}
              </span>
            </div>
            {capabilities.canAttach ? (
              <TerminalPane
                sessionId={generation.id}
                session={generation}
                forceReadOnly={forceReadOnly}
                replayLimitBytes={lineage.bytesPerGeneration}
                showKeyBar={isLatest}
                onAttachedChange={isLatest ? onAttachedChange : undefined}
                onRequestInterrupt={isLatest ? onRequestInterrupt : undefined}
              />
            ) : (
              <p className={styles.terminalUnavailable} role="note">
                {captureExplanation(captureOf(generation))}
              </p>
            )}
          </section>
        );
      })}

      {lineage.omittedAfter > 0 && (
        <p className={styles.historyOmitted} role="status">
          {lineage.omittedAfter} later generations omitted from this view; select one from the timeline to move the window.
        </p>
      )}
      <p className={styles.historyMachine}>History stored on {machineName}</p>
    </div>
  );
}
