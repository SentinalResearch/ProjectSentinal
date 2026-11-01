import { Link, useSearchParams } from "react-router-dom";

import { AttentionCard, GlassPanel } from "../../../components/glass/Glass";
import { StateTag } from "../../../components/StateTag";
import { answerPrompt, canAnswer, offeredChoices } from "../../../data/actions";
import { PromptChoices } from "../../../components/PromptChoices";
import { exitUnknown } from "../../../data/capture";
import {
  extractPort,
  formatSessionFact,
  needsAttention,
} from "../../../data/format";
import { useLiveData } from "../../../data/LiveDataContext";
import { isDelayed, machineStatusText } from "../../../data/machine-status";
import type { Machine, Session } from "../../../data/types";
import {
  onLane,
  STATUS_WORD,
  useClock,
  useMachineLoad,
} from "../dashboard/GlassDashboard";
import { PageTitle } from "./PageTitle";
import styles from "./PhoneDashboard.module.css";

/**
 * M1 · the phone dashboard (MOBILE_GLASS_SPEC §2): the same data as desktop.
 * `?needs=1` (the island's count) and `?machine=` (`go <machine>`) narrow it.
 */
export function PhoneOverview() {
  const [params, setParams] = useSearchParams();
  const { machines, terminals, loading } = useLiveData();
  const now = useClock();
  const load = useMachineLoad(machines);
  const machineFilter = params.get("machine");
  const needsOnly = params.get("needs") === "1";
  const visible = machineFilter
    ? terminals.filter((session) => session.machine === machineFilter)
    : terminals;
  const needs = visible.filter(needsAttention);
  const answerable = needs.filter(canAnswer);
  const others = needs.filter((session) => !canAnswer(session));
  const lanes = needsOnly
    ? []
    : machines.filter(
        (machine) => !machineFilter || machine.id === machineFilter,
      );
  const online = machines.filter(
    (machine) => machine.status === "ONLINE",
  ).length;
  const machineName = (id: string) =>
    machines.find((machine) => machine.id === id)?.name ?? id;
  const filterName = machineFilter
    ? machineName(machineFilter)
    : needsOnly
      ? "What needs you"
      : null;
  const time = new Date(now).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });

  if (loading) return <div className={styles.page}>Loading…</div>;

  return (
    <div className={styles.page}>
      <PageTitle>Dashboard</PageTitle>
      <p className={styles.overline}>
        {time} · {terminals.length} session{terminals.length === 1 ? "" : "s"} ·{" "}
        {online} machine{online === 1 ? "" : "s"}
      </p>
      <p className={styles.hero}>
        {needs.length > 0
          ? `${needs.length} need${needs.length === 1 ? "s" : ""} you.`
          : "All quiet."}
      </p>
      {filterName && (
        <p className={styles.filter}>
          {filterName} ·{" "}
          <button
            type="button"
            className={styles.showAll}
            onClick={() => setParams({})}
          >
            Show all
          </button>
        </p>
      )}

      {needs.length > 0 && (
        <section className={styles.needs} aria-label="Needs you">
          {answerable.map((session) => (
            <AttentionCard
              key={session.id}
              state={session.state}
              className={styles.waitCard}
              aria-label={session.name}
            >
              <div className={styles.cardTop}>
                <StateTag state={session.state} size="sm" />
                <span className={styles.where}>
                  {machineName(session.machine)}
                  {session.runningFor ? ` · ${session.runningFor}` : ""}
                </span>
              </div>
              <Link
                to={`/app/sessions/${encodeURIComponent(session.id)}`}
                className={styles.cardTitle}
              >
                {session.name}
              </Link>
              {offeredChoices(session).length > 0 ? (
                <div className={styles.choices}>
                  <span className={styles.well}>{session.prompt}</span>
                  <PromptChoices session={session} />
                </div>
              ) : (
                <div className={styles.answer}>
                  <span className={styles.well}>{session.prompt}</span>
                  <button
                    type="button"
                    className={styles.yes}
                    onClick={() => void answerPrompt(session.id, "y")}
                    aria-label={`Answer y to ${session.name}`}
                  >
                    y
                  </button>
                  <button
                    type="button"
                    className={styles.no}
                    onClick={() => void answerPrompt(session.id, "n")}
                    aria-label={`Answer n to ${session.name}`}
                  >
                    n
                  </button>
                </div>
              )}
            </AttentionCard>
          ))}
          <div className={styles.pairs}>
            {others.map((session) => (
              <AttentionCard
                key={session.id}
                as={Link}
                to={`/app/sessions/${encodeURIComponent(session.id)}`}
                state={session.state}
                className={styles.smallCard}
              >
                <StateTag
                  state={session.state}
                  size="sm"
                  exitUnknown={exitUnknown(session)}
                />
                <span className={styles.smallTitle}>{session.name}</span>
                <span className={styles.smallFact}>
                  {[shortFact(session), machineName(session.machine)]
                    .filter(Boolean)
                    .join(" · ")}
                </span>
              </AttentionCard>
            ))}
          </div>
        </section>
      )}
      {needsOnly && needs.length === 0 && (
        <p className={styles.empty}>Nothing needs you right now.</p>
      )}

      {lanes.map((machine) => (
        <Lane
          key={machine.id}
          machine={machine}
          sessions={terminals.filter(
            (session) => session.machine === machine.id && onLane(session),
          )}
          load={load[machine.id]}
        />
      ))}
    </div>
  );
}

