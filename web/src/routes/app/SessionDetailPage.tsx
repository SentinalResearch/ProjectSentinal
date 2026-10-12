import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";

import { CaptureBadge } from "../../components/CaptureBadge";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { MockBadge } from "../../components/MockBadge";
import { StateTag } from "../../components/StateTag";
import { dataAdapter } from "../../data";
import { resolveSessionLineage } from "../../data/lineage";
import type { Session } from "../../data/types";
import { captureCapabilities, captureExplanation, captureOf, exitUnknown, logHint } from "../../data/capture";
import { ReducedCapturePanel } from "./ReducedCapturePanel";
import type { CaptureLevel } from "../../protocol/CaptureLevel";
import {
  confirmConsequence,
  formatSessionDuration,
  healthColorSlug,
  lastCheckpointPath,
  sessionKind,
} from "../../data/format";
import { useLiveData } from "../../data/LiveDataContext";
import { isReadOnlyTerminalState } from "../../terminal/lifecycle";
import { EventsList } from "./EventsList";
import { HealthCard } from "./HealthCard";
import { SessionTopBlock } from "./SessionTopBlock";
import { SessionHistoryTerminals } from "./SessionHistoryTerminals";
import { SessionCommands } from "./SessionCommands";
import { ResourceBars } from "./ResourceBars";
import styles from "./SessionDetailPage.module.css";
import { useIsPhone } from "../../hooks/useViewport";
import { PhoneSessionDetail } from "./phone/PhoneSessionDetail";

const INTERRUPTIBLE = new Set(["RUNNING", "WAITING", "STALLED"]);
/** design_handoff_sentinal/design/SentinalSession.dc.html */
/** What the main section shows for each capture level. */
const SECTION_LABELS: Record<CaptureLevel, string> = {
  FULL: "TERMINAL",
  COMMANDS: "COMMANDS",
  PROCESS_LOG: "LOG",
  PROCESS: "METRICS",
};

export function SessionDetailPage() {
  return useIsPhone() ? <PhoneSessionDetail /> : <DesktopSessionDetail />;
}

function DesktopSessionDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { machines, sessions, loading } = useLiveData();
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [attached, setAttached] = useState(false);
  const [continuing, setContinuing] = useState(false);
  const [continueError, setContinueError] = useState<string | null>(null);

  const session = sessions.find((s) => s.id === id);
  const machine = session ? machines.find((m) => m.id === session.machine) : undefined;
  const machineName = machine?.name ?? session?.machine ?? "";

  if (loading) return <div className={styles.page}>Loading…</div>;

  if (!session) {
    const separator = id?.lastIndexOf(":") ?? -1;
    const requestedMachineId = separator > 0 ? id?.slice(0, separator) : undefined;
    const requestedSessionId = separator > 0 ? id?.slice(separator + 1) : id;
    const requestedMachine = machines.find((candidate) => candidate.id === requestedMachineId);
    const unavailable = requestedMachine && requestedMachine.status !== "ONLINE";
    return (
      <div className={styles.notFound}>
        <h1>Session {requestedSessionId ?? "unknown"}</h1>
        <p>
          {requestedMachine
            ? `${requestedMachine.name} · ${unavailable ? `Machine ${requestedMachine.status.toLowerCase()}` : "Session missing"}`
            : `Machine ${requestedMachineId ?? "unknown"} is not enrolled or is no longer available.`}
        </p>
        <p>The notification still points to this exact Machine and will not open a same-numbered Session elsewhere.</p>
        <button type="button" className={styles.breadcrumbMachine} onClick={() => navigate("/app/overview")}>
          Back to Overview
        </button>
      </div>
    );
  }

  let lineage;
  let lineageError: string | null = null;
  try {
    lineage = resolveSessionLineage(sessions, session.id);
  } catch (error) {
    lineageError = error instanceof Error ? error.message : "Session history is invalid.";
    lineage = resolveSessionLineage([session], session.id);
  }
  const latest = lineage.latest;
  const root = lineage.ordered[0];

  const kind = sessionKind(session);
  const interrupted = session.state === "INTERRUPTED";
  const capture = captureOf(session);
  const capabilities = captureCapabilities(capture);
  const unknownExit = exitUnknown(session);
  const hint = logHint(session);
  const readOnly = isReadOnlyTerminalState(session.state);
  const canContinue = session.id === latest.id && readOnly && capture === "FULL";
  // Ctrl-C is terminal input: only offered when the daemon allows input.
  const latestCapabilities = captureCapabilities(captureOf(latest));
  const canInterrupt = latestCapabilities.canSendInput && INTERRUPTIBLE.has(latest.state);
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
    void dataAdapter.sendSignal(latest.id, "SIGINT");
  }

  function answerPrompt(value: "y" | "n") {
    void dataAdapter.sendInput(latest.id, value);
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
      const created = await dataAdapter.continueSession(source.id);
      setContinuing(false);
      navigate(`/app/sessions/${created.id}`);
    } catch (error) {
      setContinueError(error instanceof Error ? error.message : "Could not create a new Session.");
      setContinuing(false);
    }
  }

  const sectionLabel = SECTION_LABELS[capture];

  return (
    <div className={styles.split}>
      <section
        className={styles.terminalColumn}
        aria-label={sectionLabel.charAt(0) + sectionLabel.slice(1).toLowerCase()}
      >
        <div className={styles.terminalHeader}>
          <span className={styles.terminalLabel}>{sectionLabel}</span>
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
          <SessionHistoryTerminals
            lineage={lineage}
            selectedId={session.id}
            machineName={machineName}
            onSelect={(sessionId) => navigate(`/app/sessions/${sessionId}`)}
            onAttachedChange={setAttached}
            onRequestInterrupt={askCtrlC}
          />
        ) : (
          <div className={styles.reducedBody}>
            <ReducedCapturePanel session={session} showSummary={false} />
          </div>
        )}
      </section>

      <section className={styles.details} aria-label="Session details">
        <nav className={styles.breadcrumb} aria-label="Breadcrumb">
          <button
            type="button"
            className={styles.breadcrumbMachine}
            onClick={() => navigate(`/app/overview?machine=${session.machine}`)}
          >
            {machineName}
          </button>
          <span>/</span>
          <span className={styles.breadcrumbName}>{root.name}</span>
        </nav>

        <header className={styles.header}>
          <div className={styles.headerText}>
            <div className={styles.titleRow}>
              <h1 className={styles.title}>{root.name}</h1>
              <span className={styles.generationMarker}>
                Generation {lineage.selectedIndex + 1} of {lineage.ordered.length}
              </span>
              <StateTag state={session.state} exitUnknown={unknownExit} />
              <CaptureBadge capture={capture} origin={session.origin} />
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
            {capture !== "FULL" && (
              <span className={styles.captureLine} role="note">
                {captureExplanation(capture)}
              </span>
            )}
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
                onClick={() => void continueInNewSession(latest)}
              >
                {continuing ? "Creating Session…" : "Continue in new Session"}
              </button>
            )}
            {session.id !== latest.id && (
              <button
                type="button"
                className={styles.continueButton}
                onClick={() => navigate(`/app/sessions/${latest.id}`)}
              >
                View latest Session
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
        {lineageError && <p className={styles.continueError} role="alert">{lineageError}</p>}
        {continueError && <p className={styles.continueError} role="alert">{continueError}</p>}

        <SessionTopBlock
          session={session}
          kind={kind}
          interrupted={interrupted}
          lastCheckpoint={lastCheckpointPath(session)}
          canSendInput={capabilities.canSendInput}
          onAnswer={answerPrompt}
          onRerun={rerun}
          onShowOutput={() => {
            /* the terminal is always visible in the left half; nothing to scroll to */
          }}
        />
        {capture !== "COMMANDS" && <SessionCommands sessionId={session.id} />}
        {capture !== "PROCESS" && <ResourceBars metrics={session.metrics} />}
        {session.health && <HealthCard health={session.health} interrupted={interrupted} />}
        <EventsList events={session.events ?? []} />
      </section>

      <ConfirmDialog
        open={confirmOpen}
        overline={`INTERRUPT · ${machineName.toUpperCase()}`}
        question={`Send Ctrl‑C to ${latest.name}?`}
        consequence={confirmConsequence(latest, machineName)}
        confirmLabel="Send Ctrl‑C"
        onConfirm={confirmCtrlC}
        onCancel={() => setConfirmOpen(false)}
      />
    </div>
  );
}
