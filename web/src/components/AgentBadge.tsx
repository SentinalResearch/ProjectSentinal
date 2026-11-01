import { agentUsageText, codingAgentName } from "../data/format";
import type { CodingAgentView } from "../data/types";
import styles from "./AgentBadge.module.css";

/**
 * "Claude Code" on a Session running an AI coding agent (6.1), with a dot
 * while it works; the tooltip says what it has done and used.
 */
export function AgentBadge({ agent, usage = false }: { agent: CodingAgentView; usage?: boolean }) {
  const name = codingAgentName(agent.agent);
  return (
    <span className={styles.badge} title={`${name} · ${agentUsageText(agent)}`} data-working={agent.working || undefined}>
      {agent.working && <span className={styles.dot} role="img" aria-label={`${name} is working`} />}
      {name}
      {usage && <span className={styles.usage}>{agentUsageText(agent)}</span>}
    </span>
  );
}
