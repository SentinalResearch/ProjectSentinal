import type { SessionLineage } from "../../data/lineage";
import { resumeTag } from "../../terminal/resumeTags";
import styles from "./ResumeHistory.module.css";

/**
 * The details-side list of every start and resume (design H1), newest first.
 * Each row scrolls the one terminal to where that part begins.
 */
export function ResumeHistory({
  lineage,
  onJump,
}: {
  lineage: SessionLineage;
  onJump: (sessionId: string) => void;
}) {
  const parts = lineage.ordered;
  if (parts.length < 2) return null;
  const rows = parts
    .map((part, index) => {
      if (index === 0) return { id: part.id, when: resumeTag(part, part, 0, parts.length).when, text: "Started" };
      const tag = resumeTag(parts[index - 1], part, index, parts.length);
      return { id: part.id, when: tag.when, text: `Resumed · ${tag.before}` };
    })
    .reverse();
  return (
    <section className={styles.history} aria-label="Terminal history">
      <div className={styles.head}>
        <span className={styles.title}>History</span>
        <span className={styles.count}>
          {parts.length} parts · resumed {parts.length - 1} {parts.length === 2 ? "time" : "times"}
        </span>
      </div>
      {rows.map((row) => (
        <button key={row.id} type="button" className={styles.row} onClick={() => onJump(row.id)}>
          <span className={styles.when}>{row.when}</span>
          <span>{row.text}</span>
        </button>
      ))}
    </section>
  );
}
