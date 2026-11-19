import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";

import { agentStateText, shortDuration } from "../data/agentState";
import { chatRows, inlineMarkdown, toolVerb, type ChatRow } from "../data/agentChat";
import type { ChatEntry } from "../protocol/ChatEntry";
import { dataAdapter } from "../data";
import type { Session } from "../data/types";
import styles from "./AgentChatView.module.css";

/** Gap between the text and its Enter, so the Enter is not part of a paste. */
export const ENTER_DELAY_MS = 150;

function clock(at: number): string {
  if (!at) return "";
  const d = new Date(at);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/**
 * An agent Session as a chat (AC2), read from the agent's own conversation
 * file. The terminal stays the truth: the message box types into it.
 */
export function AgentChatView({
  session,
  entries,
  agentName,
  canSend,
}: {
  session: Session;
  entries: ChatEntry[];
  agentName: string;
  canSend: boolean;
}) {
  const rows = useMemo(() => chatRows(entries), [entries]);
  const list = useRef<HTMLDivElement>(null);
  const [now, setNow] = useState(() => Date.now());
  const state = session.agentState;
  const working = state?.status === "WORKING";

  useEffect(() => {
    const node = list.current;
    if (node) node.scrollTop = node.scrollHeight;
  }, [rows.length]);

  useEffect(() => {
    if (!working) return;
    const timer = setInterval(() => setNow(Date.now()), 1_000);
    return () => clearInterval(timer);
  }, [working]);

  const asked = [...rows].reverse().find((row) => row.kind === "user");
  const turnSecs = working && asked && asked.at ? (now - asked.at) / 1000 : undefined;

  return (
    <div className={styles.chat}>
      {state?.steps != null && state.step != null && (
        <div className={styles.plan} role="status">
          <span className={styles.planStep}>
            Step {state.step} of {state.steps}
          </span>
          {state.task && <span className={styles.planTask}>{state.task}</span>}
          {state.estimatedLeftSecs != null && (
            <span className={styles.muted}>about {shortDuration(state.estimatedLeftSecs)} left at this pace</span>
          )}
          <span className={styles.track} aria-hidden="true">
            <span className={styles.fill} style={{ width: `${Math.round((state.step / state.steps) * 100)}%` }} />
          </span>
        </div>
      )}

      <div ref={list} className={styles.list} aria-label={`Conversation with ${agentName}`}>
        {rows.length === 0 && <p className={styles.muted}>Nothing yet. Send {agentName} a message.</p>}
        {rows.map((row) => (
          <Row key={row.key} row={row} agentName={agentName} />
        ))}
        {working && (
          <p className={styles.working} role="status">
            <span className={styles.workingDot} aria-hidden="true" />
            {agentName} is working{state?.detail ? ` · ${state.detail}` : ""}
            {turnSecs != null ? ` · ${shortDuration(turnSecs)}` : ""}
          </p>
        )}
      </div>

      <Composer session={session} agentName={agentName} canSend={canSend} />
      {state && <span className={styles.srOnly}>{agentStateText(state)}</span>}
    </div>
  );
}

function Row({ row, agentName }: { row: ChatRow; agentName: string }) {
  const [open, setOpen] = useState(false);
  switch (row.kind) {
    case "user":
      return (
        <div className={styles.user}>
          <span className={styles.meta}>You · {clock(row.at)}</span>
          <p className={styles.userText}>{row.text}</p>
        </div>
      );
    case "agent":
      return (
        <div className={styles.agent}>
          <span className={styles.meta}>
            <span className={styles.agentName}>{agentName}</span> {clock(row.at)}
          </span>
          <p className={styles.agentText}>
            {inlineMarkdown(row.text).map((part, index) =>
              part.kind === "code" ? (
                <code key={index} className={`${styles.code} mono`}>
                  {part.text}
                </code>
              ) : part.kind === "bold" ? (
                <strong key={index}>{part.text}</strong>
              ) : (
                part.text
              ),
            )}
          </p>
        </div>
      );
    case "turn":
      return (
        <div className={styles.turn} role="separator">
          <span>
            {agentName} finished · {row.toolUses} tool call{row.toolUses === 1 ? "" : "s"} · {clock(row.at)}
          </span>
        </div>
      );
    case "tool": {
      const status = row.result ? (row.result.ok ? "ok" : "failed") : "running";
      return (
        <div className={styles.tool} data-status={status}>
          <button
            type="button"
            className={styles.toolHead}
            aria-expanded={open}
            onClick={() => setOpen(!open)}
            disabled={!row.result?.preview}
          >
            <span className={styles.toolVerb}>{toolVerb(row.tool)}</span>
            <span className={`${styles.toolSummary} mono`}>{row.summary || row.tool}</span>
            <span className={styles.toolStatus}>{status}</span>
          </button>
          {open && row.result?.preview && <pre className={`${styles.preview} mono`}>{row.result.preview}</pre>}
        </div>
      );
    }
  }
}

function Composer({ session, agentName, canSend }: { session: Session; agentName: string; canSend: boolean }) {
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const waiting = session.state === "WAITING";
  const disabled = !canSend || waiting;

  async function send(event: FormEvent) {
    event.preventDefault();
    // One line (decision 6): a newline would send early in most agents.
    const line = text.replace(/[\r\n]+/g, " ").trim();
    if (!line || disabled) return;
    try {
      // Text, then Enter as its own keypress: sent together, agents such as
      // Claude Code take the burst as a paste and the Enter does not submit.
      await dataAdapter.sendInput(session.id, line);
      await new Promise((resolve) => setTimeout(resolve, ENTER_DELAY_MS));
      await dataAdapter.sendInput(session.id, "\r");
      setText("");
      setError(null);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "Could not send.");
    }
  }

  const placeholder = !canSend
    ? "This capture is read-only"
    : waiting
      ? "Answer the question first"
      : `Message ${agentName}…`;
  return (
    <form className={styles.composer} onSubmit={(event) => void send(event)}>
      <label className={styles.srOnly} htmlFor={`chat-${session.id}`}>
        Message {agentName}
      </label>
      <div className={styles.box}>
        <input
          id={`chat-${session.id}`}
          className={styles.input}
          value={text}
          onChange={(event) => setText(event.target.value)}
          placeholder={placeholder}
          disabled={disabled}
          autoComplete="off"
        />
        <button type="submit" className={styles.send} disabled={disabled || !text.trim()}>
          Send
        </button>
      </div>
      <span className={styles.hint}>
        {error ?? "Types your message and Enter into this Session's terminal, as if you typed it there."}
      </span>
    </form>
  );
}