/** A FAILED card says how it failed; a STALLED one, how long it has been quiet. */
function shortFact(session: Session): string {
  if (session.result) return `${session.result.failed} failed`;
  if (session.state === "STALLED") return session.runningFor ?? "quiet";
  return formatSessionFact(session) === "—" ? "" : formatSessionFact(session);
}

function Lane({
  machine,
  sessions,
  load,
}: {
  machine: Machine;
  sessions: Session[];
  load?: { cpuPct: number; gpuPct?: number };
}) {
  const online = machine.status === "ONLINE";
  const metric =
    load?.gpuPct !== undefined
      ? `GPU ${Math.round(load.gpuPct)} %`
      : load
        ? `CPU ${Math.round(load.cpuPct)} %`
        : "";
  return (
    <GlassPanel
      className={styles.lane}
      data-offline={online ? undefined : true}
      aria-label={machine.name}
    >
      <div className={styles.laneHead}>
        <span
          className={styles.laneSquare}
          data-online={online || undefined}
          aria-hidden="true"
        />
        <span className={styles.laneName}>{machine.name}</span>
        <span className={styles.laneMeta}>
          {!online
            ? `${STATUS_WORD[machine.status].toLowerCase()}${machine.lastSeen ? ` · ${machine.lastSeen}` : ""}`
            : isDelayed(machine)
              ? machineStatusText(machine, Date.now())
              : metric}
        </span>
      </div>
      {sessions.map((session) => (
        <Link
          key={session.id}
          to={`/app/sessions/${encodeURIComponent(session.id)}`}
          className={styles.row}
        >
          <StateTag
            state={session.state}
            size="sm"
            exitUnknown={exitUnknown(session)}
          />
          <span className={styles.rowTitle}>{session.name}</span>
          <span className={styles.rowMeta}>{rowMeta(session)}</span>
        </Link>
      ))}
      {online && sessions.length === 0 && (
        <p className={styles.empty}>No Sessions.</p>
      )}
    </GlassPanel>
  );
}

function rowMeta(session: Session): string {
  if (session.progress && session.progress.total > 0) {
    return `${Math.round((session.progress.current / session.progress.total) * 100)} %`;
  }
  const port = extractPort(session);
  return port ? `:${port}` : (session.runningFor ?? "");
}
