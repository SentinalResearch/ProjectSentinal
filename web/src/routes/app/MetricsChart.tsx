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

/**
 * Sentinal colours, not state colours (green and yellow mean "running" and
 * "waiting" elsewhere): blue and ink, told apart by line style as well.
 */
const SERIES: { key: SeriesKey; label: string; color: string; dash?: string; width: number }[] = [
  { key: "cpuPct", label: "CPU", color: "var(--accent)", width: 2 },
  { key: "ramPct", label: "RAM", color: "var(--text-1)", width: 1.5 },
  { key: "gpuPct", label: "GPU", color: "var(--accent)", dash: "6 4", width: 2 },
  { key: "vramPct", label: "VRAM", color: "var(--text-3)", dash: "1.5 4", width: 2 },
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
  const x = (at: number) => (last === first ? WIDTH / 2 : ((at - first) / (last - first)) * WIDTH);
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
            const seriesSamples = samples.filter((sample) => sample[series.key] !== undefined);
            const points = seriesSamples
              .map((sample) => `${x(sample.at).toFixed(1)},${y(sample[series.key] ?? 0).toFixed(1)}`)
              .join(" ");
            return (
              <g key={series.key}>
                <polyline
                  points={points}
                  fill="none"
                  stroke={series.color}
                  strokeWidth={series.width}
                  strokeDasharray={series.dash}
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  vectorEffect="non-scaling-stroke"
                />
                {/* One marker on the latest value: "now" (and the only mark when there is one sample). */}
                {seriesSamples.slice(-1).map((sample) => (
                  <circle key={sample.at} cx={x(sample.at)} cy={y(sample[series.key] ?? 0)} r="3" fill={series.color} />
                ))}
              </g>
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
                  <svg className={styles.legendSwatch} width="18" height="6" viewBox="0 0 18 6" aria-hidden="true">
                    <line x1="1" y1="3" x2="17" y2="3" stroke={series.color} strokeWidth={series.width} strokeDasharray={series.dash} strokeLinecap="round" />
                  </svg>
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
