import { useCallback, useEffect, useState, type FormEvent } from "react";

import { dataAdapter } from "../../data";
import { formatEventTime } from "../../data/format";
import type { AgentActivity } from "../../protocol/AgentActivity";
import type { AgentInfo } from "../../protocol/AgentInfo";
import type { AgentLevel } from "../../protocol/AgentLevel";
import type { CreatedAgent } from "../../protocol/CreatedAgent";
import styles from "./SettingsPage.module.css";
import own from "./AgentsTab.module.css";

/** Levels 0–2 only: the protocol has no level 3 (decision 1, PLAN_V2_AI.md). */
const LEVELS: { value: AgentLevel; label: string; may: string }[] = [
  {
    value: "OBSERVE",
    label: "0 · Observe",
    may: "Read Machines, Sessions, Events and metrics; watch them change.",
  },
  {
    value: "ANNOTATE",
    label: "1 · Annotate",
    may: "Also add notes and proposed Events, marked inferred.",
  },
  {
    value: "SUGGEST",
    label: "2 · Suggest",
    may: "Also suggest commands and answers you accept by hand, and notify.",
  },
];

const NAME = /^[A-Za-z0-9][A-Za-z0-9._-]{0,31}$/;

type Load =
  | { kind: "loading" }
  | { kind: "ready"; agents: AgentInfo[] }
  | { kind: "notControl" }
  | { kind: "error"; message: string };

function levelLabel(level: AgentLevel): string {
  return LEVELS.find((entry) => entry.value === level)?.label ?? level;
}

function when(at: number | null | undefined): string {
  if (at === undefined || at === null) return "never";
  const minutes = Math.round((Date.now() - at) / 60_000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 48) return `${hours} h ago`;
  return new Date(at).toLocaleDateString();
}

function message(error: unknown, fallback: string): string {
  return error instanceof Error ? error.message : fallback;
}

/**
 * Settings → Agents (Milestone 6.0): tokens for AI agents such as Claude
 * Code. The control enforces each agent's level; no level can type into a
 * terminal, run a command, or read terminal output.
 */
