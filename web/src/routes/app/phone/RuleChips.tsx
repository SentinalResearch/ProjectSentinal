import type { NotificationRuleSet } from "../../../data/types";
import styles from "./phoneSettings.module.css";

const KINDS: { key: keyof NotificationRuleSet; label: string }[] = [
  { key: "finished", label: "Finished" },
  { key: "error", label: "Error" },
  { key: "stalled", label: "Stalled" },
  { key: "waiting", label: "Waiting" },
  { key: "checkpoint", label: "Checkpoint" },
  { key: "progress", label: "Progress" },
];

export interface RuleRow {
  id: string;
  name: string;
  machineId: string;
  machine: string;
  isDefault?: boolean;
}

/** E3 · per-Session notification rules as wrapping toggle chips (phones). */
export function RuleChips({
  rows,
  ruleFor,
  onToggle,
}: {
  rows: RuleRow[];
  ruleFor: (id: string, machineId: string) => NotificationRuleSet;
  onToggle: (id: string, machineId: string, kind: keyof NotificationRuleSet) => void;
}) {
  return (
    <section aria-label="Per Session rules">
      <h3 className={styles.overline}>PER SESSION · TAP TO TOGGLE</h3>
      {rows.map((row) => {
        const rule = ruleFor(row.id, row.machineId);
        return (
          <div key={row.id} className={styles.ruleRow} role="group" aria-label={`${row.name} on ${row.machine}`} data-default={row.isDefault ? "true" : undefined}>
            <span className={styles.ruleName}>{row.name}</span>
            <span className={styles.cardMeta}>{row.machine}</span>
            <div className={styles.chips}>
              {KINDS.map((kind) => (
                <button
                  key={kind.key}
                  type="button"
                  className={styles.chip}
                  aria-pressed={rule[kind.key]}
                  onClick={() => onToggle(row.id, row.machineId, kind.key)}
                >
                  {kind.label}
                </button>
              ))}
            </div>
          </div>
        );
      })}
    </section>
  );
}
