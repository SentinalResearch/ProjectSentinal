import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";

import { CaptureBadge } from "../../components/CaptureBadge";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { MockBadge } from "../../components/MockBadge";
import { StateTag } from "../../components/StateTag";
import { dataAdapter } from "../../data";
import { captureCapabilities, captureExplanation, captureOf, exitUnknown, logHint } from "../../data/capture";
import {
  confirmConsequence,
  formatSessionDuration,
  healthColorSlug,
  lastCheckpointPath,
  sessionKind,
} from "../../data/format";
import { useLiveData } from "../../data/LiveDataContext";
import { TerminalPane } from "../../terminal/TerminalPane";
import { EventsList } from "./EventsList";
import { HealthCard } from "./HealthCard";
import { SessionTopBlock } from "./SessionTopBlock";
import { ResourceBars } from "./ResourceBars";
import styles from "./SessionDetailPage.module.css";

const INTERRUPTIBLE = new Set(["RUNNING", "WAITING", "STALLED"]);

/** design_handoff_sentinal/design/SentinalSession.dc.html */
export function SessionDetailPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { machines, sessions, loading } = useLiveData();
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [attached, setAttached] = useState(false);

  const session = sessions.find((s) => s.id === id);
  const machine = session ? machines.find((m) => m.id === session.machine) : undefined;
  const machineName = machine?.name ?? session?.machine ?? "";

  if (loading) return <div className={styles.page}>Loading…</div>;

  if (!session) {
    return (
      <div className={styles.notFound}>
        <p>Session not found.</p>
        <button type="button" className={styles.breadcrumbMachine} onClick={() => navigate("/app/overview")}>
          Back to Overview
        </button>
      </div>
    );
  }

  const kind = sessionKind(session);
  const interrupted = session.state === "INTERRUPTED";
  const capture = captureOf(session);
  const capabilities = captureCapabilities(capture);
  const unknownExit = exitUnknown(session);
  const hint = logHint(session);
  // Ctrl-C is terminal input: only offered when the daemon allows input.
  const canInterrupt = capabilities.canSendInput && INTERRUPTIBLE.has(session.state);
  const hasHealth = Boolean(session.health);
  const healthLabel = interrupted ? "INTERRUPTED" : (session.health?.overall ?? "");
  const healthSlug = interrupted ? "interrupted" : healthColorSlug(session.health?.overall ?? "");
  const meta = [
    session.project,
    machineName,
    formatSessionDuration(session),
    unknownExit ? "Ended · exit code unknown" : undefined,
  ]
    .filter(Boolean)
    .join(" · ");

  function askCtrlC() {
    if (canInterrupt) setConfirmOpen(true);
  }

  function confirmCtrlC() {
    setConfirmOpen(false);
    if (session) void dataAdapter.sendSignal(session.id, "SIGINT");
  }

  function answerPrompt(value: "y" | "n") {
    if (session) void dataAdapter.sendInput(session.id, value);
  }

  function rerun() {
    // QUESTION: "Run again" (SentinalSession.dc.html) has no corresponding
    // DataAdapter method — sendInput only makes sense for a WAITING prompt, and
    // there's no retry/rerun-command call in the adapter contract. Left as a
    // no-op rather than inventing one; see QUESTIONS.md.
  }

  return (
    <div className={styles.page}>
      <nav className={styles.breadcrumb}>
        <button
          type="button"
          className={styles.breadcrumbMachine}
          onClick={() => navigate(`/app/overview?machine=${session.machine}`)}
        >
          {machineName}
        </button>
        <span>/</span>
        <span className={styles.breadcrumbName}>{session.name}</span>
      </nav>

      <header className={styles.header}>
        <div className={styles.headerText}>
          <div className={styles.titleRow}>
            <h1 className={styles.title}>{session.name}</h1>
            <StateTag state={session.state} exitUnknown={unknownExit} />
            <CaptureBadge capture={capture} />
            {hasHealth && (
              <span
                className={styles.healthOutline}
                style={{ boxShadow: `inset 0 0 0 1.5px var(--s-${healthSlug}-fg)`, color: `var(--s-${healthSlug}-fg)` }}
              >
                {healthLabel}
              </span>
            )}
            <MockBadge />
          </div>
          {session.command && <span className={styles.command}>{session.command}</span>}
          <span className={styles.meta}>{meta}</span>
          {hint && <span className={styles.logHint}>{hint}</span>}
        </div>
        <div className={styles.actions}>
          <button
            type="button"
            className={styles.splitButton}
            onClick={() => navigate(`/app/split?focus=${session.id}`)}
          >
            Open in split view
          </button>
          {canInterrupt && (
            <button type="button" className={styles.ctrlCButton} onClick={askCtrlC}>
              Ctrl‑C…
            </button>
          )}
        </div>
      </header>

      <SessionTopBlock
        session={session}
        kind={kind}
        interrupted={interrupted}
        lastCheckpoint={lastCheckpointPath(session)}
        onAnswer={answerPrompt}
        onRerun={rerun}
        onShowOutput={() => {
          /* the terminal is always visible below; nothing to scroll to on this layout */
        }}
      />
      <ResourceBars metrics={session.metrics} />

      <div className={styles.body}>
        <section className={styles.terminalSection}>
          <div className={styles.terminalHeader}>
            <span className={styles.terminalLabel}>TERMINAL</span>
            <span className={`${styles.terminalPath} mono`}>
              {machineName} · {session.cwd ?? "~"}
            </span>
            {capabilities.canAttach && (
              <span className={styles.attachedIndicator}>
                <span
                  className={styles.attachedDot}
                  style={{ background: attached ? "var(--s-running)" : "var(--s-disconnected-fg)" }}
                />
                {attached ? "attached" : "connecting…"}
              </span>
            )}
          </div>
          {capabilities.canAttach ? (
            <TerminalPane
              sessionId={session.id}
              session={session}
              onAttachedChange={setAttached}
              onRequestInterrupt={askCtrlC}
            />
          ) : (
            <p className={styles.terminalUnavailable} role="note">
              {captureExplanation(capture)}
            </p>
          )}
        </section>

        <aside className={styles.side}>
          {session.health && <HealthCard health={session.health} interrupted={interrupted} />}
          <EventsList events={session.events ?? []} />
        </aside>
      </div>

      <ConfirmDialog
        open={confirmOpen}
        overline={`INTERRUPT · ${machineName.toUpperCase()}`}
        question={`Send Ctrl‑C to ${session.name}?`}
        consequence={confirmConsequence(session, machineName)}
        confirmLabel="Send Ctrl‑C"
        onConfirm={confirmCtrlC}
        onCancel={() => setConfirmOpen(false)}
      />
    </div>
  );
}
