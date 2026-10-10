import type { KeyboardEvent, MouseEvent } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";

import { MockBadge } from "../../components/MockBadge";
import { CaptureBadge } from "../../components/CaptureBadge";
import { StateTag } from "../../components/StateTag";
import { captureCapabilities, captureOf, exitUnknown } from "../../data/capture";
import { dataAdapter } from "../../data";
import { formatSessionDuration, formatSessionFact, needsAttention } from "../../data/format";
import { useLiveData } from "../../data/LiveDataContext";
import type { Machine, Session } from "../../data/types";
import { useIsPhone } from "../../hooks/useViewport";
import { MachinePanel } from "./MachinePanel";
import { PhoneOverview } from "./phone/PhoneOverview";
import styles from "./OverviewPage.module.css";

function needsFact(session: Session): string {
  if (session.state === "WAITING") return "Waiting for input";
  return formatSessionFact(session);
}

function connectionLabel(machine: Machine): string {
  switch (machine.status) {
    case "ONLINE": return `Online · ${machine.os}`;
    case "AUTH_EXPIRED": return `Authentication expired · last seen ${machine.lastSeen ?? "never"}`;
    case "DISCONNECTED": return `Disconnected · last seen ${machine.lastSeen ?? "never"}`;
    case "REVOKED": return `Revoked · last seen ${machine.lastSeen ?? "never"}`;
    case "OFFLINE": return `Offline · last seen ${machine.lastSeen ?? "never"}`;
  }
}

/** Activate a clickable row/card with Enter or Space, same as a native button — these are <div role="button">s (not real <button>s) because they contain nested interactive controls (y/n keys), and a <button> can't contain a <button>. */
function onActivateKey(handler: () => void) {
  return (e: KeyboardEvent) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      handler();
    }
  };
}

export function OverviewPage() {
  return useIsPhone() ? <PhoneOverview /> : <DesktopOverview />;
}

