import type { MachineMetricSample, MetricRangeKey } from "../../data/types";
import styles from "./MachinePanel.module.css";

const WIDTH = 640;
const HEIGHT = 160;

const RANGE_TEXT: Record<MetricRangeKey, string> = {
  HOUR: "the last hour",
  DAY: "the last 24 hours",
  WEEK: "the last 7 days",
};

type SeriesKey = "cpuPct" | "ramPct" | "gpuPct" | "vramPct";

const SERIES: { key: SeriesKey; label: string; color: string }[] = [
  { key: "cpuPct", label: "CPU", color: "var(--accent-fg)" },
  { key: "ramPct", label: "RAM", color: "var(--s-running-fg)" },
  { key: "gpuPct", label: "GPU", color: "var(--s-waiting-fg)" },
  { key: "vramPct", label: "VRAM", color: "var(--s-created-fg)" },
];

function timeLabel(at: number, range: MetricRangeKey): string {
  const date = new Date(at);
  return range === "WEEK"
    ? date.toLocaleDateString(undefined, { weekday: "short", day: "numeric" })
    : date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

function percent(value: number | undefined): string {
  return value === undefined ? "—" : `${Math.round(value * 10) / 10} %`;
}

/** Machine-wide percentages over time: one SVG line per resource, with a now/min/avg/max table beside it. */
export function MetricsChart({ samples, range }: { samples: MachineMetricSample[]; range: MetricRangeKey }) {
  const present = SERIES.filter((series) => samples.some((sample) => sample[series.key] !== undefined));
  const name = `${present.map((series) => series.label).join(", ") || "Metrics"} over ${RANGE_TEXT[range]}`;
  if (samples.length === 0) {
    return (
      <div className={styles.chartEmpty} role="img" aria-label={name}>
        No samples for {RANGE_TEXT[range]} yet.
      </div>
    );
  }
  const first = samples[0].at;
  const last = samples[samples.length - 1].at;
  const x = (at: number) => (last === first ? WIDTH : ((at - first) / (last - first)) * WIDTH);
  const y = (value: number) => HEIGHT - (Math.min(100, Math.max(0, value)) / 100) * HEIGHT;
  const latest = samples[samples.length - 1];

  return (
    <figure className={styles.chart}>
      <div className={styles.chartPlot}>
        <svg
          role="img"
          aria-label={name}
          viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
          preserveAspectRatio="none"
          className={styles.chartSvg}
        >
          {[25, 50, 75].map((level) => (
            <line key={level} x1={0} x2={WIDTH} y1={y(level)} y2={y(level)} className={styles.gridLine} />
          ))}
          {present.map((series) => {
            const points = samples
              .filter((sample) => sample[series.key] !== undefined)
              .map((sample) => `${x(sample.at).toFixed(1)},${y(sample[series.key] ?? 0).toFixed(1)}`)
              .join(" ");
            return (
              <polyline
                key={series.key}
                points={points}
                fill="none"
                stroke={series.color}
                strokeWidth={1.5}
                vectorEffect="non-scaling-stroke"
              />
            );
          })}
        </svg>
        <div className={styles.chartAxis}>
          <span>{timeLabel(first, range)}</span>
          <span>{timeLabel(last, range)}</span>
        </div>
      </div>
      <table className={styles.summaryTable} aria-label="Machine load summary">
        <thead>
          <tr>
            <th scope="col">Resource</th>
            <th scope="col">Now</th>
            <th scope="col">Min</th>
            <th scope="col">Avg</th>
            <th scope="col">Max</th>
          </tr>
        </thead>
        <tbody>
          {present.map((series) => {
            const values = samples.flatMap((sample) => {
              const value = sample[series.key];
              return value === undefined ? [] : [value];
            });
            const average = values.reduce((total, value) => total + value, 0) / values.length;
            return (
              <tr key={series.key}>
                <th scope="row">
                  <span className={styles.legendSwatch} style={{ background: series.color }} />
                  {series.label}
                </th>
                <td className="mono">{percent(latest[series.key])}</td>
                <td className="mono">{percent(Math.min(...values))}</td>
                <td className="mono">{percent(average)}</td>
                <td className="mono">{percent(Math.max(...values))}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </figure>
  );
}
