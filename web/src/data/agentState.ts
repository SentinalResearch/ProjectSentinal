import type { AgentReportedState } from "../protocol/AgentReportedState";
import type { AgentStatus } from "../protocol/AgentStatus";

/** What an agent running in a Session last said it is doing (6.4). */
export interface AgentStateView {
  status: AgentStatus;
  detail?: string;
  reportedAt: number;
}

export function mapAgentState(wire: AgentReportedState | null | undefined): AgentStateView | undefined {
  if (!wire) return undefined;
  return { status: wire.status, detail: wire.detail ?? undefined, reportedAt: wire.reported_at };
}

/** How the UI names each reported status. */
export const AGENT_STATUS_WORD: Record<AgentStatus, string> = {
  WORKING: "working",
  BLOCKED: "blocked",
  WAITING: "waiting",
  DONE: "done",
};

/** "blocked · needs approval for cargo test" */
export function agentStateText(state: AgentStateView): string {
  return state.detail ? `${AGENT_STATUS_WORD[state.status]} · ${state.detail}` : AGENT_STATUS_WORD[state.status];
}

const AGENT_NAME: Record<string, string> = {
  CLAUDE: "claude",
  CODEX: "codex",
  AIDER: "aider",
  GEMINI: "gemini",
  VIBE: "vibe",
  ANTIGRAVITY: "antigravity",
};

/** Who is speaking: the detected coding agent, else "agent". */
export function reporterName(codingAgent: { agent: string } | undefined): string {
  return (codingAgent && AGENT_NAME[codingAgent.agent]) ?? "agent";
}
