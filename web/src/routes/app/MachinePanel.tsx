import { useEffect, useState } from "react";

import { dataAdapter } from "../../data";
import type { Machine, MachineInsight, MachineMetricSample, MetricRangeKey, Session } from "../../data/types";
import type { SessionOrigin } from "../../protocol/SessionOrigin";
import styles from "./MachinePanel.module.css";
import { MetricsChart } from "./MetricsChart";

const DETAILS_REFRESH_MS = 5_000;
const HISTORY_REFRESH_MS = 30_000;
const FINISHED = new Set(["COMPLETED", "FAILED", "INTERRUPTED"]);

const RANGES: { key: MetricRangeKey; label: string }[] = [
  { key: "HOUR", label: "1h" },
  { key: "DAY", label: "24h" },
  { key: "WEEK", label: "7d" },
];

const ORIGIN_LABEL: Record<SessionOrigin, string> = {
  UI: "Sentinal terminal",
  WATCH: "sentinal watch",
  SHELL_HOOK: "Shell hook",
  AUTO_WRAP: "Auto-wrapped shell",
  ADOPTED: "Adopted process",
};

function since(at: number | undefined, now: number): string {
  if (at === undefined) return "—";
  const seconds = Math.max(0, Math.round((now - at) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  return hours < 48 ? `${hours}h ${minutes % 60}m` : `${Math.floor(hours / 24)}d ${hours % 24}h`;
}

function gigabytes(bytes: number): string {
  return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
}

/**
 * The single-Machine view under a filtered Overview: host and daemon facts,
 * Machine history, capture sources, and per-Session processes. Each refresh
 * keeps the previous data on screen until the new reply arrives.
 */
export function MachinePanel({ machine, sessions }: { machine: Machine; sessions: Session[] }) {
  const [insight, setInsight] = useState<MachineInsight | null>(null);
  const [detailsError, setDetailsError] = useState<string | null>(null);
  const [range, setRange] = useState<MetricRangeKey>("HOUR");
  const [samples, setSamples] = useState<{ range: MetricRangeKey; points: MachineMetricSample[] } | null>(null);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    let cancelled = false;
    const load = () => {
      dataAdapter.machineInsights.details(machine.id).then(
        (next) => {
          if (cancelled) return;
          setInsight(next);
          setDetailsError(null);
          setNow(Date.now());
        },
        (error: unknown) => {
          if (!cancelled) setDetailsError(error instanceof Error ? error.message : "Machine details unavailable.");
        },
      );
    };
    load();
    const timer = setInterval(load, DETAILS_REFRESH_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [machine.id]);

  useEffect(() => {
    let cancelled = false;
    const load = () => {
      dataAdapter.machineInsights.metrics(machine.id, range).then(
        (points) => {
          if (!cancelled) setSamples({ range, points });
        },
        () => {
          if (!cancelled) setSamples((current) => current ?? { range, points: [] });
        },
      );
    };
    load();
    const timer = setInterval(load, HISTORY_REFRESH_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [machine.id, range]);

  const live = sessions.filter((session) => !FINISHED.has(session.state));
  const processes = (insight?.processes ?? []).filter((entry) => entry.processes.length > 0);

  return (
    <div className={styles.panel}>
      {detailsError && !insight && (
        <p className={styles.notice} role="alert">
          {detailsError}
        </p>
      )}
      <div className={styles.cards}>
        <section className={styles.card} aria-label="System">
          <h2 className={styles.cardTitle}>System</h2>
          <dl className={styles.facts}>
            <dt>Hostname</dt>
            <dd>{insight?.hostname || "—"}</dd>
            <dt>OS</dt>
            <dd>{insight?.osVersion ?? machine.os}</dd>
            <dt>Kernel</dt>
            <dd>{insight?.kernelVersion ?? "—"}</dd>
            <dt>Hardware</dt>
            <dd>{machine.hardware}</dd>
          </dl>
        </section>
        <section className={styles.card} aria-label="Sentinal daemon">
          <h2 className={styles.cardTitle}>Sentinal daemon</h2>
          <dl className={styles.facts}>
            <dt>Version</dt>
            <dd className="mono">{insight?.daemonVersion ?? "—"}</dd>
            <dt>Uptime</dt>
            <dd>{insight ? since(insight.startedAt, now) : "—"}</dd>
            <dt>Listening</dt>
            <dd className="mono">{insight?.bind ?? machine.endpoint ?? "—"}</dd>
            <dt>Link</dt>
            <dd>
              {!insight?.link || insight.link.state === "NOT_CONNECTED"
                ? "Not linked to a control"
                : `${insight.link.state}${insight.link.lastHeartbeatAt ? ` · heartbeat ${since(insight.link.lastHeartbeatAt, now)} ago` : ""}`}
            </dd>
          </dl>
        </section>
      </div>

      <section className={styles.card} aria-label="Metrics">
        <div className={styles.cardHead}>
          <h2 className={styles.cardTitle}>Machine load</h2>
          <div className={styles.rangeGroup}>
            {RANGES.map((option) => (
              <button
                key={option.key}
                type="button"
                className={styles.rangeButton}
                aria-pressed={range === option.key}
                onClick={() => setRange(option.key)}
              >
                {option.label}
              </button>
            ))}
          </div>
        </div>
        {samples ? (
          <MetricsChart samples={samples.points} range={samples.range} />
        ) : (
          <div className={styles.chartEmpty}>Loading history…</div>
        )}
      </section>

      <section className={styles.card} aria-label="Capture sources">
        <h2 className={styles.cardTitle}>Capture sources</h2>
        {live.length === 0 ? (
          <p className={styles.muted}>Nothing is being watched on this Machine right now.</p>
        ) : (
          <ul className={styles.list}>
            {live.map((session) => (
              <li key={session.id} className={styles.listRow}>
                <span className={styles.listName}>{session.name}</span>
                <span className={styles.muted}>{ORIGIN_LABEL[session.origin ?? "UI"]}</span>
                <span className="mono">{session.capture ?? "FULL"}</span>
                <span className={styles.muted}>{session.state}</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className={styles.card} aria-label="Processes">
        <h2 className={styles.cardTitle}>Processes</h2>
        {processes.length === 0 ? (
          <p className={styles.muted}>No sampled Session processes.</p>
        ) : (
          processes.map((entry) => (
            <div key={entry.sessionId} className={styles.processGroup}>
              <span className={styles.listName}>
                {sessions.find((session) => session.id === entry.sessionId)?.name ?? entry.sessionId}
              </span>
              <ul className={styles.list}>
                {entry.processes.map((process) => (
                  <li key={process.pid} className={styles.listRow}>
                    <span>{process.name}</span>
                    <span className="mono">{process.pid}</span>
                    <span className="mono">{process.cpuPct.toFixed(1)} % CPU</span>
                    <span className="mono">{gigabytes(process.rssBytes)}</span>
                  </li>
                ))}
              </ul>
            </div>
          ))
        )}
      </section>
    </div>
  );
}
