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
  const x = (at: number) => (last === first ? WIDTH : ((at - first) / (last - first)) * WIDTH);
  const y = (value: number) => HEIGHT - (Math.max(0, value) / top) * HEIGHT;
  const line = (key: "p50" | "p95") => points.map((point) => `${x(point.at).toFixed(1)},${y(point[key]).toFixed(1)}`).join(" ");
  const latest = points[points.length - 1];
  return (
    <figure className={chart.chart}>
      <div className={chart.chartPlot}>
        <svg role="img" aria-label={title} viewBox={`0 0 ${WIDTH} ${HEIGHT}`} preserveAspectRatio="none" className={chart.chartSvg}>
          <polyline points={line("p95")} fill="none" stroke="var(--s-waiting-fg)" strokeWidth="1.5" vectorEffect="non-scaling-stroke" />
          <polyline points={line("p50")} fill="none" stroke="var(--accent-fg)" strokeWidth="2" vectorEffect="non-scaling-stroke" />
        </svg>
      </div>
      <figcaption className={styles.legend}>
        <span>{title}</span>
        <span>median {formatValue(metric, latest.p50)}</span>
        <span>p95 {formatValue(metric, latest.p95)}</span>
        <span>top {formatValue(metric, top / 1.1)}</span>
      </figcaption>
    </figure>
  );
}
