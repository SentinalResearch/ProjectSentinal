import { useCallback, useState } from "react";
import { useLocation, useNavigate, useParams } from "react-router-dom";

import { CaptureBadge } from "../../components/CaptureBadge";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { MockBadge } from "../../components/MockBadge";
import { StateTag } from "../../components/StateTag";
import { dataAdapter } from "../../data";
import type { Session } from "../../data/types";
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
import { isReadOnlyTerminalState } from "../../terminal/lifecycle";
import { EventsList } from "./EventsList";
import { HealthCard } from "./HealthCard";
import { SessionTopBlock } from "./SessionTopBlock";
import { ResourceBars } from "./ResourceBars";
import styles from "./SessionDetailPage.module.css";

const INTERRUPTIBLE = new Set(["RUNNING", "WAITING", "STALLED"]);
const MAX_TITLE_BYTES = 256;

function continuedTitle(name: string): string {
  const suffix = " (continued)";
  const encoder = new TextEncoder();
  let base = name;
  while (base.length > 0 && encoder.encode(`${base}${suffix}`).length > MAX_TITLE_BYTES) {
    base = Array.from(base).slice(0, -1).join("");
  }
  return `${base || "Session"}${suffix}`;
}

/** design_handoff_sentinal/design/SentinalSession.dc.html */
export function SessionDetailPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const location = useLocation();
  const { machines, sessions, loading } = useLiveData();
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [attached, setAttached] = useState(false);
  const [dimensions, setDimensions] = useState<{ cols: number; rows: number }>();
  const [continuing, setContinuing] = useState(false);
  const [continueError, setContinueError] = useState<string | null>(null);
  const rememberDimensions = useCallback((cols: number, rows: number) => {
    setDimensions((current) =>
      current?.cols === cols && current.rows === rows ? current : { cols, rows },
    );
  }, []);

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
  const readOnly = isReadOnlyTerminalState(session.state);
  const canContinue = readOnly && capture === "FULL";
  const continuation = location.state as { continuedFrom?: string } | null;
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

  async function continueInNewSession(source: Session) {
    if (!canContinue || continuing) return;
    setContinuing(true);
    setContinueError(null);
    try {
      const created = await dataAdapter.createSession({
        shell: source.shell,
        cwd: source.cwd,
        title: continuedTitle(source.name),
        cols: dimensions?.cols,
        rows: dimensions?.rows,
      });
      setContinuing(false);
      navigate(`/app/sessions/${created.id}`, { state: { continuedFrom: source.id } });
    } catch (error) {
      setContinueError(error instanceof Error ? error.message : "Could not create a new Session.");
      setContinuing(false);
    }
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
          {canContinue && (
            <button
              type="button"
              className={styles.continueButton}
              disabled={continuing}
              onClick={() => void continueInNewSession(session)}
            >
              {continuing ? "Creating Session…" : "Continue in new Session"}
            </button>
          )}
        </div>
      </header>

      {canContinue && (
        <div className={styles.endedBanner} role="status">
          <strong>
            Session ended — output saved · {" "}
            {session.state}
            {session.exitCode !== null && session.exitCode !== undefined
              ? ` · exit code ${session.exitCode}`
              : ""}
            {" · saved with this Session; no server path is exposed"}
          </strong>
        </div>
      )}
      {continuation?.continuedFrom && (
        <div className={styles.newSessionBanner} role="status">
          New Session ID: {session.id} · continued from Session {continuation.continuedFrom}
        </div>
      )}
      {continueError && <p className={styles.continueError} role="alert">{continueError}</p>}

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
            {capabilities.canAttach && !readOnly && (
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
              onDimensionsChange={rememberDimensions}
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