function DesktopOverview() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const { machines, sessions, loading } = useLiveData();
  const machineFilter = searchParams.get("machine");

  const open = (id: string) => navigate(`/app/sessions/${id}`);
  const answer = (e: MouseEvent, sessionId: string, value: "y" | "n") => {
    e.stopPropagation();
    void dataAdapter.sendInput(sessionId, value);
  };

  if (loading) {
    return (
      <div className={styles.page}>
        <p className={styles.summary}>Loading…</p>
      </div>
    );
  }

  const filteredMachine: Machine | undefined = machineFilter ? machines.find((m) => m.id === machineFilter) : undefined;

  const visibleSessions = machineFilter ? sessions.filter((s) => s.machine === machineFilter) : sessions;
  const needs = visibleSessions.filter(needsAttention);

  const onlineCount = machines.filter((m) => m.status === "ONLINE").length;
  const unavailableMachines = machines.filter((m) => m.status !== "ONLINE");

  const summary = filteredMachine
    ? filteredMachine.hardware
    : [
        `${sessions.length} session${sessions.length === 1 ? "" : "s"} on ${onlineCount} machine${onlineCount === 1 ? "" : "s"}`,
        needs.length > 0 ? `${needs.length} need you` : null,
        unavailableMachines.length > 0 ? `${unavailableMachines.map((m) => m.name).join(", ")} unavailable` : null,
      ]
        .filter(Boolean)
        .join(" · ");

  const groupMachines = filteredMachine ? [filteredMachine] : machines;

  return (
    <div className={styles.page}>
      <header className={styles.header}>
        <div className={styles.headerText}>
          <div className={styles.titleRow}>
            <h1 className={styles.title}>{filteredMachine ? filteredMachine.name : "Overview"}</h1>
            <MockBadge />
          </div>
          <span className={styles.summary}>{summary}</span>
        </div>
        {filteredMachine && (
          <button type="button" className={styles.clearFilter} onClick={() => navigate("/app/overview")}>
            Show all machines
          </button>
        )}
      </header>

      {filteredMachine && (
        <MachinePanel machine={filteredMachine} sessions={visibleSessions} />
      )}

      {needs.length > 0 && (
        <section className={styles.section}>
          <div className={styles.sectionHead}>
            <span className={styles.sectionTitle}>Needs you</span>
            <span className={styles.sectionTitle}>{needs.length}</span>
          </div>
          <div className={styles.needsGrid}>
            {needs.map((session) => {
              const machine = machines.find((m) => m.id === session.machine);
              return (
                <div
                  key={session.id}
                  role="button"
                  tabIndex={0}
                  className={styles.needsCard}
                  onClick={() => open(session.id)}
                  onKeyDown={onActivateKey(() => open(session.id))}
                >
                  <div className={styles.needsCardTop}>
                    <StateTag state={session.state} size="sm" exitUnknown={exitUnknown(session)} />
                    <CaptureBadge capture={session.capture ?? "FULL"} origin={session.origin} />
                    <span className={styles.needsMachine}>{machine?.name ?? session.machine}</span>
                  </div>
                  <div className={styles.needsBody}>
                    <span className={styles.needsName}>{session.name}</span>
                    <span className={styles.needsFact}>{needsFact(session)}</span>
                  </div>
                  {session.state === "WAITING" && session.prompt && (
                    <div className={styles.promptRow}>
                      <span className={styles.promptText}>{session.prompt}</span>
                      {captureCapabilities(captureOf(session)).canSendInput && (
                        <>
                          <button
                            type="button"
                            className={styles.promptKey}
                            onClick={(e) => answer(e, session.id, "y")}
                          >
                            y
                          </button>
                          <button
                            type="button"
                            className={styles.promptKey}
                            onClick={(e) => answer(e, session.id, "n")}
                          >
                            n
                          </button>
                        </>
                      )}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </section>
      )}

      {groupMachines.map((machine) => {
        const rows = visibleSessions.filter((s) => s.machine === machine.id);
        const online = machine.status === "ONLINE";
        return (
          <section key={machine.id} className={styles.section} style={{ opacity: online ? 1 : 0.55 }}>
            <div className={styles.sectionHeadWrap}>
              <span className={styles.sectionTitle}>{machine.name}</span>
              <span className={styles.machineHead}>
                <span
                  className={styles.machineDot}
                  style={{
                    background: online ? "var(--s-running)" : "transparent",
                    boxShadow: online ? "none" : "inset 0 0 0 1.5px var(--s-disconnected-fg)",
                  }}
                />
                {connectionLabel(machine)}
              </span>
              <span className={styles.machineHw}>{machine.hardware}</span>
            </div>

            {rows.map((session) => (
              <button key={session.id} type="button" className={styles.row} onClick={() => open(session.id)}>
                <span className={styles.rowTagCell}>
                  <StateTag state={session.state} size="sm" exitUnknown={exitUnknown(session)} />
                  <CaptureBadge capture={session.capture ?? "FULL"} origin={session.origin} />
                </span>
                <span className={styles.rowName}>
                  <span className={styles.rowNameText}>{session.name}</span>
                  {session.command && <span className={styles.rowCommand}>{session.command}</span>}
                </span>
                {/* rowMeta is `display:contents` on desktop (invisible — rowFact/rowProject stay
                    direct grid items of .row) and a real flex row on phone, so this one wrapper
                    lets the row reflow to a second line without changing desktop layout at all. */}
                <span className={styles.rowMeta}>
                  <span className={styles.rowFact}>{formatSessionFact(session)}</span>
                  <span className={styles.rowProject}>{session.project ?? "—"}</span>
                </span>
                <span className={styles.rowDuration}>{formatSessionDuration(session)}</span>
              </button>
            ))}

            {rows.length === 0 && machine.status !== "ONLINE" && (
              <div className={styles.emptyRow}>
                {connectionLabel(machine)}. Sessions will appear when the agent reconnects.
              </div>
            )}
            {rows.length === 0 && machine.status === "ONLINE" && (
              <div className={styles.emptyRow}>Online · no Sessions.</div>
            )}
          </section>
        );
      })}
    </div>
  );
}
