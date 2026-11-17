import type { HealthMetric } from "../../protocol/HealthMetric";
import type { SeriesPoint } from "../../protocol/SeriesPoint";
import { formatValue } from "./health-view";
import styles from "./HealthPanel.module.css";
import chart from "./MachinePanel.module.css";

const WIDTH = 640;
const HEIGHT = 120;

/**
 * Median and 95th percentile of one health metric over time. Same look as
 * `MetricsChart`, but scaled to the data (milliseconds or bytes, not 0–100 %).
 */
export function HealthChart({ title, metric, points }: { title: string; metric: HealthMetric; points: SeriesPoint[] }) {
  if (points.length === 0) {
    return (
      <div className={chart.chartEmpty} role="img" aria-label={title}>
        No samples in this range yet.
      </div>
    );
  }
  const first = points[0].at;
  const last = points[points.length - 1].at;
  const top = Math.max(...points.map((point) => point.p95)) * 1.1 || 1;
  const x = (at: number) => (last === first ? WIDTH / 2 : ((at - first) / (last - first)) * WIDTH);
  const y = (value: number) => HEIGHT - (Math.max(0, value) / top) * HEIGHT;
  // One sample has no time span: draw it as a level line across the plot instead of a single, invisible point.
  const line = (key: "p50" | "p95") =>
    last === first
      ? `0,${y(points[0][key]).toFixed(1)} ${WIDTH},${y(points[0][key]).toFixed(1)}`
      : points.map((point) => `${x(point.at).toFixed(1)},${y(point[key]).toFixed(1)}`).join(" ");
  const latest = points[points.length - 1];
  return (
    <figure className={chart.chart}>
      <div className={chart.chartPlot}>
        <svg role="img" aria-label={title} viewBox={`0 0 ${WIDTH} ${HEIGHT}`} preserveAspectRatio="none" className={chart.chartSvg}>
          <line x1="0" x2={WIDTH} y1={HEIGHT / 2} y2={HEIGHT / 2} className={chart.gridLine} vectorEffect="non-scaling-stroke" />
          <polyline points={line("p95")} fill="none" stroke="var(--text-3)" strokeWidth="1.5" strokeDasharray="5 4" strokeLinecap="round" vectorEffect="non-scaling-stroke" />
          <polyline points={line("p50")} fill="none" stroke="var(--accent)" strokeWidth="2" strokeLinecap="round" vectorEffect="non-scaling-stroke" />
          {/* A one-point polyline renders nothing; the markers keep single-sample series visible. */}
          {points.map((point) => (
            <circle key={point.at} cx={x(point.at)} cy={y(point.p50)} r="2.5" fill="var(--accent)" />
          ))}
        </svg>
      </div>
      <figcaption className={styles.legend}>
        <span>{title}</span>
        <span>
          <i className={styles.swatch} style={{ background: "var(--accent)" }} aria-hidden="true" />
          median {formatValue(metric, latest.p50)}
        </span>
        <span>
          <i className={styles.swatch} style={{ background: "repeating-linear-gradient(90deg, var(--text-3) 0 5px, transparent 5px 9px)" }} aria-hidden="true" />
          p95 {formatValue(metric, latest.p95)}
        </span>
        <span>top {formatValue(metric, top / 1.1)}</span>
      </figcaption>
    </figure>
  );
}
