import { useEffect, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";

import { AgentBadge } from "../../components/AgentBadge";
import { CaptureBadge } from "../../components/CaptureBadge";
import { AgentStateChip, SandboxBadge } from "../../components/AgentStateChip";
import { OutputSearchToggle } from "../../components/OutputSearchToggle";
import { MockBadge } from "../../components/MockBadge";
import { StateTag } from "../../components/StateTag";
import { dataAdapter } from "../../data";
import { answerPrompt, endSession, INTERRUPTIBLE } from "../../data/actions";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { InterruptConfirm } from "./InterruptConfirm";
import { lifecycle } from "../../data/lifecycle";
import { useShell } from "./shell/ShellContext";
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
import { SummaryLine } from "./SummaryLine";
import { Suggestions } from "./Suggestions";
import { SessionHistoryTerminals } from "./SessionHistoryTerminals";
import { AgentChatView } from "../../components/AgentChatView";
import { useAgentChat } from "../../data/agentChat";
import { ResumeHistory } from "./ResumeHistory";
import { SessionRemoveActions } from "./SessionRemoveActions";
import { SplitHandle, useSessionSplit } from "./SplitHandle";
import { SessionCommands } from "./SessionCommands";
import { ResourceBars } from "./ResourceBars";
import styles from "./SessionDetailPage.module.css";
import { useIsPhone } from "../../hooks/useViewport";
import { PhoneSessionDetail } from "./phone/PhoneSessionDetail";

/** design_handoff_sentinal/design/SentinalSession.dc.html */
/** What the main section shows for each capture level. */
const SECTION_LABELS: Record<CaptureLevel, string> = {
  FULL: "TERMINAL",
  COMMANDS: "COMMANDS",
  PROCESS_LOG: "LOG",
  PROCESS: "METRICS",
};

const CHAT_VIEW_KEY = "sentinal.session.view";

function readChatView(): "chat" | "terminal" {
  try {
    return localStorage.getItem(CHAT_VIEW_KEY) === "chat" ? "chat" : "terminal";
  } catch {
    return "terminal";
  }
}

export function SessionDetailPage() {
  return useIsPhone() ? <PhoneSessionDetail /> : <DesktopSessionDetail />;
}

function DesktopSessionDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { machines, sessions, loading } = useLiveData();
  const [confirmOpen, setConfirmOpen] = useState(false);
  const shell = useShell();
  const glass = shell !== null;
  // The terminal's share of the page; the divider resizes it (desktop shell only).
  const splitRef = useRef<HTMLDivElement>(null);
  const [split, setSplit] = useSessionSplit();
  const [attached, setAttached] = useState(false);
  const [continuing, setContinuing] = useState(false);
  const [endOpen, setEndOpen] = useState(false);
  const [continueError, setContinueError] = useState<string | null>(null);
  const [jumpTo, setJumpTo] = useState<{ id: string; nonce: number } | undefined>(undefined);
  // AC2: Claude Code Sessions as a chat. Terminal first on desktop (decision 5).
  const chat = useAgentChat(id ?? "");
  const [view, setView] = useState<"chat" | "terminal">(() => readChatView());
  function chooseView(next: "chat" | "terminal") {
    setView(next);
    try {
      localStorage.setItem(CHAT_VIEW_KEY, next);
    } catch {
      // A per-browser convenience only.
    }
  }

  const sessionsRef = useRef(sessions);
  sessionsRef.current = sessions;
  const session = sessions.find((s) => s.id === id);
  const machine = session ? machines.find((m) => m.id === session.machine) : undefined;
  const machineName = machine?.name ?? session?.machine ?? "";

  // Reduced-capture Sessions never attach, so they otherwise never get a fresh
  // authoritative fetch from their owning worker — fields the control
  // projection omits (e.g. cwd) would never arrive without this.
  // Opening a terminal marks it seen and reads its notifications (design L3).
  useEffect(() => {
    if (session) lifecycle.markSeen(session);
  }, [session?.id, session?.state, session?.updatedAt]);

  useEffect(() => {
    if (!session || captureOf(session) === "FULL") return;
    void dataAdapter.refreshSessionDetail(session.id);
  }, [session?.id]);

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

  // End: Ctrl-C, then close the shell (design L2). Also for an idle shell.
  const canEnd = latestCapabilities.canSendInput && !readOnlyLatest(latest.state);

  function askEnd() {
    if (!canEnd) return;
    if (shell?.requestEnd) shell.requestEnd(latest);
    else setEndOpen(true);
  }

  function askCtrlC() {
    if (canInterrupt) setConfirmOpen(true);
  }

  function answer(value: "y" | "n") {
    void answerPrompt(latest.id, value);
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
      // Another part of the same terminal: replace, so no stale sheet is left behind.
      navigate(`/app/sessions/${created.id}`, { replace: true });
    } catch (error) {
      setContinueError(error instanceof Error ? error.message : "Could not resume the terminal.");
      setContinuing(false);
    }
  }

  const sectionLabel = SECTION_LABELS[capture];
  const showChat = chat.linked && view === "chat";

  return (
    <div
      ref={splitRef}
      className={styles.split}
      data-glass={glass || undefined}
      data-full-bleed={glass || undefined}
      style={glass ? { gridTemplateColumns: `${split}% minmax(0, 1fr)` } : undefined}
    >
      <section
        className={styles.terminalColumn}
        aria-label={sectionLabel.charAt(0) + sectionLabel.slice(1).toLowerCase()}
      >
        <div className={styles.terminalHeader}>
          {chat.linked ? (
            <span className={styles.viewSwitch} role="group" aria-label="View">
              <button type="button" aria-pressed={showChat} onClick={() => chooseView("chat")}>
                Chat
              </button>
              <button type="button" aria-pressed={!showChat} onClick={() => chooseView("terminal")}>
                Terminal
              </button>
            </span>
          ) : (
            <span className={styles.terminalLabel}>{sectionLabel}</span>
          )}
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
        {showChat && (
          <AgentChatView
            session={session}
            entries={chat.entries}
            agentName="Claude Code"
            canSend={capabilities.canSendInput && !readOnly}
          />
        )}
        {/* Stays attached while Chat shows: the live chat arrives on it. */}
        <div className={styles.terminalBody} hidden={showChat}>
        {capabilities.canAttach ? (
          <SessionHistoryTerminals
            lineage={lineage}
            jumpTo={jumpTo ?? (session.id !== latest.id ? { id: session.id, nonce: 0 } : undefined)}
            onAttachedChange={setAttached}
            onRequestInterrupt={askCtrlC}
          />
        ) : (
          <div className={styles.reducedBody}>
            <ReducedCapturePanel session={session} showSummary={false} />
          </div>
        )}
        </div>
      </section>

      {glass && <SplitHandle container={splitRef} split={split} onChange={setSplit} />}

      <section className={styles.details} aria-label="Session details" data-island-anchor>
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
              {lineage.ordered.length > 1 && (
                <span className={styles.generationMarker}>
                  Resumed {lineage.ordered.length - 1} {lineage.ordered.length === 2 ? "time" : "times"}
                </span>
              )}
              <StateTag state={session.state} exitUnknown={unknownExit} variant={glass ? "line" : undefined} />
              <CaptureBadge capture={capture} origin={session.origin} />
              <SandboxBadge session={session} />
              {session.codingAgent && <AgentBadge agent={session.codingAgent} usage />}
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
            {session.agentState && (
              <span>
                <AgentStateChip session={session} />
              </span>
            )}
            {capture !== "FULL" && (
              <span className={styles.captureLine} role="note">
                {captureExplanation(capture)}
              </span>
            )}
            {hint && <span className={styles.logHint}>{hint}</span>}
            {capture === "FULL" && <OutputSearchToggle key={session.id} session={session} />}
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
            {canEnd && (
              <button type="button" className={styles.ctrlCButton} onClick={askEnd}>
                End terminal…
              </button>
            )}
            {canContinue && (
              <button
                type="button"
                className={styles.continueButton}
                disabled={continuing}
                onClick={() => void continueInNewSession(latest)}
              >
                {continuing ? "Resuming…" : "Resume terminal"}
              </button>
            )}
            {session.id === latest.id && <SessionRemoveActions session={session} />}
            {session.id !== latest.id && (
              <button
                type="button"
                className={styles.continueButton}
                onClick={() => navigate(`/app/sessions/${latest.id}`, { replace: true })}
              >
                Go to the live terminal
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
        <ResumeHistory
          lineage={lineage}
          onJump={(id) => setJumpTo((current) => ({ id, nonce: (current?.nonce ?? 0) + 1 }))}
        />
        {continueError && <p className={styles.continueError} role="alert">{continueError}</p>}

        {session.summary && <SummaryLine summary={session.summary} />}
        <Suggestions session={session} />
        <SessionTopBlock
          session={session}
          kind={kind}
          interrupted={interrupted}
          lastCheckpoint={lastCheckpointPath(session)}
          canSendInput={capabilities.canSendInput}
          onAnswer={answer}
          onRerun={rerun}
          onShowOutput={() => {
            /* the terminal is always visible in the left half; nothing to scroll to */
          }}
        />
        {capture !== "COMMANDS" && <SessionCommands sessionId={session.id} />}
        {/* Under glass, a training run's GPU and VRAM are already the tiles above. */}
        {capture !== "PROCESS" && !(glass && kind === "training") && <ResourceBars metrics={session.metrics} />}
        {session.health && <HealthCard health={session.health} interrupted={interrupted} />}
        <EventsList events={session.events ?? []} />
      </section>

      <ConfirmDialog
        open={endOpen}
        overline={`END · ${machineName.toUpperCase()}`}
        question={`End ${latest.name}?`}
        consequence="Sends Ctrl-C, then closes the terminal if it is still open. Its output is kept and it moves to Done."
        confirmLabel="End"
        onConfirm={() => {
          setEndOpen(false);
          const id = latest.id;
          void endSession(id, () => !readOnlyLatest(sessionsRef.current.find((entry) => entry.id === id)?.state)).catch(() => undefined);
        }}
        onCancel={() => setEndOpen(false)}
      />
      <InterruptConfirm
        session={confirmOpen ? latest : undefined}
        machineName={machineName}
        onClose={() => setConfirmOpen(false)}
      />
    </div>
  );
}

/** A Session that has ended (or is unknown): nothing left to end. */
function readOnlyLatest(state: string | undefined): boolean {
  return state === undefined || state === "COMPLETED" || state === "FAILED" || state === "INTERRUPTED";
}
