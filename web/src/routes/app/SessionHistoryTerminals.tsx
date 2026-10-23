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

function displayedSessionId(id: string): string {
  return id.includes(":") ? (id.split(":").at(-1) ?? id) : id;
}

/**
 * Render one bounded history window with isolated terminal emulators. The
 * newest visible generation fills the column (with the key bar when it is the
 * latest); earlier ones sit read-only in a scrollable strip above it.
 */
export function SessionHistoryTerminals({
  lineage,
  selectedId,
  machineName,
  onSelect,
  onAttachedChange,
  onRequestInterrupt,
}: SessionHistoryTerminalsProps) {
  const multiple = lineage.ordered.length > 1;
  const lastVisible = lineage.visible.length - 1;

  function renderGeneration(generation: SessionLineage["ordered"][number], visibleIndex: number) {
    const generationIndex = lineage.visibleStart + visibleIndex;
    const previous = lineage.ordered[generationIndex - 1];
    const isLatest = generation.id === lineage.latest.id;
    const primary = visibleIndex === lastVisible;
    const capabilities = captureCapabilities(captureOf(generation));
    const forceReadOnly = !isLatest || isReadOnlyTerminalState(generation.state);
    return (
      <section
        key={generation.id}
        className={primary ? `${styles.generation} ${styles.generationPrimary}` : styles.generation}
        aria-label={`Part ${generationIndex + 1} of ${lineage.ordered.length}`}
      >
        {multiple && previous && (
          <div className={styles.generationBoundary}>
            {previous.state} — resumed at {generationTime(generation)}
          </div>
        )}
        {multiple && (
          <div className={styles.generationHeader}>
            <strong>Part {generationIndex + 1} of {lineage.ordered.length}</strong>
            <span>
              Session {displayedSessionId(generation.id)} · {generation.state}
              {generation.exitCode !== null && generation.exitCode !== undefined
                ? ` · exit code ${generation.exitCode}`
                : ""}
              {` · started ${generationTime(generation)}`}
            </span>
          </div>
        )}
        {capabilities.canAttach ? (
          <TerminalPane
            sessionId={generation.id}
            session={generation}
            height={primary ? "fill" : 220}
            className={primary ? styles.fillPane : undefined}
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
  }

  return (
    <div className={styles.history}>
      {multiple && (
        <div className={styles.historyStack}>
          <nav className={styles.generationNav} aria-label="Terminal parts">
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
              {lineage.omittedBefore} earlier parts omitted from this view; their saved data remains available.
            </p>
          )}
          {lineage.visible.slice(0, lastVisible).map((generation, visibleIndex) =>
            renderGeneration(generation, visibleIndex),
          )}
          {lineage.omittedAfter > 0 && (
            <p className={styles.historyOmitted} role="status">
              {lineage.omittedAfter} later parts omitted from this view; select one from the timeline to move the window.
            </p>
          )}
          <p className={styles.historyMachine}>History stored on {machineName}</p>
        </div>
      )}
      {lastVisible >= 0 && renderGeneration(lineage.visible[lastVisible], lastVisible)}
    </div>
  );
}
