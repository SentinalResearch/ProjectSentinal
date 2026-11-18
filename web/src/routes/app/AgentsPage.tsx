import { useEffect, useId, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";

import { AgentStateChip } from "../../components/AgentStateChip";
import { PromptChoices } from "../../components/PromptChoices";
import { StateIcon } from "../../components/StateIcon";
import { answerPrompt, canAnswer, offeredChoices } from "../../data/actions";
import { AGENT_STATUS_WORD, agentPercent, agentProgressParts, reporterName } from "../../data/agentState";
import { needsAttention } from "../../data/format";
import { useLiveData } from "../../data/LiveDataContext";
import type { Machine, Session } from "../../data/types";
import styles from "./AgentsPage.module.css";

/** The four filters of the Agents view (6.5). */
export type AgentFilter = "all" | "needs" | "working" | "done";

const FILTERS: { key: AgentFilter; label: string }[] = [
  { key: "all", label: "All" },
  { key: "needs", label: "Needs you" },
  { key: "working", label: "Working" },
  { key: "done", label: "Done" },
];

type Bucket = Exclude<AgentFilter, "all"> | "idle";

/** Where an agent belongs: a question or a blocked report needs you. */
function bucket(session: Session): Bucket {
  if (needsAttention(session) || session.agentState?.status === "BLOCKED") return "needs";
  if (session.agentState?.status === "DONE" || session.state === "COMPLETED") return "done";
  if (session.state === "RUNNING") return "working";
  return "idle";
}

const RANK: Record<Bucket, number> = { needs: 0, working: 1, done: 2, idle: 3 };

/** A Session counts as an agent when Sentinal detected one or it reported itself. */
function isAgent(session: Session): boolean {
  return !session.dismissed && Boolean(session.codingAgent || session.agentState);
}

export interface AgentGroup {
  label: string;
  sessions: Session[];
}

/** Agents grouped by Project, then by folder; groups and rows that need you first. */
export function agentGroups(sessions: Session[], filter: AgentFilter): AgentGroup[] {
  const groups = new Map<string, Session[]>();
  for (const session of sessions.filter(isAgent)) {
    if (filter !== "all" && bucket(session) !== filter) continue;
    const label = session.project ?? session.cwd ?? "Other";
    groups.set(label, [...(groups.get(label) ?? []), session]);
  }
  const best = (list: Session[]) => Math.min(...list.map((s) => RANK[bucket(s)]));
  return [...groups.entries()]
    .map(([label, list]) => ({ label, sessions: [...list].sort((l, r) => RANK[bucket(l)] - RANK[bucket(r)]) }))
    .sort((left, right) => best(left.sessions) - best(right.sessions) || left.label.localeCompare(right.label));
}

function useNow(everyMs = 30_000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), everyMs);
    return () => window.clearInterval(timer);
  }, [everyMs]);
  return now;
}

/** /app/agents: every agent on every Machine (6.5). */
export function AgentsPage() {
  const { terminals, machines } = useLiveData();
  const [params, setParams] = useSearchParams();
  const now = useNow();
  const shown = params.get("show");
  const filter: AgentFilter = FILTERS.some((f) => f.key === shown) ? (shown as AgentFilter) : "all";
  return (
    <AgentsView
      sessions={terminals}
      machines={machines}
      now={now}
      filter={filter}
      onFilter={(next) => setParams(next === "all" ? {} : { show: next }, { replace: true })}
    />
  );
}

