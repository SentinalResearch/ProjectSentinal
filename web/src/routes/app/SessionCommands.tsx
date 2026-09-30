import { useEffect, useState } from "react";

import { dataAdapter } from "../../data";
import type { HistoryEntry } from "../../data/types";
import styles from "./SessionDetailPage.module.css";

/** Show bounded structured Commands for one selected Session generation. */
export function SessionCommands({ sessionId }: { sessionId: string }) {
  const [commands, setCommands] = useState<HistoryEntry[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setCommands([]);
    setError(null);
    void dataAdapter
      .getHistory({ sessionId })
      .then((history) => {
        if (!cancelled) setCommands(history.filter((entry) => entry.sessionId === sessionId));
      })
      .catch((reason: unknown) => {
        if (!cancelled) setError(reason instanceof Error ? reason.message : "Commands could not be loaded.");
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId]);

  if (error) return <p className={styles.commandHistoryError}>{error}</p>;
  if (commands.length === 0) return null;
  return (
    <section className={styles.commandHistory} aria-label={`Commands for Session ${sessionId}`}>
      <h2>COMMANDS · SESSION {sessionId}</h2>
      <ol>
        {commands.map((command) => (
          <li key={command.id}>
            <span>{command.ok === null ? "●" : command.ok ? "✓" : "✗"}</span>
            <code>{command.command}</code>
            <span>{command.duration}</span>
          </li>
        ))}
      </ol>
    </section>
  );
}
