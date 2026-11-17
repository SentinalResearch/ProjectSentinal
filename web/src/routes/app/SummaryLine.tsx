import type { SessionSummary } from "../../data/types";
import styles from "./SummaryLine.module.css";

/**
 * What a terminal is doing, in one line (design L4). A guess from the
 * intelligence layer wears the dashed ring and says so; a known summary
 * does not.
 */
export function SummaryLine({ summary }: { summary: SessionSummary }) {
  return (
    <div className={styles.line} data-inferred={summary.inferred || undefined}>
      {summary.inferred && <span className={styles.ring} aria-hidden="true" />}
      <span className={styles.body}>
        <span className={styles.text}>{summary.text}</span>
        {summary.inferred && <span className={styles.note}>Sentinal thinks · read off the screen</span>}
      </span>
    </div>
  );
}
