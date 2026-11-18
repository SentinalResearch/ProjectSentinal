import { agentStateText, reporterName } from "../data/agentState";
import type { Session } from "../data/types";
import styles from "./AgentStateChip.module.css";

/**
 * What an agent said about itself (6.4): a solid outline and "says", so a
 * claim is never mistaken for something Sentinal measured (no marker) or a
 * model's guess (dashed ring).
 */
export function AgentStateChip({ session, compact = false }: { session: Session; compact?: boolean }) {
  const state = session.agentState;
  if (!state) return null;
  const who = reporterName(session.codingAgent);
  return (
    <span
      className={styles.chip}
      data-status={state.status}
      data-compact={compact || undefined}
      title={`Reported by ${who}, not measured by Sentinal`}
    >
      <span className={styles.dot} aria-hidden="true" />
      <span className={styles.text}>
        {who} says · {agentStateText(state)}
      </span>
    </span>
  );
}

/** "Sandbox" with the honest one-line explanation. */
export function SandboxBadge({ session }: { session: Session }) {
  if (!session.sandbox) return null;
  return (
    <span className={styles.sandbox} title="Separate working folder. Still runs as you; not a security boundary.">
      Sandbox
    </span>
  );
}
