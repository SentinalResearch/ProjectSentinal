import type { AgentReportedState } from "../protocol/AgentReportedState";
import type { AgentStatus } from "../protocol/AgentStatus";

/** What an agent running in a Session last said it is doing (6.4) and how far along it is (6.5). */
export interface AgentStateView {
  status: AgentStatus;
  detail?: string;
  /** The task it says it is on. */
  task?: string;
  /** The step it says it is on, of `steps`. */
  step?: number;
  steps?: number;
  /** The agent's own guess of seconds left: a claim. */
  etaSecs?: number;
  /** When Sentinal first saw this task (known). */
  taskStartedAt?: number;
  /** Sentinal's estimate from the pace of recent steps. */
  estimatedLeftSecs?: number;
  reportedAt: number;
}

export function mapAgentState(wire: AgentReportedState | null | undefined): AgentStateView | undefined {
  if (!wire) return undefined;
  const view: AgentStateView = { status: wire.status, reportedAt: wire.reported_at };
  if (wire.detail != null) view.detail = wire.detail;
  if (wire.task != null) view.task = wire.task;
  if (wire.step != null) view.step = wire.step;
  if (wire.steps != null) view.steps = wire.steps;
  if (wire.eta_secs != null) view.etaSecs = wire.eta_secs;
  if (wire.task_started_at != null) view.taskStartedAt = wire.task_started_at;
  if (wire.estimated_left_secs != null) view.estimatedLeftSecs = wire.estimated_left_secs;
  return view;
}

/** "45 s", "12 m", "2 h 5 m". */
export function shortDuration(secs: number): string {
  if (secs < 60) return `${Math.max(0, Math.round(secs))} s`;
  const minutes = Math.round(secs / 60);
  if (minutes < 60) return `${minutes} m`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest ? `${hours} h ${rest} m` : `${hours} h`;
}

/** The pieces of an agent's progress line, each labelled by who knows it. */
export interface AgentProgressParts {
  task?: string;
  /** "step 3 of 8": reported. */
  steps?: string;
  /** "claude says ~12 m": the agent's claim. */
  said?: string;
  /** "about 15 m at this pace": Sentinal's estimate. */
  pace?: string;
  /** "4 m on task": known by Sentinal. */
  onTask?: string;
}

export function agentProgressParts(state: AgentStateView, who: string, now: number): AgentProgressParts {
  const steps =
    state.step != null ? (state.steps != null ? `step ${state.step} of ${state.steps}` : `step ${state.step}`) : undefined;
  const elapsed = state.taskStartedAt != null ? (now - state.taskStartedAt) / 1000 : undefined;
  return {
    task: state.task,
    steps,
    said: state.etaSecs != null ? `${who} says ~${shortDuration(state.etaSecs)}` : undefined,
    pace: state.estimatedLeftSecs != null ? `about ${shortDuration(state.estimatedLeftSecs)} at this pace` : undefined,
    onTask: elapsed == null ? undefined : elapsed < 60 ? "under a minute on task" : `${shortDuration(elapsed)} on task`,
  };
}

/** Percent for a progress ring from reported steps, or undefined without a plan. */
export function agentPercent(state: AgentStateView | undefined): number | undefined {
  if (!state || state.step == null || !state.steps) return undefined;
  return Math.round((Math.min(state.step, state.steps) / state.steps) * 100);
}

/** How the UI names each reported status. */
export const AGENT_STATUS_WORD: Record<AgentStatus, string> = {
  WORKING: "working",
  BLOCKED: "blocked",
  WAITING: "waiting",
  DONE: "done",
};

/** "blocked · needs approval for cargo test", with "step 3 of 8" when reported (6.5). */
export function agentStateText(state: AgentStateView): string {
  const steps = state.step != null && state.steps ? `step ${state.step} of ${state.steps}` : undefined;
  return [AGENT_STATUS_WORD[state.status], state.task, steps, state.detail].filter(Boolean).join(" · ");
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
