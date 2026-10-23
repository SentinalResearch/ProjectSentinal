import { useEffect, useState } from "react";

import { dataAdapter } from "../../data";
import { useLiveData } from "../../data/LiveDataContext";
import type { ControlPingResult, Machine, PingResult } from "../../data/types";
import settings from "./SettingsPage.module.css";
import styles from "./DiagnosticsTab.module.css";

/** Above this, a round trip is worth a note: typing starts to feel late. */
const SLOW_MS = 120;

interface Measurement {
  browser?: PingResult;
  control?: ControlPingResult;
  error?: string;
}

function median(values: number[]): number | undefined {
  if (values.length === 0) return undefined;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
}

function RoundTrip({ result }: { result?: PingResult }) {
  if (!result) return <span className={styles.muted}>—</span>;
  const middle = median(result.roundTripsMs);
  if (middle === undefined) return <span className={styles.bad}>Unreachable</span>;
  const range = `${Math.min(...result.roundTripsMs).toFixed(1)}–${Math.max(...result.roundTripsMs).toFixed(1)} ms`;
  return (
    <span className={styles.timing} title={`${result.roundTripsMs.length} samples, ${range}`}>
      <span className={middle > SLOW_MS ? styles.slow : undefined}>{middle.toFixed(1)} ms</span>
      <span className={styles.muted}>
        {range}
        {result.failed > 0 ? ` · ${result.failed} failed` : ""}
      </span>
    </span>
  );
}

function pathLabel(control?: ControlPingResult): string {
  if (!control) return "—";
  if (control.path === "DIRECT") return "Direct";
  if (control.path === "RELAYED") return control.relay ? `Relayed · ${control.relay}` : "Relayed";
  return "Unknown";
}

function note(machine: Machine, isControl: boolean, measurement?: Measurement): string {
  if (!measurement) return "";
  if (measurement.error) return measurement.error;
  const { browser, control } = measurement;
  if (browser && browser.roundTripsMs.length === 0) {
    return isControl
      ? "This browser cannot reach the control."
      : `Not reachable through the control. On ${machine.name}, run: sentinal connection status`;
  }
  if (control?.path === "RELAYED") {
    return `A Tailscale relay adds delay. On the control, run: tailscale ping ${machine.name} — a direct path usually needs UDP 41641 allowed.`;
  }
  const slowest = Math.max(median(browser?.roundTripsMs ?? []) ?? 0, median(control?.roundTripsMs ?? []) ?? 0);
  if (slowest > SLOW_MS) return "High delay: terminal typing will feel late.";
  return isControl ? "Round trip from this browser to the control." : "";
}

/** Settings → Diagnostics: how long each hop takes, measured on demand. */
export function DiagnosticsTab() {
  const { machines } = useLiveData();
  const [controlId, setControlId] = useState<string | null>(null);
  const [results, setResults] = useState<Record<string, Measurement>>({});
  const [running, setRunning] = useState(false);
  const [ranAt, setRanAt] = useState<Date | null>(null);

  useEffect(() => {
    let cancelled = false;
    dataAdapter.registry
      .list()
      .then((rows) => {
        if (!cancelled) setControlId(rows.find((row) => row.role === "CONTROL")?.machine_id ?? null);
      })
      .catch(() => {
        // Not a control: only this browser's round trip can be measured.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  async function measure(machine: Machine): Promise<Measurement> {
    try {
      const browser = await dataAdapter.diagnostics.ping(machine.id);
      if (machine.id === controlId || controlId === null) return { browser };
      return { browser, control: await dataAdapter.diagnostics.controlPing(machine.id) };
    } catch (failure) {
      return { error: failure instanceof Error ? failure.message : String(failure) };
    }
  }

  async function run() {
    setRunning(true);
    setResults({});
    // One Machine at a time, so the samples do not compete with each other.
    for (const machine of machines) {
      const measurement = await measure(machine);
      setResults((previous) => ({ ...previous, [machine.id]: measurement }));
    }
    setRanAt(new Date());
    setRunning(false);
  }

  return (
    <>
      <header className={settings.tabHeader}>
        <div className={settings.tabHeaderText}>
          <h2 className={settings.tabTitle}>Diagnostics</h2>
          <span className={settings.tabSubtitle}>
            Round trips from this browser and from the control to every Machine, and how Tailscale connects them.
          </span>
        </div>
        <button type="button" className={settings.primaryBtn} disabled={running} onClick={() => void run()}>
          {running ? "Testing…" : "Run network test"}
        </button>
      </header>

      <div className={settings.tableScroll}>
        <table className={styles.table} aria-label="Network test">
          <thead>
            <tr>
              <th scope="col">Machine</th>
              <th scope="col">Status</th>
              <th scope="col">This browser →</th>
              <th scope="col">Control →</th>
              <th scope="col">Tailscale path</th>
              <th scope="col">Note</th>
            </tr>
          </thead>
          <tbody>
            {machines.map((machine) => {
              const isControl = machine.id === controlId;
              const measurement = results[machine.id];
              return (
                <tr key={machine.id}>
                  <th scope="row">
                    <span className={styles.name}>{machine.name}</span>
                    {isControl && <span className={styles.role}>Control</span>}
                  </th>
                  <td>
                    {machine.status === "ONLINE" ? "Online" : machine.status.toLowerCase().replace("_", " ")}
                    {machine.lastSeen ? <span className={styles.muted}> · seen {machine.lastSeen} ago</span> : null}
                  </td>
                  <td>
                    <RoundTrip result={measurement?.browser} />
                  </td>
                  <td>{isControl ? <span className={styles.muted}>—</span> : <RoundTrip result={measurement?.control} />}</td>
                  <td>{isControl ? "—" : pathLabel(measurement?.control)}</td>
                  <td className={styles.note}>{note(machine, isControl, measurement)}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <p className={styles.footer}>
        {ranAt
          ? `Measured ${ranAt.toLocaleTimeString()}. Each value is the median of 5 round trips; the first may include connection setup.`
          : "Nothing is measured until you run the test."}{" "}
        The browser → worker figure goes through the control, so it is what a terminal on that worker feels like from here.
      </p>
    </>
  );
}
