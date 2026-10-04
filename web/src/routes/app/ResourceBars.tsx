import type { SessionMetrics } from "../../data/types";
import styles from "./ResourceBars.module.css";

export function ResourceBars({ metrics }: { metrics: SessionMetrics | undefined }) {
  if (!metrics) return null;
  const rows = [
    metrics.cpuPct === undefined ? null : { label: "CPU", value: `${metrics.cpuPct} %`, percent: metrics.cpuPct },
    metrics.ramGB === undefined
      ? null
      : {
          label: "RAM",
          value: `${metrics.ramGB}${metrics.ramTotalGB === undefined ? "" : ` / ${metrics.ramTotalGB}`} GB`,
          percent: metrics.ramTotalGB ? (metrics.ramGB / metrics.ramTotalGB) * 100 : undefined,
        },
    metrics.gpuPct === undefined ? null : { label: "GPU", value: `${metrics.gpuPct} %`, percent: metrics.gpuPct },
    metrics.vramGB === undefined
      ? null
      : {
          label: "VRAM",
          value: `${metrics.vramGB}${metrics.vramTotalGB === undefined ? "" : ` / ${metrics.vramTotalGB}`} GB`,
          percent: metrics.vramTotalGB ? (metrics.vramGB / metrics.vramTotalGB) * 100 : undefined,
        },
  ].filter((row): row is NonNullable<typeof row> => row !== null);
  if (rows.length === 0) return null;

  return (
    <section className={styles.bars} aria-label="Live resources">
      {rows.map((row) => (
        <div className={styles.item} key={row.label}>
          <div className={styles.labelRow}>
            <span>{row.label}</span>
            <span className="mono">{row.value}</span>
          </div>
          {row.percent !== undefined && (
            <div className={styles.track}>
              <div className={styles.fill} style={{ width: `${Math.min(100, Math.max(0, row.percent))}%` }} />
            </div>
          )}
        </div>
      ))}
    </section>
  );
}
