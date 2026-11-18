import { useState } from "react";
import { Link, useSearchParams } from "react-router-dom";

import { ConfirmDialog } from "../../../components/ConfirmDialog";
import { AgentStateChip } from "../../../components/AgentStateChip";
import { LearnTip } from "../../../components/LearnTip";
import { useShell } from "../shell/ShellContext";
import { PromptChoices } from "../../../components/PromptChoices";
import { StateIcon } from "../../../components/StateIcon";
import { answerPrompt, canAnswer, offeredChoices } from "../../../data/actions";
import { extractPort, formatSessionFact, needsAttention } from "../../../data/format";
import { clearsAt, clearsInText, isDone, useLifecycle } from "../../../data/lifecycle";
import { useLiveData } from "../../../data/LiveDataContext";
import { isDelayed, machineStatusText } from "../../../data/machine-status";
import type { Machine, MachineMetricSample, Session } from "../../../data/types";
import { onLane, STATUS_WORD, useClock, useMachineLoad } from "../dashboard/GlassDashboard";
import { useSessionRemoval } from "../useSessionRemoval";
import { PageTitle } from "./PageTitle";
import styles from "./PhoneQuiet.module.css";

/**
 * P1 · the phone dashboard in the quiet style: the device line, one big thin
 * number, then ruled lists: Needs you, a list per Machine, and Done (swipe a
 * row for Hide and Delete…). `?needs=1` (the island) and `?machine=` narrow it.
 */
export function PhoneOverview() {
  const [params, setParams] = useSearchParams();
  const shell = useShell();
  const { machines, terminals, hidden, loading } = useLiveData();
  const agents = terminals.filter((session) => !session.dismissed && (session.codingAgent || session.agentState)).length;
  const removal = useSessionRemoval();
  const now = useClock();
  const load = useMachineLoad(machines);
  const machineFilter = params.get("machine");
  const needsOnly = params.get("needs") === "1";
  const scoped = machineFilter ? machines.find((machine) => machine.id === machineFilter) : undefined;
  const visible = machineFilter ? terminals.filter((session) => session.machine === machineFilter) : terminals;
  const needs = visible.filter(needsAttention);
  const lanes = needsOnly ? [] : machines.filter((machine) => !machineFilter || machine.id === machineFilter);
  const online = machines.filter((machine) => machine.status === "ONLINE").length;
  const machineName = (id: string) => machines.find((machine) => machine.id === id)?.name ?? id;
  const time = new Date(now).toLocaleString(undefined, { weekday: "short", hour: "2-digit", minute: "2-digit", hour12: false });
  const scopeTo = (id: string | null) => setParams(id ? { machine: id } : {});

  if (loading) return <div className={styles.page}>Loading…</div>;

  const summary = scoped
    ? `${scoped.name} · ${visible.length} session${visible.length === 1 ? "" : "s"} · ${STATUS_WORD[scoped.status].toLowerCase()}`
    : `${terminals.length} session${terminals.length === 1 ? "" : "s"} · ${online} machine${online === 1 ? "" : "s"}`;

  return (
    <div className={styles.page}>
      <PageTitle>Overview</PageTitle>
      <div className={styles.dateRow}>
        <span>Sentinal</span>
        <span className={styles.muted}>{time}</span>
      </div>

      <nav className={styles.devices} aria-label="Devices">
        <button type="button" className={styles.device} aria-pressed={!machineFilter && !needsOnly} onClick={() => scopeTo(null)}>
          All
        </button>
        {machines.map((machine) => (
          <button
            key={machine.id}
            type="button"
            className={styles.device}
            aria-pressed={machineFilter === machine.id}
            data-online={machine.status === "ONLINE" || undefined}
            onClick={() => scopeTo(machine.id)}
          >
            <span className={styles.deviceDot} aria-hidden="true" />
            {machine.name}
          </button>
        ))}
      </nav>

      <div className={styles.hero}>
        <h2 className={styles.heroTitle}>
          {needs.length > 0 ? (
            <>
              <span className={styles.count}>{needs.length}</span>{" "}
              <span className={styles.words}>
                need{needs.length === 1 ? "s" : ""} <span className={styles.accent}>you.</span>
              </span>
            </>
          ) : (
            <span className={styles.words}>All quiet.</span>
          )}
        </h2>
        <p className={styles.summary}>{summary}</p>
      </div>
      <LearnTip phone onTry={shell ? () => shell.openPrompt() : undefined} />
      {needsOnly && (
        <p className={styles.filter}>
          What needs you ·{" "}
          <button type="button" className={styles.link} onClick={() => setParams({})}>
            Show all
          </button>
        </p>
      )}

      {needs.length > 0 && (
        <section className={styles.section} aria-label="Needs you">
          <h3 className={styles.sectionHead}>Needs you</h3>
          {needs.map((session) =>
            canAnswer(session) ? (
              <article key={session.id} className={styles.row} aria-label={session.name}>
                <StateIcon state={session.state} size={20} />
                <span className={styles.rowBody}>
                  <Link to={`/app/sessions/${encodeURIComponent(session.id)}`} className={styles.rowTitle}>
                    {session.name}
                  </Link>
                  <span className={styles.rowMetaMono}>
                    {session.prompt} · {machineName(session.machine)}
                  </span>
                  {offeredChoices(session).length > 0 && (
                    <span className={styles.choices}>
                      <PromptChoices session={session} />
                    </span>
                  )}
                </span>
                {offeredChoices(session).length === 0 && (
                  <span className={styles.answers}>
                    <button type="button" className={styles.yes} onClick={() => void answerPrompt(session.id, "y")} aria-label={`Answer y to ${session.name}`}>
                      y
                    </button>
                    <button type="button" className={styles.no} onClick={() => void answerPrompt(session.id, "n")} aria-label={`Answer n to ${session.name}`}>
                      n
                    </button>
                  </span>
                )}
              </article>
            ) : (
              <div key={session.id} className={styles.row}>
                <StateIcon state={session.state} size={20} />
                <Link to={`/app/sessions/${encodeURIComponent(session.id)}`} className={styles.rowLink}>
                  <span className={styles.rowTitle}>
                    {session.name} {NEED_WORD[session.state] ?? ""}
                  </span>
                  <span className={styles.rowMeta}>{[machineName(session.machine), shortFact(session)].filter(Boolean).join(" · ")}</span>
                </Link>
                {session.state === "FAILED" ? (
                  <button
                    type="button"
                    className={styles.quiet}
                    aria-label={`Hide ${session.name}`}
                    disabled={removal.busy}
                    onClick={() => void removal.hide([session.id])}
                  >
                    Hide
                  </button>
                ) : (
                  <span className={styles.chevron} aria-hidden="true">
                    ›
                  </span>
                )}
              </div>
            ),
          )}
        </section>
      )}
      {removal.error && (
        <p className={styles.empty} role="alert">
          {removal.error}
        </p>
      )}
      {needsOnly && needs.length === 0 && <p className={styles.empty}>Nothing needs you right now.</p>}

      {lanes.map((machine) => (
        <Lane
          key={machine.id}
          machine={machine}
          sessions={terminals.filter((session) => session.machine === machine.id && onLane(session) && !needsAttention(session))}
          load={load[machine.id]}
        />
      ))}

      {!needsOnly && <PhoneDone sessions={visible} machineName={machineName} now={now} />}

      {!needsOnly && agents > 0 && (
        <Link className={styles.hiddenLink} to="/app/agents">
          {agents} agent{agents === 1 ? "" : "s"} · Task and progress ›
        </Link>
      )}

      {!needsOnly && hidden.length > 0 && (
        <Link className={styles.hiddenLink} to="/app/sessions/hidden">
          {hidden.length} hidden · Show
        </Link>
      )}
    </div>
  );
}

