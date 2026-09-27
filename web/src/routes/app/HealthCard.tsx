import { healthColorSlug } from "../../data/format";
import type { SessionHealth } from "../../data/types";
import styles from "./HealthCard.module.css";

export interface HealthCardProps {
  health: SessionHealth;
  /** An interrupted session's stale health snapshot still says e.g. "HEALTHY" — override the Overall row. */
  interrupted: boolean;
}

// See SentinalSession.dc.html's `cmap` — health row values are free-form strings
// colored by a lookup with a neutral (text-1) fallback for anything unrecognized.
const VALUE_COLORS: Record<string, string> = {
  GOOD: "var(--s-running-fg)",
  HEALTHY: "var(--s-running-fg)",
  NONE: "var(--text-1)",
  FAILED: "var(--s-failed-fg)",
  WAITING: "var(--s-waiting-fg)",
  PAUSED: "var(--s-waiting-fg)",
  STALLED: "var(--s-stalled-fg)",
  EXITED: "var(--text-2)",
};

function valueColor(value: string): string {
  if (VALUE_COLORS[value]) return VALUE_COLORS[value];
  if (/^\d+$/.test(value) && value !== "0") return "var(--s-failed-fg)"; // a bare non-zero count, e.g. Errors: "3"
  if (value.startsWith("NONE") || value.toUpperCase().includes("STALL")) return "var(--s-stalled-fg)";
  return "var(--text-1)";
}

/** 6 fixed rows (Progress/Activity/Resources/Stability/Errors/Overall) + evidence line. */
export function HealthCard({ health, interrupted }: HealthCardProps) {
  const overall = interrupted ? "INTERRUPTED" : health.overall;
  const slug = interrupted ? "interrupted" : healthColorSlug(health.overall);
  const fill = `var(--s-${slug})`;

  const rows: [string, string | undefined][] = [
    ["Progress", health.progress],
    ["Activity", health.activity],
    ["Resources", health.resources],
    ["Stability", health.stability],
    ["Errors", health.errors],
    ["Overall", overall],
  ];

  return (
    <div className={styles.card}>
      <div className={styles.headRow} style={{ background: fill }}>
        <span className={styles.headLabel}>HEALTH</span>
        <span className={styles.headValue}>{overall}</span>
      </div>
      {rows
        .filter(([, value]) => value !== undefined)
        .map(([label, value]) => (
          <div key={label} className={styles.row}>
            <span className={styles.rowLabel}>{label}</span>
            <span
              className={styles.rowValue}
              style={{ color: label === "Overall" && interrupted ? "var(--s-interrupted-fg)" : valueColor(value ?? "") }}
            >
              {value}
            </span>
          </div>
        ))}
      {health.evidence && health.evidence.length > 0 && (
        <span className={styles.evidence}>
          {interrupted ? "Ctrl‑C sent from this browser · process exited" : health.evidence.join(" · ")}
        </span>
      )}
    </div>
  );
}
