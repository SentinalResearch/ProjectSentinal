import { useEffect, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";

import { CaptureBadge } from "../../../components/CaptureBadge";
import { StateTag } from "../../../components/StateTag";
import { dataAdapter } from "../../../data";
import { captureCapabilities, captureExplanation, captureOf, exitUnknown, logHint } from "../../../data/capture";
import { formatSessionDuration, healthColorSlug, lossOf } from "../../../data/format";
import { useLiveData } from "../../../data/LiveDataContext";
import type { Machine, Session } from "../../../data/types";
import { TerminalPane } from "../../../terminal/TerminalPane";
import { EventsList } from "../EventsList";
import { ReducedCapturePanel } from "../ReducedCapturePanel";
import { BackBar } from "./BackBar";
import { ConfirmSignalSheet } from "./ConfirmSignalSheet";
import { answer } from "./NeedsYouCard";
import { needsInterruptConfirm } from "./interrupt";
import shell from "./phone.module.css";
import styles from "./phoneSession.module.css";

const INTERRUPTIBLE = new Set(["RUNNING", "WAITING", "STALLED"]);

function ago(timestamp: number | undefined): string {
  if (timestamp === undefined) return "—";
  const seconds = Math.max(0, Math.round((Date.now() - timestamp) / 1000));
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  return minutes < 60 ? `${minutes}m ago` : `${Math.floor(minutes / 60)}h ago`;
}

/** B3/B4/B5 on phones: the screen follows the Session's state, as a tapped push expects. */
export function PhoneSessionDetail() {
  const { id = "" } = useParams<{ id: string }>();
  const { machines, sessions, loading } = useLiveData();
  const session = sessions.find((candidate) => candidate.id === id);

  if (loading) return <div className={shell.page}>Loading…</div>;
  if (!session) return <MissingSession id={id} machines={machines} />;

  const machine = machines.find((candidate) => candidate.id === session.machine);
  return <SessionScreen session={session} machineName={machine?.name ?? session.machine} />;
}

function MissingSession({ id, machines }: { id: string; machines: Machine[] }) {
  const separator = id.lastIndexOf(":");
  const machineId = separator > 0 ? id.slice(0, separator) : undefined;
  const sessionNumber = separator > 0 ? id.slice(separator + 1) : id;
  const machine = machines.find((candidate) => candidate.id === machineId);
  const status = machine && machine.status !== "ONLINE" ? `Machine ${machine.status.toLowerCase()}` : "Session missing";
  return (
    <>
      <BackBar parentLabel="Overview" parentPath="/app/overview" />
      <div className={shell.page}>
        <h1 className={styles.detailTitle}>Session {sessionNumber}</h1>
        <p>{machine ? `${machine.name} · ${status}` : `Machine ${machineId ?? "unknown"} is not enrolled or is no longer available.`}</p>
        <p className={styles.muted}>This link points to this exact Machine and never opens a same-numbered Session elsewhere.</p>
      </div>
    </>
  );
}

function SessionScreen({ session, machineName }: { session: Session; machineName: string }) {
  const [sheetOpen, setSheetOpen] = useState(false);
  const capture = captureOf(session);
  const capabilities = captureCapabilities(capture);
  const canInterrupt = capabilities.canSendInput && INTERRUPTIBLE.has(session.state);
  const waiting = session.state === "WAITING" && Boolean(session.prompt);
  const failed = session.state === "FAILED";

  function stop() {
    if (needsInterruptConfirm(session)) setSheetOpen(true);
    else void dataAdapter.sendSignal(session.id, "SIGINT");
  }

  return (
    <div className={styles.detail}>
      <BackBar parentLabel="Overview" parentPath="/app/overview" />
      {failed && <FailureBand session={session} machineName={machineName} />}
      <div className={shell.page}>
        <header className={styles.detailHeader}>
          <span className={styles.overline}>
            {machineName}
            {session.project ? ` / ${session.project}` : ""}
          </span>
          <h1 className={styles.detailTitle}>{session.name}</h1>
          <div className={styles.badges}>
            <StateTag state={session.state} exitUnknown={exitUnknown(session)} />
            <CaptureBadge capture={capture} origin={session.origin} />
            {session.health && (
              <span className={styles.healthBadge} style={{ color: `var(--s-${healthColorSlug(session.health.overall)}-fg)` }}>
                {session.health.overall}
              </span>
            )}
            <span className="mono">{formatSessionDuration(session)}</span>
          </div>
          {session.command && <code className={`${styles.command} mono`}>{session.command}</code>}
          {capture !== "FULL" && (
            <p className={styles.muted} role="note">
              {captureExplanation(capture)}
            </p>
          )}
          {logHint(session) && <p className={styles.muted}>{logHint(session)}</p>}
        </header>

        {waiting ? (
          <WaitingPanel session={session} canAnswer={capabilities.canSendInput} />
        ) : failed ? (
          <FailureDetail session={session} />
        ) : (
          <ProgressAndMetrics session={session} />
        )}

        {capture !== "FULL" && <ReducedCapturePanel session={session} showSummary={false} />}

        {session.health?.evidence && session.health.evidence.length > 0 && (
          <section aria-label="Health evidence" className={styles.evidence}>
            <span className={styles.overline}>HEALTH · EVIDENCE</span>
            <p>{session.health.evidence.slice(-4).join(" · ")}</p>
          </section>
        )}
        <EventsList events={session.events ?? []} />
      </div>

      <div className={styles.stickyActions}>
        {capabilities.canAttach ? (
          <Link className={styles.primaryButton} to={`/app/sessions/${session.id}/terminal`}>
            Open terminal
          </Link>
        ) : (
          <span className={styles.muted}>No terminal for this capture</span>
        )}
        {canInterrupt && (
          <button type="button" className={styles.stopButton} onClick={stop}>
            Stop
          </button>
        )}
      </div>

      <ConfirmSignalSheet
        open={sheetOpen}
        session={session}
        machineName={machineName}
        onClose={() => setSheetOpen(false)}
        onConfirm={() => {
          setSheetOpen(false);
          void dataAdapter.sendSignal(session.id, "SIGINT");
        }}
      />
    </div>
  );
}

function ProgressAndMetrics({ session }: { session: Session }) {
  const progress = session.progress;
  const percent = progress && progress.total > 0 ? Math.round((progress.current / progress.total) * 100) : undefined;
  const loss = lossOf(session);
  const metrics = session.metrics;
  const cells = [
    { label: "LOSS", value: loss ? String(loss.to) : "—", foot: loss?.from !== undefined ? `from ${loss.from}` : "" },
    { label: "GPU", value: metrics?.gpuPct !== undefined ? `${metrics.gpuPct} %` : "—", foot: "" },
    {
      label: "VRAM",
      value: metrics?.vramGB !== undefined ? `${metrics.vramGB}${metrics.vramTotalGB ? ` / ${metrics.vramTotalGB}` : ""} GB` : "—",
      foot: "",
    },
    { label: "LAST ACTIVITY", value: ago(session.updatedAt), foot: "" },
  ];
  return (
    <>
      {percent !== undefined && progress && (
        <section aria-label="Progress" className={styles.progress}>
          <span className={styles.bigNumber}>{percent} %</span>
          <span className="mono">
            {progress.unit} {progress.current}/{progress.total}
          </span>
          <div className={styles.track}>
            <div className={styles.fill} style={{ width: `${percent}%` }} />
          </div>
        </section>
      )}
      <ul className={styles.metricGrid} aria-label="Metrics">
        {cells.map((cell) => (
          <li key={cell.label} className={styles.metricCell}>
            <span className={styles.overline}>{cell.label}</span>
            <span className={`${styles.metricValue} mono`}>{cell.value}</span>
            {cell.foot && <span className={styles.muted}>{cell.foot}</span>}
          </li>
        ))}
      </ul>
    </>
  );
}

function WaitingPanel({ session, canAnswer }: { session: Session; canAnswer: boolean }) {
  return (
    <section aria-label="Waiting for input" className={styles.waiting}>
      <span className={styles.waitingOverline}>? WAITING FOR INPUT</span>
      <div className={styles.promptBlock}>
        <span className={styles.overline}>PROMPT</span>
        <span className={`${styles.bigPrompt} mono`}>{session.prompt}</span>
      </div>
      {canAnswer ? (
        <>
          <div className={styles.bigAnswers}>
            <button type="button" className={styles.bigAnswer} onClick={() => answer(session.id, "y")}>y</button>
            <button type="button" className={styles.bigAnswer} onClick={() => answer(session.id, "n")}>n</button>
          </div>
          <p className={styles.muted}>Sends the key and Enter to the session. Anything else needs the terminal.</p>
        </>
      ) : (
        <p className={styles.muted}>This capture is read-only; answer it where the program runs.</p>
      )}
    </section>
  );
}

function FailureBand({ session, machineName }: { session: Session; machineName: string }) {
  return (
    <div className={styles.failureBand} role="status">
      <strong>✕ FAILED · {machineName.toUpperCase()}</strong>
      <span>{session.result?.summary ?? (session.exitCode != null ? `Exited with code ${session.exitCode}` : "The run failed.")}</span>
    </div>
  );
}

function FailureDetail({ session }: { session: Session }) {
  const navigate = useNavigate();
  const [previous, setPrevious] = useState<string | null>(null);
  const result = session.result;
  const capture = captureOf(session);

  useEffect(() => {
    if (!session.command) return;
    let cancelled = false;
    dataAdapter
      .getHistory({ machine: session.machine, query: session.command, result: "ok" })
      .then((history) => {
        const match = history.find(
          (entry) => entry.ok === true && entry.command === session.command && entry.sessionId && entry.sessionId !== session.id,
        );
        if (!cancelled) setPrevious(match?.sessionId ?? null);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [session.command, session.id, session.machine]);

  return (
    <>
      {result && (
        <ul className={styles.statGrid} aria-label="Result">
          <li className={styles.metricCell}>
            <span className={styles.overline}>FAILED</span>
            <span className={`${styles.metricValue} mono`}>{result.failed}</span>
          </li>
          <li className={styles.metricCell}>
            <span className={styles.overline}>PASSED</span>
            <span className={`${styles.metricValue} mono`}>{result.passed}</span>
          </li>
          <li className={styles.metricCell}>
            <span className={styles.overline}>TIME</span>
            <span className={`${styles.metricValue} mono`}>{result.duration}</span>
          </li>
        </ul>
      )}
      {capture === "FULL" && (
        <section aria-label="Last output" className={styles.lastOutput}>
          <span className={styles.overline}>LAST OUTPUT</span>
          <TerminalPane sessionId={session.id} session={session} height={180} showKeyBar={false} forceReadOnly />
        </section>
      )}
      {previous && (
        <button type="button" className={styles.outlineButton} onClick={() => navigate(`/app/sessions/${previous}`)}>
          See how it passed last time
        </button>
      )}
    </>
  );
}