const NEED_WORD: Partial<Record<Session["state"], string>> = { FAILED: "failed", STALLED: "stalled", WAITING: "waiting" };

/** A failure says how it failed; a stall, how long it has been quiet. */
function shortFact(session: Session): string {
  if (session.result) return `${session.result.failed} failed`;
  if (session.state === "STALLED") return session.runningFor ? `quiet ${session.runningFor}` : "quiet";
  if (session.state === "WAITING") return session.prompt ?? "waiting";
  return formatSessionFact(session) === "—" ? "" : formatSessionFact(session);
}

function Lane({ machine, sessions, load }: { machine: Machine; sessions: Session[]; load?: MachineMetricSample }) {
  const online = machine.status === "ONLINE";
  const metric = load?.gpuPct !== undefined ? `GPU ${Math.round(load.gpuPct)} %` : load ? `CPU ${Math.round(load.cpuPct)} %` : "";
  return (
    <section className={styles.section} data-offline={online ? undefined : true} aria-label={machine.name}>
      <div className={styles.sectionHeadRow}>
        <h3 className={styles.sectionHead}>{machine.name}</h3>
        <span className={styles.muted}>
          {!online
            ? `${STATUS_WORD[machine.status].toLowerCase()}${machine.lastSeen ? ` · ${machine.lastSeen}` : ""}`
            : isDelayed(machine)
              ? machineStatusText(machine, Date.now())
              : metric}
        </span>
      </div>
      {sessions.map((session) => {
        const percent = session.progress && session.progress.total > 0 ? Math.round((session.progress.current / session.progress.total) * 100) : null;
        return (
          <Link key={session.id} to={`/app/sessions/${encodeURIComponent(session.id)}`} className={styles.laneRow}>
            {percent !== null ? (
              <span className={styles.percent}>
                {session.progress?.inferred ? "≈" : ""}
                {percent}
                <span className={styles.percentSign}> %</span>
              </span>
            ) : (
              <StateIcon state={session.state} size={18} />
            )}
            <span className={styles.laneBody}>
              <span className={styles.rowTitle}>{session.name}</span>
              {session.agentState && <AgentStateChip session={session} compact />}
              {percent !== null && (
                <span className={styles.bar} aria-hidden="true">
                  <span className={styles.barFill} style={{ width: `${percent}%` }} />
                </span>
              )}
            </span>
            {percent === null && <span className={styles.rowMetaMono}>{laneMeta(session)}</span>}
          </Link>
        );
      })}
      {online && sessions.length === 0 && <p className={styles.empty}>Nothing running.</p>}
    </section>
  );
}