export function AgentsTab() {
  const [load, setLoad] = useState<Load>({ kind: "loading" });
  const [name, setName] = useState("");
  const [level, setLevel] = useState<AgentLevel>("OBSERVE");
  const [formError, setFormError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [created, setCreated] = useState<CreatedAgent | null>(null);

  const reload = useCallback(async () => {
    try {
      setLoad({ kind: "ready", agents: await dataAdapter.agents.list() });
    } catch (error) {
      if ((error as { code?: string } | null)?.code === "NOT_CONTROL")
        setLoad({ kind: "notControl" });
      else
        setLoad({
          kind: "error",
          message: message(error, "Could not load agents."),
        });
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function onCreate(event: FormEvent) {
    event.preventDefault();
    const trimmed = name.trim();
    if (!NAME.test(trimmed)) {
      setFormError(
        "Use 1–32 letters, digits, '.', '_' or '-', starting with a letter or digit.",
      );
      return;
    }
    setBusy(true);
    setFormError(null);
    try {
      setCreated(await dataAdapter.agents.create(trimmed, level));
      setName("");
    } catch (error) {
      setFormError(message(error, "Could not create the agent."));
    } finally {
      setBusy(false);
    }
  }

  if (load.kind === "loading")
    return <div className={styles.tabLoading}>Loading…</div>;
  if (load.kind === "notControl") {
    return (
      <p className={styles.tabLoading}>
        Agents are managed only on the control Machine. Open Settings there.
      </p>
    );
  }

  return (
    <>
      <header className={styles.tabHeader}>
        <div className={styles.tabHeaderText}>
          <h2 className={styles.tabTitle}>Agents</h2>
          <span className={styles.tabSubtitle}>
            Tokens for AI agents such as Claude Code. Each has its own level and
            can be revoked at once. No agent can type into a terminal, run
            commands, or read terminal output.
          </span>
        </div>
      </header>

      {created ? (
        <TokenReveal
          created={created}
          onDone={() => {
            setCreated(null);
            void reload();
          }}
        />
      ) : (
        <form className={own.form} onSubmit={onCreate} noValidate>
          <label className={own.field}>
            <span className={styles.fieldLabel}>Name</span>
            <input
              className={styles.pwInput}
              value={name}
              maxLength={32}
              autoComplete="off"
              spellCheck={false}
              placeholder="claude"
              onChange={(event) => setName(event.target.value)}
            />
          </label>
          <label className={own.field}>
            <span className={styles.fieldLabel}>Level</span>
            <select
              className={styles.pwInput}
              value={level}
              onChange={(event) => setLevel(event.target.value as AgentLevel)}
            >
              {LEVELS.map((entry) => (
                <option key={entry.value} value={entry.value}>
                  {entry.label}
                </option>
              ))}
            </select>
          </label>
          <button type="submit" className={styles.primaryBtn} disabled={busy}>
            Create agent
          </button>
          <p className={own.levelHint}>
            {LEVELS.find((entry) => entry.value === level)?.may}
          </p>
          {formError && (
            <p className={styles.rowError} role="alert">
              {formError}
            </p>
          )}
        </form>
      )}

      {load.kind === "error" ? (
        <p className={styles.tabLoading} role="alert">
          {load.message}
        </p>
      ) : load.agents.length === 0 ? (
        <p className={styles.tabLoading}>No agents yet.</p>
      ) : (
        <div className={styles.tableScroll}>
          <table className={own.table}>
            <thead>
              <tr>
                <th scope="col">Name</th>
                <th scope="col">Level</th>
                <th scope="col">Created</th>
                <th scope="col">Last used</th>
                <th scope="col">
                  <span className={own.hidden}>Actions</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {load.agents.map((agent) => (
                <AgentRow
                  key={agent.id}
                  agent={agent}
                  onRevoked={() => void reload()}
                />
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}

function TokenReveal({
  created,
  onDone,
}: {
  created: CreatedAgent;
  onDone: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const origin =
    typeof window === "undefined"
      ? "https://<control>"
      : window.location.origin;
  const command = `claude mcp add sentinal --env SENTINAL_AGENT_TOKEN=<token> -- sentinal mcp --control ${origin}`;
  return (
    <section className={own.reveal} aria-label="New agent token">
      <p className={own.revealTitle}>
        Token for <strong>{created.agent.name}</strong> (
        {levelLabel(created.agent.level)})
      </p>
      <code className={own.token}>{created.token}</code>
      <p className={own.revealNote}>
        It is shown only now; Sentinal keeps just a fingerprint. Store it in the
        agent's settings.
      </p>
      <p className={styles.fieldLabel}>Add it to Claude Code:</p>
      <code className={own.command}>{command}</code>
      <div className={own.revealActions}>
        <button
          type="button"
          className={styles.rotateBtn}
          onClick={() => {
            void navigator.clipboard?.writeText(created.token).then(
              () => setCopied(true),
              () => undefined,
            );
          }}
        >
          {copied ? "Copied" : "Copy token"}
        </button>
        <button type="button" className={styles.primaryBtn} onClick={onDone}>
          I've saved it
        </button>
      </div>
    </section>
  );
}

function AgentRow({
  agent,
  onRevoked,
}: {
  agent: AgentInfo;
  onRevoked: () => void;
}) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [activity, setActivity] = useState<AgentActivity | "loading" | null>(null);

  async function toggleActivity() {
    if (activity !== null) {
      setActivity(null);
      return;
    }
    setActivity("loading");
    try {
      setActivity(await dataAdapter.agents.activity(agent.id));
    } catch (failure) {
      setActivity(null);
      setError(message(failure, "Could not load activity."));
    }
  }

  async function revoke() {
    setBusy(true);
    try {
      await dataAdapter.agents.revoke(agent.id);
      onRevoked();
    } catch (failure) {
      setError(message(failure, "Could not revoke."));
      setBusy(false);
    }
  }

  return (
    <>
      <tr aria-label={agent.name}>
        <td className="mono">{agent.name}</td>
        <td>{levelLabel(agent.level)}</td>
        <td>{when(agent.created_at)}</td>
        <td>{when(agent.last_used_at)}</td>
        <td>
          <div className={own.actions}>
            {error && (
              <span className={styles.rowError} role="alert">
                {error}
              </span>
            )}
            {confirming ? (
              <>
                <span className={styles.revokeConfirmLabel}>
                  Revoke {agent.name}?
                </span>
                <button
                  type="button"
                  className={styles.revokeBtn}
                  disabled={busy}
                  onClick={() => void revoke()}
                >
                  Revoke
                </button>
                <button
                  type="button"
                  className={styles.keepBtn}
                  onClick={() => setConfirming(false)}
                >
                  Keep
                </button>
              </>
            ) : (
              <>
                <button type="button" className={styles.keepBtn} aria-expanded={activity !== null} onClick={() => void toggleActivity()}>
                  {activity === null ? "Activity" : "Hide activity"}
                </button>
                <button
                  type="button"
                  className={styles.revokeEntry}
                  onClick={() => setConfirming(true)}
                >
                  Revoke…
                </button>
              </>
            )}
          </div>
        </td>
      </tr>
      {activity !== null && (
        <tr className={own.activityRow}>
          <td colSpan={5}>
            {activity === "loading" ? (
              <p className={own.activityNote}>Loading…</p>
            ) : (
              <ActivityPanel activity={activity} />
            )}
          </td>
        </tr>
      )}
    </>
  );
}

/** An agent's latest calls, "via claude (level 0)" (6.0 attribution). */
function ActivityPanel({ activity }: { activity: AgentActivity }) {
  const total = Number(activity.total_calls);
  return (
    <section className={own.activity} aria-label={`${activity.agent.name} activity`}>
      <p className={own.activityVia}>
        {activity.via} · {total} call{total === 1 ? "" : "s"}
      </p>
      {activity.calls.length === 0 ? (
        <p className={own.activityNote}>No calls since the control started.</p>
      ) : (
        <ol className={own.activityList}>
          {activity.calls.slice(0, 20).map((call, index) => (
            <li key={`${call.at}-${index}`} className={own.activityItem} data-denied={call.outcome === "DENIED" || undefined}>
              <span className={own.activityTime}>{formatEventTime(call.at)}</span>
              <span className="mono">{call.tool}</span>
              <span>{call.outcome === "ALLOWED" ? "served" : "refused: level too low"}</span>
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}
