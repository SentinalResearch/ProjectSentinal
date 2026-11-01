import { useEffect, useId, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";

import { AgentBadge } from "../../../components/AgentBadge";
import { AttentionCard, GlassPanel } from "../../../components/glass/Glass";
import { StateTag } from "../../../components/StateTag";
import { dataAdapter } from "../../../data";
import { answerPrompt, canAnswer, canInterrupt, offeredChoices } from "../../../data/actions";
import { PromptChoices } from "../../../components/PromptChoices";
import { exitUnknown } from "../../../data/capture";
import { extractPort, formatSessionFact, machineSummary, needsAttention, sessionKind } from "../../../data/format";
import { useLiveData } from "../../../data/LiveDataContext";
import type { Machine, MachineMetricSample, Session } from "../../../data/types";
import { InterruptConfirm } from "../InterruptConfirm";
import { useShell } from "../shell/ShellContext";
import { awayFeed } from "./feed";
import { recordSample, sparkBars, type SparkHistory } from "./spark";
import styles from "./GlassDashboard.module.css";

const SAMPLE_MS = 3_000;
const LOAD_MS = 30_000;

/** The glass dashboard (GLASS_SPEC.md §7), desktop ≥ 1024 px. */
export function GlassDashboard() {
  const navigate = useNavigate();
  const shell = useShell();
  const { machines, terminals, loading } = useLiveData();
  const [interrupting, setInterrupting] = useState<Session | null>(null);
  // 6.1: show only Sessions running an AI coding agent.
  const [agentsOnly, setAgentsOnly] = useState(false);
  const shown = (session: Session) => !agentsOnly || session.codingAgent !== undefined;
  const now = useClock();
  const sparks = useSparkHistory(terminals);
  const load = useMachineLoad(machines);

  const needs = terminals.filter((session) => needsAttention(session) && shown(session));
  const online = machines.filter((machine) => machine.status === "ONLINE").length;
  const feed = useMemo(() => awayFeed(terminals, machines), [terminals, machines]);
  const since = feed.filter((item) => item.time).at(-1)?.time.slice(0, 5);
  const machineName = (id: string) => machines.find((machine) => machine.id === id)?.name ?? id;
  const open = (id: string) => navigate(`/app/sessions/${encodeURIComponent(id)}`);

  const date = new Date(now).toLocaleString(undefined, { weekday: "long", hour: "2-digit", minute: "2-digit", hour12: false });

  return (
    <div className={styles.page}>
      <div className={styles.main}>
        <header className={styles.header}>
          <p className={styles.overline}>
            {date} · {terminals.length} session{terminals.length === 1 ? "" : "s"} on {online} machine{online === 1 ? "" : "s"}
          </p>
          <div className={styles.heroRow}>
            <h1 className={styles.hero}>{loading ? "Loading…" : needs.length > 0 ? `${needs.length} need${needs.length === 1 ? "s" : ""} you.` : "All quiet."}</h1>
            <button
              type="button"
              className={styles.filter}
              aria-pressed={agentsOnly}
              aria-label="AI agents only"
              onClick={() => setAgentsOnly((only) => !only)}
            >
              AI agents
            </button>
            {shell && (
              <button type="button" className={styles.hint} onClick={() => shell.openPrompt()}>
                press <kbd>/</kbd> to do anything
              </button>
            )}
          </div>
        </header>

        {needs.length > 0 && (
          <section className={styles.needs} aria-labelledby="needs-you-heading">
            <h2 id="needs-you-heading" className={styles.visuallyHidden}>
              Needs you
            </h2>
            {needs.map((session) => (
              <NeedsCard
                key={session.id}
                session={session}
                machineName={machineName(session.machine)}
                onOpen={() => open(session.id)}
                onInterrupt={() => setInterrupting(session)}
              />
            ))}
          </section>
        )}

        <div className={styles.lanes}>
          {machines.map((machine) => (
            <MachineLane
              key={machine.id}
              machine={machine}
              sessions={terminals.filter((session) => session.machine === machine.id && onLane(session) && shown(session))}
              emptyNote={agentsOnly ? "No AI agents." : "No Sessions."}
              sparks={sparks}
              load={load[machine.id]}
              onOpen={open}
              onOpenMachine={() => navigate(`/app/overview?machine=${encodeURIComponent(machine.id)}`)}
            />
          ))}
        </div>
      </div>

      <GlassPanel as="aside" className={styles.feed} aria-label="While you were away">
        <div className={styles.feedHead}>
          <h2 className={styles.feedTitle}>While you were away</h2>
          {since && <span className={styles.feedSince}>since {since}</span>}
        </div>
        {feed.length === 0 && <p className={styles.feedEmpty}>Nothing new.</p>}
        <ol className={styles.feedList}>
          {feed.map((item) => (
            <li key={item.id}>
              <button
                type="button"
                className={styles.feedItem}
                disabled={!item.sessionId}
                onClick={() => item.sessionId && open(item.sessionId)}
              >
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
      </GlassPanel>

      <InterruptConfirm
        session={interrupting ?? undefined}
        machineName={interrupting ? machineName(interrupting.machine) : ""}
        onClose={() => setInterrupting(null)}
      />
    </div>
  );
}

/** How a lane names a Machine that is not online. */
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


function NeedsCard({
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
  const fact = session.state === "FAILED" ? (session.result?.summary ?? formatSessionFact(session)) : formatSessionFact(session);
  const stop = (handler: () => void) => (event: { stopPropagation(): void }) => {
    event.stopPropagation();
    handler();
  };
  return (
    // The title is the card's one "open" control; the rest of the card is a
    // larger mouse target for it (no button nested in a button).
    <AttentionCard as="article" state={session.state} className={styles.needCard} aria-labelledby={titleId} onClick={onOpen}>
      <div className={styles.needTop}>
        <StateTag state={session.state} size="sm" exitUnknown={exitUnknown(session)} />
        {session.codingAgent && <AgentBadge agent={session.codingAgent} />}
        <span className={styles.needWhere}>
          {machineName}
          {session.runningFor ? ` · ${session.runningFor}` : ""}
        </span>
      </div>
      <h3 className={styles.needTitle} id={titleId}>
        <button
          type="button"
          className={styles.titleButton}
          onClick={stop(onOpen)}
          aria-label={`${session.name}, ${session.state.toLowerCase()} on ${machineName}`}
        >
          {session.name}
        </button>
      </h3>

      {session.state === "WAITING" && canAnswer(session) && offeredChoices(session).length > 0 ? (
        <div className={styles.choiceColumn}>
          <span className={styles.well}>{session.prompt}</span>
          <PromptChoices session={session} />
        </div>
      ) : session.state === "WAITING" && canAnswer(session) ? (
        <div className={styles.answerRow}>
          <span className={styles.well}>{session.prompt}</span>
          <button type="button" className={styles.yes} onClick={stop(() => void answerPrompt(session.id, "y"))} aria-label={`Answer y to ${session.name}`}>
            y
          </button>
          <button type="button" className={styles.no} onClick={stop(() => void answerPrompt(session.id, "n"))} aria-label={`Answer n to ${session.name}`}>
            n
          </button>
        </div>
      ) : (
        <>
          <p className={styles.needFact}>{session.state === "WAITING" ? (session.prompt ?? "Waiting for input") : fact}</p>
          <div className={styles.actions}>
            {session.state === "STALLED" ? (
              <>
                <button type="button" className={styles.chip} onClick={stop(onOpen)}>
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
                <button type="button" className={styles.chip} onClick={stop(onOpen)}>
                  Open
                </button>
                {session.state === "FAILED" && session.command && (
                  <button
                    type="button"
                    className={styles.chip}
                    onClick={stop(() =>
                      navigate(`/app/history?${new URLSearchParams({ q: session.command ?? "", result: "ok" }).toString()}`),
                    )}
                  >
                    Last passing run
                  </button>
                )}
              </>
            )}
          </div>
        </>
      )}
    </AttentionCard>
  );
}

function MachineLane({
  machine,
  sessions,
  sparks,
  load,
  onOpen,
  onOpenMachine,
  emptyNote,
}: {
  machine: Machine;
  sessions: Session[];
  sparks: SparkHistory;
  load: MachineMetricSample | undefined;
  onOpen: (id: string) => void;
  onOpenMachine: () => void;
  emptyNote: string;
}) {
  const online = machine.status === "ONLINE";
  const bars: Array<[string, number | undefined]> =
    load?.gpuPct !== undefined
      ? [["GPU", load.gpuPct], ["VRAM", load.vramPct]]
      : [["CPU", load?.cpuPct], ["RAM", load?.ramPct]];
  return (
    <GlassPanel className={styles.lane} data-offline={online ? undefined : true} aria-label={machine.name}>
      <div className={styles.laneInfo}>
        <button type="button" className={styles.laneName} onClick={onOpenMachine}>
          <span className={styles.laneSquare} data-online={online || undefined} aria-hidden="true" />
          {machine.name}
        </button>
        <span className={styles.laneHardware}>{machineSummary(machine)}</span>
        {online && (
          <div className={styles.meters}>
            {bars.map(([label, value]) => (
              <span key={label} className={styles.meter} title={value === undefined ? `${label} unknown` : `${label} ${Math.round(value)} %`}>
                <span className={styles.meterLabel}>{label}</span>
                <span className={styles.meterTrack}>
                  <span
                    className={styles.meterFill}
                    data-high={(value ?? 0) >= 90 || undefined}
                    style={{ width: `${Math.min(100, Math.max(0, value ?? 0))}%` }}
                  />
                </span>
              </span>
            ))}
          </div>
        )}
      </div>
      {/* An unreachable Machine keeps its last snapshot visible (milestone 4.A), dimmed, under one note. */}
      <div className={styles.cards}>
        {!online && (
          <p className={styles.laneNote}>
            {STATUS_WORD[machine.status]} · {machine.lastSeen ? `last seen ${machine.lastSeen} ago` : "not seen yet"} · sessions
            resume when it reconnects
          </p>
        )}
        {online && sessions.length === 0 && <p className={styles.laneNote}>{emptyNote}</p>}
        {sessions.map((session) => (
          <LaneCard key={session.id} session={session} samples={sparks[session.id]} onOpen={() => onOpen(session.id)} />
        ))}
      </div>
    </GlassPanel>
  );
}

function LaneCard({ session, samples, onOpen }: { session: Session; samples?: Array<number | null>; onOpen: () => void }) {
  const training = sessionKind(session) === "training";
  const port = extractPort(session);
  const progress = session.progress && session.progress.total > 0 ? `${Math.round((session.progress.current / session.progress.total) * 100)} %` : null;
  const corner = progress ?? (port ? `:${port}` : null) ?? session.runningFor ?? (session.cwd ? session.cwd.replace(/^.*\/(?=[^/]+$)/, "~/") : "");
  const bars = sparkBars(samples);
  return (
    <button type="button" className={styles.laneCard} data-focus={training || undefined} onClick={onOpen}>
      <span className={styles.laneCardTop}>
        <StateTag state={session.state} size="sm" exitUnknown={exitUnknown(session)} />
        <span className={styles.laneCorner}>{corner}</span>
      </span>
      <span className={styles.laneTitle}>{session.name}</span>
      {session.codingAgent && <AgentBadge agent={session.codingAgent} />}
      <span className={styles.spark} data-state={training ? "FOCUS" : session.state} aria-hidden="true">
        {bars.map((height, index) => (
          <span key={index} className={height === null ? styles.dash : styles.bar} style={height === null ? undefined : { height: `${Math.round(height * 100)}%` }} />
        ))}
      </span>
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

/** The last 20 samples of each Session's activity, taken every 3 s. */
function useSparkHistory(sessions: Session[]): SparkHistory {
  const [history, setHistory] = useState<SparkHistory>(() => recordSample({}, sessions));
  // One steady timer reading the newest Sessions: live updates must not reset it.
  const latest = useRef(sessions);
  useEffect(() => {
    latest.current = sessions;
    // First data: draw a sample now rather than three seconds later.
    setHistory((previous) => (Object.keys(previous).length === 0 && sessions.length > 0 ? recordSample(previous, sessions) : previous));
  }, [sessions]);
  useEffect(() => {
    const timer = setInterval(() => setHistory((previous) => recordSample(previous, latest.current)), SAMPLE_MS);
    return () => clearInterval(timer);
  }, []);
  return history;
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
