import { useState } from "react";

import { dataAdapter } from "../data";
import type { Session } from "../data/types";
import styles from "./OutputSearchToggle.module.css";

/**
 * Opt a FULL Session's saved output into search (5.S). Off by default; the
 * Machine scans the log only when asked and hides secret-shaped values.
 */
export function OutputSearchToggle({ session }: { session: Session }) {
  const [enabled, setEnabled] = useState(Boolean(session.outputSearch));
  const [error, setError] = useState<string | null>(null);

  async function change(next: boolean) {
    setEnabled(next);
    setError(null);
    try {
      await dataAdapter.setOutputSearch(session.id, next);
    } catch (failure) {
      setEnabled(!next);
      setError(failure instanceof Error ? failure.message : "Could not change output search.");
    }
  }

  return (
    <label className={styles.toggle}>
      <input type="checkbox" checked={enabled} onChange={(event) => void change(event.target.checked)} />
      <span>
        Searchable output
        <span className={styles.hint}> · search may read this Session's saved output, with secrets hidden</span>
      </span>
      {error && (
        <span className={styles.error} role="alert">
          {error}
        </span>
      )}
    </label>
  );
}
