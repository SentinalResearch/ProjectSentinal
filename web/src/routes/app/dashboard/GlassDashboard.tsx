import { useEffect, useId, useMemo, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";

import { AgentBadge } from "../../../components/AgentBadge";
import { PromptChoices } from "../../../components/PromptChoices";
import { StateIcon } from "../../../components/StateIcon";
import { dataAdapter } from "../../../data";
import { answerPrompt, canAnswer, canInterrupt, noAnswerReason, offeredChoices } from "../../../data/actions";
import { extractPort, formatSessionFact, machineSummary, needsAttention } from "../../../data/format";
import { useLiveData } from "../../../data/LiveDataContext";
import type { Machine, MachineMetricSample, Session } from "../../../data/types";
import { InterruptConfirm } from "../InterruptConfirm";
import { MachinePanel } from "../MachinePanel";
import { useShell } from "../shell/ShellContext";
import { useSessionRemoval } from "../useSessionRemoval";
import { DoneSection } from "./DoneSection";
import { awayFeed } from "./feed";
import styles from "./GlassDashboard.module.css";

const LOAD_MS = 30_000;

/**
 * The desktop dashboard (≥ 1024 px), in the quiet "Option A" style: one big
 * number for what needs you, the devices as one line of text that scopes the
 * page to a Machine, then plain ruled lists. Search lives in the island.
 */
export function GlassDashboard() {
  const navigate = useNavigate();
  const shell = useShell();
  const [searchParams, setSearchParams] = useSearchParams();
  const { machines, terminals, hidden, loading } = useLiveData();
  const [interrupting, setInterrupting] = useState<Session | null>(null);
  // 6.1: show only Sessions running an AI coding agent. A Session counts as an
  // agent when its Command started one or its screen showed a menu with
  // options (an agent's prompt or banner), not by command name alone (P6).
  const [agentsOnly, setAgentsOnly] = useState(false);
  const isAgentSession = (session: Session) =>
    session.codingAgent !== undefined ||
    (session.promptOptions !== undefined && session.promptOptions.length > 0);
  const shown = (session: Session) => !agentsOnly || isAgentSession(session);
  const now = useClock();
  const load = useMachineLoad(machines);

  // The device line scopes the whole page to one Machine (`?machine=`).
  const scopeId = searchParams.get("machine");
  const scoped = scopeId ? machines.find((machine) => machine.id === scopeId) : undefined;
  const inScope = (session: Session) => !scoped || session.machine === scoped.id;
  const scopeTo = (id: string | null) => setSearchParams(id ? { machine: id } : {});

  const scopedSessions = terminals.filter(inScope);
  const needs = scopedSessions.filter((session) => needsAttention(session) && shown(session));
  const laneMachines = scoped ? [scoped] : machines;
  const online = machines.filter((machine) => machine.status === "ONLINE").length;
  const feed = useMemo(
    () => awayFeed(terminals.filter(inScope), laneMachines),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [terminals, machines, scoped?.id],
  );
  const since = feed.filter((item) => item.time).at(-1)?.time.slice(0, 5);
  const machineName = (id: string) => machines.find((machine) => machine.id === id)?.name ?? id;
  const open = (id: string) => navigate(`/app/sessions/${encodeURIComponent(id)}`);

  const date = new Date(now).toLocaleString(undefined, { weekday: "short", day: "numeric", month: "short", hour: "2-digit", minute: "2-digit", hour12: false });
  const summary = scoped
    ? `${scopedSessions.length} session${scopedSessions.length === 1 ? "" : "s"} on ${scoped.name}. ${STATUS_WORD[scoped.status]}.`
    : `${terminals.length} session${terminals.length === 1 ? "" : "s"} on ${online} machine${online === 1 ? "" : "s"}.${needs.length > 0 ? " The rest are fine." : ""}`;

  return (
    <div className={styles.page}>
      <div className={styles.side}>
        <div className={styles.dateRow}>
          <span>Sentinal</span>
          <span className={styles.muted}>{date}</span>
        </div>

        <nav className={styles.devices} aria-label="Devices">
          <button type="button" className={styles.device} aria-pressed={!scoped} onClick={() => scopeTo(null)}>
            All
          </button>
          {machines.map((machine) => (
            <button
              key={machine.id}
              type="button"
              className={styles.device}
              aria-pressed={scoped?.id === machine.id}
              data-online={machine.status === "ONLINE" || undefined}
              onClick={() => scopeTo(machine.id)}
              title={`${machine.name} · ${STATUS_WORD[machine.status]}`}
            >
              <span className={styles.deviceDot} aria-hidden="true" />
              {machine.name}
              <span className={styles.visuallyHidden}> · {STATUS_WORD[machine.status]}</span>
            </button>
          ))}
        </nav>

        <header className={styles.hero}>
          <h1 className={styles.heroTitle}>
            {loading ? (
              "Loading…"
            ) : needs.length > 0 ? (
              <>
                <span className={styles.count}>{needs.length}</span>{" "}
                <span className={styles.words}>
                  need{needs.length === 1 ? "s" : ""} <span className={styles.accent}>you.</span>
                </span>
              </>
            ) : (
              <span className={styles.quiet}>All quiet.</span>
            )}
          </h1>
          <p className={styles.summary}>{summary}</p>
        </header>

      </div>

      <div className={styles.main}>
        <section className={styles.section} aria-labelledby="needs-you-heading">
          <div className={styles.sectionHead}>
            <h2 id="needs-you-heading" className={styles.sectionTitle}>
              Needs you
            </h2>
            <span className={styles.sectionTools}>
              {hidden.length > 0 && (
                <button type="button" className={styles.link} onClick={() => navigate("/app/sessions/hidden")}>
                  Hidden ({hidden.length})
                </button>
              )}
              {/* A view filter for the whole page. */}
              <button
                type="button"
                className={styles.pill}
                aria-pressed={agentsOnly}
                aria-label="AI agents only"
                title="Show only Sessions running an AI coding agent: the Command started one, or the screen showed its prompt or menu"
                onClick={() => setAgentsOnly((only) => !only)}
              >
                AI agents
              </button>
            </span>
          </div>
          {needs.length === 0 && !loading && <p className={styles.empty}>Nothing needs you{scoped ? ` on ${scoped.name}` : ""}.</p>}
          {needs.map((session) => (
            <NeedsRow
              key={session.id}
              session={session}
              machineName={machineName(session.machine)}
              onOpen={() => open(session.id)}
              // Under the shell the island asks; elsewhere the dialog does.
              onInterrupt={() => (shell?.requestInterrupt ? shell.requestInterrupt(session) : setInterrupting(session))}
            />
          ))}
        </section>

        {laneMachines.map((machine) => (
          <MachineLane
            key={machine.id}
            machine={machine}
            sessions={terminals.filter((session) => session.machine === machine.id && onLane(session) && shown(session))}
            emptyNote={agentsOnly ? "No AI agents." : "No Sessions."}
            load={load[machine.id]}
            onOpen={open}
            onScope={scoped ? undefined : () => scopeTo(machine.id)}
          />
        ))}

        <DoneSection sessions={scopedSessions} machineName={machineName} now={now} onOpen={open} />

        {scoped && <MachinePanel machine={scoped} sessions={scopedSessions} />}

        <aside className={styles.section} aria-label="While you were away">
          <div className={styles.sectionHead}>
            <h2 className={styles.sectionTitle}>While you were away</h2>
            {since && <span className={styles.muted}>since {since}</span>}
          </div>
          {feed.length === 0 && <p className={styles.empty}>Nothing new.</p>}
          <ol className={styles.feedList}>
            {feed.map((item) => (
              <li key={item.id}>
                <button type="button" className={styles.feedItem} disabled={!item.sessionId} onClick={() => item.sessionId && open(item.sessionId)}>
                  <span className={styles.feedTime}>{item.time.slice(0, 5)}</span>
                  <span className={styles.feedSquare} style={{ background: item.square }} aria-hidden="true" />
                  <span className={styles.feedText}>
                    <span className={styles.feedHeadline}>{item.headline}</span>
                    <span className={styles.feedDetail}>{item.detail}</span>
                  </span>
                </button>
              </li>
            ))}
          </ol>
        </aside>
      </div>

      <InterruptConfirm
        session={interrupting ?? undefined}
        machineName={interrupting ? machineName(interrupting.machine) : ""}
        onClose={() => setInterrupting(null)}
      />
    </div>
  );
}

/** How the dashboard names a Machine's connection. */
export const STATUS_WORD: Record<Machine["status"], string> = {
  ONLINE: "Online",
  OFFLINE: "Offline",
  DISCONNECTED: "Disconnected",
  AUTH_EXPIRED: "Authentication expired",
  REVOKED: "Revoked",
};

/** Lanes show live work and failures; finished runs live in History. */
export function onLane(session: Session): boolean {
  return session.state !== "COMPLETED" && session.state !== "INTERRUPTED";
}

function NeedsRow({
  session,
  machineName,
  onOpen,
  onInterrupt,
}: {
  session: Session;
  machineName: string;
  onOpen: () => void;
  onInterrupt: () => void;
}) {
  const navigate = useNavigate();
  const titleId = `need-${useId()}`;
  const removal = useSessionRemoval();
  const fact = session.state === "FAILED" ? (session.result?.summary ?? formatSessionFact(session)) : formatSessionFact(session);
  const stop = (handler: () => void) => (event: { stopPropagation(): void }) => {
    event.stopPropagation();
    handler();
  };
  const answerable = session.state === "WAITING" && canAnswer(session);
  const choices = answerable && offeredChoices(session).length > 0;
  const reason = noAnswerReason(session);
  const where = `${machineName}${session.runningFor ? ` · ${session.runningFor}` : ""}`;

  return (
    // The title is the row's one "open" control; the rest of the row is a
    // larger mouse target for it (no button nested in a button).
    <article className={styles.needRow} aria-labelledby={titleId} onClick={onOpen}>
      <StateIcon state={session.state} />
      <div className={styles.needBody}>
        <h3 className={styles.needTitle} id={titleId}>
          <button
            type="button"
            className={styles.titleButton}
            onClick={stop(onOpen)}
            aria-label={`${session.name}, ${session.state.toLowerCase()} on ${machineName}`}
          >
            {session.name}
          </button>
          {session.codingAgent && <AgentBadge agent={session.codingAgent} />}
        </h3>
        {session.state === "WAITING" && session.prompt ? (
          <p className={styles.needMeta}>
            <span className={styles.mono}>{session.prompt}</span> · {where}
          </p>
        ) : (
          <p className={styles.needMeta}>
            {where} · {session.state === "WAITING" ? "Waiting for input" : fact}
          </p>
        )}
        {reason && <p className={styles.needNote}>{reason}</p>}
        {choices && (
          <div className={styles.choices} onClick={(event) => event.stopPropagation()}>
            <PromptChoices session={session} />
          </div>
        )}
        {removal.error && (
          <p className={styles.needNote} role="alert">
            {removal.error}
          </p>
        )}
      </div>

      <div className={styles.actions}>
        {answerable && !choices ? (
          <>
            <button type="button" className={styles.yes} onClick={stop(() => void answerPrompt(session.id, "y"))} aria-label={`Answer y to ${session.name}`}>
              y
            </button>
            <button type="button" className={styles.no} onClick={stop(() => void answerPrompt(session.id, "n"))} aria-label={`Answer n to ${session.name}`}>
              n
            </button>
          </>
        ) : session.state === "STALLED" ? (
          <>
            <button type="button" className={styles.pill} onClick={stop(onOpen)}>
              Open terminal
            </button>
            {canInterrupt(session) && (
              <button type="button" className={styles.danger} onClick={stop(onInterrupt)}>
                Kill…
              </button>
            )}
          </>
        ) : (
          <>
            <button type="button" className={styles.primary} onClick={stop(onOpen)}>
              {choices ? "Open terminal" : "Open"}
            </button>
            {session.state === "FAILED" && session.command && (
              <button
                type="button"
                className={styles.pill}
                onClick={stop(() => navigate(`/app/history?${new URLSearchParams({ q: session.command ?? "", result: "ok" }).toString()}`))}
              >
                Last passing run
              </button>
            )}
            {session.state === "FAILED" && (
              <button
                type="button"
                className={styles.quietButton}
                disabled={removal.busy}
                onClick={stop(() => void removal.hide([session.id]))}
                aria-label={`Hide ${session.name}`}
              >
                Hide
              </button>
            )}
          </>
        )}
      </div>
    </article>
  );
}

function MachineLane({
  machine,
  sessions,
  load,
  onOpen,
  onScope,
  emptyNote,
}: {
  machine: Machine;
  sessions: Session[];
  load: MachineMetricSample | undefined;
  onOpen: (id: string) => void;
  onScope?: () => void;
  emptyNote: string;
}) {
  const online = machine.status === "ONLINE";
  const meters: Array<[string, number | undefined]> =
    load?.gpuPct !== undefined
      ? [["GPU", load.gpuPct], ["VRAM", load.vramPct]]
      : [["CPU", load?.cpuPct], ["RAM", load?.ramPct]];
  return (
    <section className={styles.section} data-offline={online ? undefined : true} aria-label={machine.name}>
      <div className={styles.sectionHead}>
        {onScope ? (
          <button type="button" className={styles.laneName} onClick={onScope}>
            {machine.name}
          </button>
        ) : (
          <h2 className={styles.sectionTitle}>{machine.name}</h2>
        )}
        <span className={styles.laneMeta}>
          {online &&
            meters
              .filter(([, value]) => value !== undefined)
              .map(([label, value]) => (
                <span key={label} data-high={(value ?? 0) >= 90 || undefined} className={styles.meterText}>
                  {label} {Math.round(value ?? 0)} %
                </span>
              ))}
          <span className={styles.muted}>{machineSummary(machine)}</span>
        </span>
      </div>
      {/* An unreachable Machine keeps its last snapshot visible (milestone 4.A), dimmed, under one note. */}
      {!online && (
        <p className={styles.empty}>
          {STATUS_WORD[machine.status]} · {machine.lastSeen ? `last seen ${machine.lastSeen} ago` : "not seen yet"} · sessions resume when it
          reconnects
        </p>
      )}
      {online && sessions.length === 0 && <p className={styles.empty}>{emptyNote}</p>}
      {sessions.map((session) => (
        <LaneRow key={session.id} session={session} onOpen={() => onOpen(session.id)} />
      ))}
    </section>
  );
}

function LaneRow({ session, onOpen }: { session: Session; onOpen: () => void }) {
  const port = extractPort(session);
  const percent = session.progress && session.progress.total > 0 ? Math.round((session.progress.current / session.progress.total) * 100) : null;
  const corner = percent !== null ? `${percent} %` : (port ? `:${port}` : null) ?? session.runningFor ?? "";
  return (
    <button type="button" className={styles.laneRow} onClick={onOpen} aria-label={`${session.name}, ${session.state.toLowerCase()}${corner ? `, ${corner}` : ""}`}>
      <StateIcon state={session.state} size={20} />
      <span className={styles.laneTitle}>{session.name}</span>
      {session.codingAgent && <AgentBadge agent={session.codingAgent} />}
      <span className={styles.laneCommand}>{session.command ?? session.cwd ?? ""}</span>
      {percent !== null ? (
        <span className={styles.progress} aria-hidden="true">
          <span className={styles.progressFill} style={{ width: `${percent}%` }} />
        </span>
      ) : (
        <span />
      )}
      <span className={styles.laneCorner}>{corner}</span>
    </button>
  );
}

/** Wall clock for the header, every 30 s. */
export function useClock(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(timer);
  }, []);
  return now;
}

/** Each online Machine's most recent load sample, refreshed every 30 s. */
export function useMachineLoad(machines: Machine[]): Record<string, MachineMetricSample | undefined> {
  const [load, setLoad] = useState<Record<string, MachineMetricSample | undefined>>({});
  const ids = machines
    .filter((machine) => machine.status === "ONLINE")
    .map((machine) => machine.id)
    .join(",");
  useEffect(() => {
    let live = true;
    const fetchAll = () => {
      for (const id of ids.split(",").filter(Boolean)) {
        dataAdapter.machineInsights
          .metrics(id, "HOUR")
          .then((samples) => live && setLoad((previous) => ({ ...previous, [id]: samples.at(-1) })))
          .catch(() => undefined);
      }
    };
    fetchAll();
    const timer = setInterval(fetchAll, LOAD_MS);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [ids]);
  return load;
}
