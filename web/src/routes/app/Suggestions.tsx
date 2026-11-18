import { useState } from "react";
import { useNavigate } from "react-router-dom";

import { dataAdapter } from "../../data";
import type { Session } from "../../data/types";
import styles from "./Suggestions.module.css";

/**
 * Design B3, example 2: commands the intelligence layer suggests (level 2).
 * Each is a guess with its reason; nothing runs until the owner taps Run,
 * which opens a new terminal on the same Machine and folder and types it.
 */
export function Suggestions({ session }: { session: Session }) {
  const navigate = useNavigate();
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const list = session.suggestions ?? [];
  if (list.length === 0) return null;

  async function run(command: string) {
    setBusy(command);
    setError(null);
    try {
      const created = await dataAdapter.createSession({ machineId: session.machine, cwd: session.cwd, title: command.slice(0, 40) });
      await dataAdapter.sendInput(created.id, `${command}\r`);
      navigate(`/app/sessions/${encodeURIComponent(created.id)}`);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "Could not start it. Try again.");
    } finally {
      setBusy(null);
    }
  }

  return (
    <section className={styles.wrap} aria-label="Suggested next">
      <h2 className={styles.head}>
        <span className={styles.ring} aria-hidden="true" />
        Suggested next
      </h2>
      {list.map((suggestion) => (
        <div key={suggestion.command} className={styles.row}>
          <span className={styles.text}>
            <code className={styles.command}>{suggestion.command}</code>
            <span className={styles.why}>{suggestion.why}</span>
          </span>
          <button
            type="button"
            className={styles.run}
            disabled={busy !== null}
            onClick={() => void run(suggestion.command)}
            aria-label={`Run ${suggestion.command} in a new terminal`}
          >
            {busy === suggestion.command ? "Starting…" : "Run"}
          </button>
        </div>
      ))}
      <p className={styles.note}>Sentinal thinks · read off the screen. Run opens a new terminal; nothing runs before you tap.</p>
      {error && (
        <p className={styles.error} role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
