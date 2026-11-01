import { useEffect, useState } from "react";

import { dataAdapter } from "../../data";
import type { InferredEvent } from "../../protocol/InferredEvent";
import styles from "./AgentAccessPanel.module.css";

const num = (value: unknown) => (typeof value === "number" ? String(value) : "?");
const text = (value: unknown) => (typeof value === "string" ? value : "");

/** One inferred fact in plain words; the arguments came through the control's checks. */
export function describeInferred(event: InferredEvent): string {
  const a = event.args;
  switch (event.function) {
    case "progress_updated":
      return a.total == null ? `${text(a.unit)} ${num(a.current)}` : `${text(a.unit)} ${num(a.current)}/${num(a.total)}`;
    case "metric_reported":
      return `${text(a.name)} ${num(a.value)}`;
    case "checkpoint_created":
      return `checkpoint ${text(a.path)}`;
    case "error_detected":
      return `error: ${text(a.kind)}`;
    case "warning_detected":
      return text(a.summary) ? `warning: ${text(a.summary)}` : "warning";
    case "waiting_for_input":
      return `waiting: ${text(a.prompt)}`;
    case "server_started":
      return text(a.url) || `port ${num(a.port)}`;
    case "tests_summary":
      return `tests ${num(a.passed)} passed, ${num(a.failed)} failed, ${num(a.skipped)} skipped`;
    default:
      return event.function.replace(/_/g, " ");
  }
}

function ago(at: number): string {
  const minutes = Math.max(0, Math.round((Date.now() - at) / 60_000));
  return minutes < 1 ? "just now" : minutes < 60 ? `${minutes} min ago` : `${Math.round(minutes / 60)} h ago`;
}

/**
 * Session page (6.2): whether agents may read this terminal (off by default;
 * only agents created with output access can, and only redacted), and what
 * they inferred from it. Inferred facts never replace Sentinal's own Events.
 */
export function AgentAccessPanel({ sessionId }: { sessionId: string }) {
  const [allowed, setAllowed] = useState<boolean | null>(null);
  const [events, setEvents] = useState<InferredEvent[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    const load = async () => {
      try {
        const [isAllowed, inferred] = await Promise.all([
          dataAdapter.agents.outputAllowed(sessionId),
          dataAdapter.agents.inferred(sessionId),
        ]);
        if (live) {
          setAllowed(isAllowed);
          setEvents(inferred);
        }
      } catch {
        if (live) setAllowed(null);
      }
    };
    void load();
    const timer = window.setInterval(() => void load(), 15_000);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [sessionId]);

  if (allowed === null) return null;

  async function toggle() {
    const next = !allowed;
    setError(null);
    try {
      await dataAdapter.agents.setOutputAllowed(sessionId, next);
      setAllowed(next);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "Could not change agent access.");
    }
  }

  const recent = events.slice(-6).reverse();
  return (
    <section className={styles.panel} aria-labelledby={`agents-${sessionId}`}>
      <div className={styles.head}>
        <h2 id={`agents-${sessionId}`} className={styles.title}>
          AI agents
        </h2>
        <button
          type="button"
          role="switch"
          aria-checked={allowed}
          className={styles.switch}
          data-on={allowed || undefined}
          onClick={() => void toggle()}
        >
          <span className={styles.knob} aria-hidden="true" />
          <span className={styles.switchLabel}>Let agents read this terminal</span>
        </button>
      </div>
      <p className={styles.note}>
        {allowed
          ? "Agents you created with output access can read the last 60 lines, with secrets redacted. Every read is logged."
          : "Off: no agent can read this terminal."}
      </p>
      {error && (
        <p className={styles.error} role="alert">
          {error}
        </p>
      )}
      {recent.length > 0 && (
        <ul className={styles.list}>
          {recent.map((event) => (
            <li key={event.id} className={styles.item}>
              <span className={`${styles.fact} mono`}>{describeInferred(event)}</span>
              <span className={styles.meta}>
                inferred · via {event.agent_name} · {ago(event.at)}
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