function laneMeta(session: Session): string {
  const port = extractPort(session);
  return port ? `:${port}` : (session.runningFor ?? "");
}

/** Done on the phone: swipe a row left (or focus it) for Hide and Delete…. */
function PhoneDone({ sessions, machineName, now }: { sessions: Session[]; machineName: (id: string) => string; now: number }) {
  const life = useLifecycle();
  const removal = useSessionRemoval();
  const [deleting, setDeleting] = useState<Session | null>(null);
  const done = sessions.filter((session) => isDone(session) && !session.dismissed).sort((a, b) => (b.updatedAt ?? 0) - (a.updatedAt ?? 0));
  if (done.length === 0) return null;
  return (
    <section className={styles.section} aria-label="Done">
      <div className={styles.sectionHeadRow}>
        <h3 className={styles.sectionHead}>
          Done <span className={styles.muted}>{done.length}</span>
        </h3>
        <button type="button" className={styles.link} disabled={removal.busy} onClick={() => void removal.hide(done.map((session) => session.id))}>
          Hide all
        </button>
      </div>
      {done.map((session) => {
        const clears = clearsAt(session, life);
        const fate = life.kept.includes(session.id) ? "kept" : clears !== null ? clearsInText(clears, now) : null;
        const how = session.state === "INTERRUPTED" ? "stopped" : (session.result?.summary ?? formatSessionFact(session));
        return (
          <SwipeRow
            key={session.id}
            name={session.name}
            busy={removal.busy}
            onHide={() => void removal.hide([session.id])}
            onDelete={() => setDeleting(session)}
          >
            <StateIcon state={session.state} size={20} />
            <Link to={`/app/sessions/${encodeURIComponent(session.id)}`} className={styles.rowLink}>
              <span className={styles.rowTitle}>{session.name}</span>
              <span className={styles.rowMeta}>{[machineName(session.machine), how, fate].filter((part) => part && part !== "—").join(" · ")}</span>
            </Link>
          </SwipeRow>
        );
      })}
      {removal.error && (
        <p className={styles.empty} role="alert">
          {removal.error}
        </p>
      )}
      <ConfirmDialog
        open={deleting !== null}
        overline="Delete terminal"
        question={`Permanently delete ${deleting?.name ?? ""}?`}
        consequence="Its saved output, Commands and Events are erased from this Machine. This cannot be undone. Hide keeps it restorable."
        confirmLabel="Delete"
        focusCancel
        onConfirm={() => {
          const target = deleting;
          setDeleting(null);
          if (target) void removal.remove([target.id]);
        }}
        onCancel={() => setDeleting(null)}
      />
    </section>
  );
}

const REVEAL = 148;

/**
 * A row that slides left to show Hide and Delete…. The buttons are always in
 * the page, so keyboard and screen-reader users reach them without swiping;
 * focusing one slides the row open too.
 */
export function SwipeRow({
  name,
  busy,
  onHide,
  onDelete,
  children,
}: {
  name: string;
  busy: boolean;
  onHide: () => void;
  onDelete: () => void;
  children: React.ReactNode;
}) {
  const [offset, setOffset] = useState(0);
  const [start, setStart] = useState<{ x: number; y: number; base: number } | null>(null);
  const open = offset <= -REVEAL / 2;
  return (
    <div className={styles.swipe} data-open={open || undefined}>
      <div className={styles.swipeActions}>
        <button
          type="button"
          className={styles.swipeHide}
          disabled={busy}
          onClick={onHide}
          onFocus={() => setOffset(-REVEAL)}
          aria-label={`Hide ${name}`}
        >
          Hide
        </button>
        <button
          type="button"
          className={styles.swipeDelete}
          disabled={busy}
          onClick={onDelete}
          onFocus={() => setOffset(-REVEAL)}
          aria-label={`Delete ${name}`}
        >
          Delete…
        </button>
      </div>
      <div
        className={styles.swipeFront}
        style={{ transform: `translateX(${offset}px)`, transition: start ? "none" : undefined }}
        onPointerDown={(event) => setStart({ x: event.clientX, y: event.clientY, base: offset })}
        onPointerMove={(event) => {
          if (!start) return;
          const dx = event.clientX - start.x;
          if (Math.abs(dx) < 6 || Math.abs(event.clientY - start.y) > Math.abs(dx)) return;
          setOffset(Math.max(-REVEAL, Math.min(0, start.base + dx)));
        }}
        onPointerUp={() => {
          if (!start) return;
          setStart(null);
          setOffset((value) => (value <= -REVEAL / 2 ? -REVEAL : 0));
        }}
        onPointerCancel={() => {
          setStart(null);
          setOffset((value) => (value <= -REVEAL / 2 ? -REVEAL : 0));
        }}
        onClickCapture={(event) => {
          // A tap on an open row closes it instead of opening the terminal.
          if (offset !== 0) {
            event.preventDefault();
            event.stopPropagation();
            setOffset(0);
          }
        }}
      >
        {children}
      </div>
    </div>
  );
}
