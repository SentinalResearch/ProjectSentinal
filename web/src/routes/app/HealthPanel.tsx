import { useEffect, useState } from "react";

import { dataAdapter } from "../../data";
import { useLiveData } from "../../data/LiveDataContext";
import { machineStatusLabel } from "../../data/machine-status";
import type { HealthIncident } from "../../protocol/HealthIncident";
import type { HealthMetric } from "../../protocol/HealthMetric";
import type { HealthRange } from "../../protocol/HealthRange";
import type { HealthSummary } from "../../protocol/HealthSummary";
import type { MachineHealth } from "../../protocol/MachineHealth";
import { HealthChart } from "./HealthChart";
import styles from "./HealthPanel.module.css";
import { emptyText, incidentText, pathText, reading, type Reading } from "./health-view";
import machinesStyles from "./MachinePanel.module.css";

const RANGES: { value: HealthRange; label: string }[] = [
  { value: "1h", label: "1 h" },
  { value: "24h", label: "24 h" },
  { value: "7d", label: "7 d" },
  { value: "30d", label: "30 d" },
];

const ROWS: { metric: HealthMetric; label: string }[] = [
  { metric: "TYPING_ECHO_MS", label: "Typing delay" },
  { metric: "TERMINAL_OPEN_MS", label: "Terminal open" },
  { metric: "LINK_RTT_MS", label: "Link round trip" },
  { metric: "UPLOAD_DELAY_MS", label: "Dashboard update delay" },
  { metric: "DAEMON_RSS_BYTES", label: "sentinald memory" },
];

const CHART_TITLES: Partial<Record<HealthMetric, string>> = {
  TYPING_ECHO_MS: "Typing delay",
  TERMINAL_OPEN_MS: "Terminal open",
  LINK_RTT_MS: "Link round trip",
  DAEMON_RSS_BYTES: "sentinald memory",
  UPLOAD_DELAY_MS: "Dashboard update delay",
};

function Value({ value }: { value: Reading | undefined }) {
  if (!value) return <span className={styles.muted}>—</span>;
  return <span className={styles[value.level]}>{value.value}</span>;
}

function MachineCard({ machine, incidents }: { machine: MachineHealth; incidents: HealthIncident[] }) {
  const { machines } = useLiveData();
  const live = machines.find((entry) => entry.id === machine.machine_id);
  const empty = emptyText(machine);
  const readings = ROWS.map((row) => ({ ...row, value: reading(machine.stats, row.metric) }));
  const reasons = readings.flatMap((row) => (row.value?.reason ? [row.value.reason] : []));
  const path = pathText(incidents, machine.machine_id);
  return (
    <section role="group" aria-label={`${machine.display_name} health`} className={styles.card}>
      <header className={styles.cardHead}>
        <strong>{machine.display_name}</strong>
        {machine.role === "CONTROL" && <span className={styles.badge}>Control</span>}
        <span className={styles.muted}>
          {live ? machineStatusLabel(live, Date.now()) : "Unknown"}
          {machine.daemon_version ? ` · ${machine.daemon_version}` : ""}
          {path ? ` · ${path}` : ""}
        </span>
      </header>
      {empty ? (
        <p className={styles.muted}>{empty}</p>
      ) : (
        <dl className={styles.readings}>
          {readings.map((row) => (
            <div key={row.metric}>
              <dt>{row.label}</dt>
              <dd>
                <Value value={row.value} />
              </dd>
            </div>
          ))}
        </dl>
      )}
      {reasons.map((reason) => (
        <p key={reason} className={styles.reason}>
          {reason}
        </p>
      ))}
    </section>
  );
}

/** Settings → Diagnostics, top: what the control has measured over a range (4.H). */
export function HealthPanel() {
  const [range, setRange] = useState<HealthRange>("24h");
  const [summary, setSummary] = useState<HealthSummary | null>(null);
  const [incidents, setIncidents] = useState<HealthIncident[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [chartMachine, setChartMachine] = useState<string | null>(null);
  const [realNames, setRealNames] = useState(false);

  useEffect(() => {
    let cancelled = false;
    Promise.all([dataAdapter.health.summary(range), dataAdapter.health.incidents(range)])
      .then(([nextSummary, nextIncidents]) => {
        if (cancelled) return;
        setSummary(nextSummary);
        setIncidents(nextIncidents.incidents);
        setError(null);
      })
      .catch((failure: unknown) => {
        if (!cancelled) setError(failure instanceof Error ? failure.message : String(failure));
      });
    return () => {
      cancelled = true;
    };
  }, [range]);

  const names = new Map(summary?.machines.map((machine) => [machine.machine_id, machine.display_name]));
  const charted = summary?.machines.find((machine) => machine.machine_id === chartMachine) ?? summary?.machines[0];
  const reportUrl = dataAdapter.health.reportUrl(range, realNames ? "real" : "anonymous");
  const now = Date.now();

  return (
    <section aria-label="Health" className={styles.panel}>
      <div className={styles.toolbar}>
        <h3 className={styles.title}>Health</h3>
        <div className={machinesStyles.rangeGroup} role="group" aria-label="Range">
          {RANGES.map((option) => (
            <button
              key={option.value}
              type="button"
              className={machinesStyles.rangeButton}
              aria-pressed={range === option.value}
              onClick={() => setRange(option.value)}
            >
              {option.label}
            </button>
          ))}
        </div>
      </div>
      {error && (
        <p role="alert" className={styles.bad}>
          {error}
        </p>
      )}
      {summary && (
        <>
          <div className={styles.cards}>
            {summary.machines.map((machine) => (
              <MachineCard key={machine.machine_id} machine={machine} incidents={incidents} />
            ))}
          </div>

          {charted && charted.series.length > 0 && (
            <div className={styles.charts}>
              <label className={styles.chartPicker}>
                Charts for{" "}
                <select value={charted.machine_id} onChange={(event) => setChartMachine(event.target.value)}>
                  {summary.machines.map((machine) => (
                    <option key={machine.machine_id} value={machine.machine_id}>
                      {machine.display_name}
                    </option>
                  ))}
                </select>
              </label>
              {charted.series.map((series) => (
                <HealthChart
                  key={series.metric}
                  metric={series.metric}
                  title={`${CHART_TITLES[series.metric] ?? series.metric} on ${charted.display_name}`}
                  points={series.points}
                />
              ))}
            </div>
          )}

          <h4 className={styles.subtitle}>Incidents</h4>
          {incidents.length === 0 ? (
            <p className={styles.muted}>No incidents in this range.</p>
          ) : (
            <ul aria-label="Incidents" className={styles.incidents}>
              {incidents.map((incident) => (
                <li key={`${incident.machine_id}-${incident.kind}-${incident.started_at}`}>
                  {incidentText(incident, names.get(incident.machine_id) ?? "A Machine", now)}
                </li>
              ))}
            </ul>
          )}

          <div className={styles.report}>
            {reportUrl ? (
              <>
                <a className={styles.download} href={reportUrl} download>
                  Download report
                </a>
                <label>
                  <input type="checkbox" checked={realNames} onChange={(event) => setRealNames(event.target.checked)} /> Use
                  real Machine names
                </label>
                <span className={styles.muted}>
                  Numbers and codes only. By default Machines are "Machine 1, 2, …" so the report is safe to share.
                </span>
              </>
            ) : (
              <span className={styles.muted}>Reports download from a real control, not in demo mode.</span>
            )}
          </div>
        </>
      )}
    </section>
  );
}