export function AgentsView({
  sessions,
  machines,
  now,
  filter,
  onFilter,
}: {
  sessions: Session[];
  machines: Machine[];
  now: number;
  filter: AgentFilter;
  onFilter: (filter: AgentFilter) => void;
}) {
  const all = sessions.filter(isAgent);
  const groups = agentGroups(sessions, filter);
  const count = (key: AgentFilter) => (key === "all" ? all.length : all.filter((s) => bucket(s) === key).length);
  const needs = count("needs");
  const machineName = (id: string) => machines.find((machine) => machine.id === id)?.name ?? id;

  return (
    <div className={styles.page}>
      <header className={styles.header}>
        <h1 className={styles.title}>Agents</h1>
        <p className={styles.subtitle}>
          {all.length} agent{all.length === 1 ? "" : "s"}
          {needs > 0 && (
            <>
              {" · "}
              <span className={styles.accent}>{needs} need{needs === 1 ? "s" : ""} you</span>
            </>
          )}
        </p>
      </header>

      <div className={styles.filters} role="group" aria-label="Show">
        {FILTERS.map(({ key, label }) => (
          <button key={key} type="button" className={styles.pill} aria-pressed={filter === key} onClick={() => onFilter(key)}>
            {label}
            <span className={styles.pillCount}>{count(key)}</span>
          </button>
        ))}
      </div>

      {groups.length === 0 ? (
        <div className={styles.empty}>
          <p>No agents here yet.</p>
          <p className={styles.muted}>
            Run Claude Code, Codex or any agent in a Sentinal terminal. It can report its task with{" "}
            <code>sentinal agent-state</code>, and a script can print <code>::sentinal task</code> lines.
          </p>
        </div>
      ) : (
        groups.map((group) => (
          <section key={group.label} className={styles.group} aria-label={group.label}>
            <h2 className={styles.groupHead}>
              <span>{group.label}</span>
              <span className={styles.muted}>
                {group.sessions.length} agent{group.sessions.length === 1 ? "" : "s"}
              </span>
            </h2>
            {group.sessions.map((session) => (
              <AgentRow key={session.id} session={session} machineName={machineName(session.machine)} now={now} />
            ))}
          </section>
        ))
      )}
    </div>
  );
}

function AgentRow({ session, machineName, now }: { session: Session; machineName: string; now: number }) {
  const navigate = useNavigate();
  const titleId = `agent-${useId()}`;
  const state = session.agentState;
  const who = reporterName(session.codingAgent);
  const parts = state ? agentProgressParts(state, who, now) : {};
  const percent = agentPercent(state);
  const answerable = canAnswer(session);
  const choices = answerable && offeredChoices(session).length > 0;
  const open = () => navigate(`/app/sessions/${session.id}`);
  const statusWord = state ? AGENT_STATUS_WORD[state.status] : session.state === "WAITING" ? "waiting" : session.state.toLowerCase();
  const facts = [parts.steps, parts.said, parts.pace, parts.onTask].filter(Boolean);

  return (
    <article className={styles.row} aria-labelledby={titleId} data-bucket={bucket(session)}>
      <StateIcon state={session.state} size={20} />
      <div className={styles.body}>
        <h3 className={styles.rowTitle} id={titleId}>
          <button type="button" className={styles.titleButton} onClick={open}>
            {parts.task ?? session.name}
          </button>
          <span className={styles.who}>{who}</span>
          <span className={styles.status} data-status={state?.status ?? session.state}>
            {statusWord}
          </span>
        </h3>
        <p className={styles.meta}>
          {machineName}
          {session.state === "WAITING" && session.prompt ? (
            <>
              {" · "}
              <span className={styles.mono}>{session.prompt}</span>
            </>
          ) : (
            state?.detail && <> · {state.detail}</>
          )}
        </p>
        {facts.length > 0 && (
          // Reported values (steps, the agent's ETA) carry the solid outline;
          // Sentinal's own pace and time on task stay plain.
          <p className={styles.facts}>
            {parts.steps && (
              <span className={styles.reported} title={`Reported by ${who}, not measured by Sentinal`}>
                {parts.steps}
              </span>
            )}
            {parts.said && <span className={styles.reported}>{parts.said}</span>}
            {parts.pace && <span>{parts.pace}</span>}
            {parts.onTask && <span className={styles.muted}>{parts.onTask}</span>}
          </p>
        )}
        {!state?.task && state && <AgentStateChip session={session} compact />}
        {choices && (
          <div className={styles.choices}>
            <PromptChoices session={session} />
          </div>
        )}
      </div>
      <div className={styles.actions}>
        {percent !== undefined && (
          <span className={styles.percent} data-quiet={state?.status !== "WORKING" || undefined} aria-label={`${who} says ${percent} % done`}>
            {percent} %
          </span>
        )}
        {answerable && !choices ? (
          <>
            <button type="button" className={styles.yes} onClick={() => void answerPrompt(session.id, "y")} aria-label={`Answer y to ${session.name}`}>
              y
            </button>
            <button type="button" className={styles.no} onClick={() => void answerPrompt(session.id, "n")} aria-label={`Answer n to ${session.name}`}>
              n
            </button>
          </>
        ) : (
          <button type="button" className={styles.pill} onClick={open}>
            Open
          </button>
        )}
      </div>
    </article>
  );
}
