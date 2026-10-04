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

/** Machine-wide percentages over time as one SVG line per resource. */
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
  const y = (percent: number) => HEIGHT - (Math.min(100, Math.max(0, percent)) / 100) * HEIGHT;
  const latest = samples[samples.length - 1];

  return (
    <figure className={styles.chart}>
      <svg
        role="img"
        aria-label={name}
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        preserveAspectRatio="none"
        className={styles.chartSvg}
      >
        {[25, 50, 75].map((percent) => (
          <line key={percent} x1={0} x2={WIDTH} y1={y(percent)} y2={y(percent)} className={styles.gridLine} />
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
      <figcaption className={styles.legend}>
        {present.map((series) => (
          <span key={series.key} className={styles.legendItem}>
            <span className={styles.legendSwatch} style={{ background: series.color }} />
            {series.label}
            <span className="mono">{latest[series.key] === undefined ? "—" : `${latest[series.key]} %`}</span>
          </span>
        ))}
      </figcaption>
    </figure>
  );
}
