import { useNavigate, useSearchParams } from "react-router-dom";

import { dataAdapter } from "../../../data";
import { needsAttention } from "../../../data/format";
import { useLiveData } from "../../../data/LiveDataContext";
import { NeedsYouCard } from "./NeedsYouCard";
import { PhoneSessionRow } from "./PhoneSessionRow";
import styles from "./phoneSession.module.css";
import shell from "./phone.module.css";

/** B2 · Overview on phones. */
export function PhoneOverview() {
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  const { machines, sessions, loading } = useLiveData();
  const filter = params.get("machine");
  const visible = filter ? sessions.filter((session) => session.machine === filter) : sessions;
  const needs = visible.filter(needsAttention);
  const groups = filter ? machines.filter((machine) => machine.id === filter) : machines;

  async function newSession() {
    const session = await dataAdapter.createSession();
    navigate(`/app/sessions/${session.id}/terminal`);
  }

  if (loading) return <div className={shell.page}>Loading…</div>;

  return (
    <div className={shell.page}>
      <header className={styles.titleRow}>
        <h1 className={shell.pageTitle}>Overview</h1>
        <button type="button" className={styles.plus} aria-label="New session" onClick={() => void newSession()}>
          +
        </button>
      </header>

      <div className={styles.chips} role="group" aria-label="Machines">
        <button type="button" className={styles.chip} aria-pressed={!filter} onClick={() => setParams({})}>
          All
        </button>
        {machines.map((machine) => (
          <button
            key={machine.id}
            type="button"
            className={styles.chip}
            aria-pressed={filter === machine.id}
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
