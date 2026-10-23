import { useSearchParams } from "react-router-dom";

import { needsAttention } from "../../../data/format";
import { useLiveData } from "../../../data/LiveDataContext";
import { useNewSession } from "../NewSession";
import { NeedsYouCard } from "./NeedsYouCard";
import { PhoneSessionRow } from "./PhoneSessionRow";
import styles from "./phoneSession.module.css";
import shell from "./phone.module.css";
import { PageTitle } from "./PageTitle";

/** B2 · Overview on phones. */
export function PhoneOverview() {
  const [params, setParams] = useSearchParams();
  const { machines, terminals: sessions, loading } = useLiveData();
  const filter = params.get("machine");
  // The top bar's needs-you count opens Overview filtered to just those Sessions.
  const needsOnly = params.get("needs") === "1";
  const visible = filter ? sessions.filter((session) => session.machine === filter) : sessions;
  const needs = visible.filter(needsAttention);
  const groups = needsOnly ? [] : filter ? machines.filter((machine) => machine.id === filter) : machines;

  const newSession = useNewSession((id) => `/app/sessions/${id}/terminal`);

  if (loading) return <div className={shell.page}>Loading…</div>;

  return (
    <div className={shell.page}>
      <header className={styles.titleRow}>
        <PageTitle>Overview</PageTitle>
        <button type="button" className={styles.plus} aria-label="New session" onClick={() => newSession.start(filter)}>
          +
        </button>
      </header>
      {newSession.chooser}

      <div className={styles.chips} role="group" aria-label="Machines">
        <button type="button" className={styles.chip} aria-pressed={!filter && !needsOnly} onClick={() => setParams({})}>
          All
        </button>
        {needsOnly && (
          <button type="button" className={styles.chip} aria-pressed="true" onClick={() => setParams({})}>
            Needs you {needs.length}
          </button>
        )}
        {machines.map((machine) => (
          <button
            key={machine.id}
            type="button"
            className={styles.chip}
            aria-pressed={!needsOnly && filter === machine.id}
            onClick={() => setParams({ machine: machine.id })}
          >
            <span className={styles.dot} data-online={machine.status === "ONLINE"} aria-hidden="true" />
            {machine.name}
          </button>
        ))}
      </div>

      {needs.length > 0 && (
        <section aria-label="Needs you">
          <h2 className={styles.sectionHead}>
            Needs you <span className={styles.needsCount}>{needs.length}</span>
          </h2>
          <div className={styles.needsList}>
            {needs.map((session) => (
              <NeedsYouCard key={session.id} session={session} machine={machines.find((m) => m.id === session.machine)} />
            ))}
          </div>
        </section>
      )}

      {needsOnly && needs.length === 0 && <p className={styles.empty}>Nothing needs you right now.</p>}

      {groups.map((machine) => {
        const rows = visible.filter((session) => session.machine === machine.id);
        return (
          <section key={machine.id} aria-label={machine.name}>
            <h2 className={styles.sectionHead}>
              <span className={styles.machineName}>{machine.name}</span>
              <span className={styles.machineState}>{machine.status === "ONLINE" ? "online" : machine.status.toLowerCase()}</span>
            </h2>
            {rows.map((session) => (
              <PhoneSessionRow key={session.id} session={session} />
            ))}
            {rows.length === 0 && <p className={styles.empty}>No Sessions.</p>}
          </section>
        );
      })}
    </div>
  );
}
