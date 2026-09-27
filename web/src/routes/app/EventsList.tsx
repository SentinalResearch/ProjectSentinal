import { eventColor } from "../../data/format";
import type { SessionEvent } from "../../data/types";
import styles from "./EventsList.module.css";

export interface EventsListProps {
  events: SessionEvent[];
  /**
   * Session name for each event, aligned by index with `events` (same order, not
   * reversed) — pass this on a multi-session view (e.g. Project's event journal,
   * "the session name next to each event") to show who the event belongs to.
   * Omit for a single-session view (e.g. Session Detail) where it'd be redundant.
   */
  sessionNames?: (string | undefined)[];
  title?: string;
  /** False when the parent page already renders its own section heading (e.g. Project's "Event journal"). */
  showHead?: boolean;
}

/** Newest first — time · color square · TYPE (· session name) · detail (README's Event row spec). */
export function EventsList({ events, sessionNames, title = "Events", showHead = true }: EventsListProps) {
  const combined = events.map((event, i) => ({ event, sessionName: sessionNames?.[i] }));
  const newestFirst = [...combined].reverse();
  return (
    <div className={styles.wrap}>
      {showHead && (
        <div className={styles.head}>
          <span className={styles.headTitle}>{title}</span>
          <span className={styles.headCount}>{events.length}</span>
        </div>
      )}
      {newestFirst.length === 0 ? (
        <span className={styles.empty}>No events yet.</span>
      ) : (
        newestFirst.map(({ event: [time, kind, detail], sessionName }, i) => {
          const color = eventColor(kind);
          return (
            <div key={`${time}-${kind}-${i}`} className={styles.row}>
              <span className={styles.time}>{time}</span>
              <span className={styles.square} style={{ background: color.square }} />
              <span className={styles.body}>
                <span className={styles.typeRow}>
                  <span className={styles.type} style={{ color: color.text }}>
                    {kind}
                  </span>
                  {sessionName && <span className={styles.who}>{sessionName}</span>}
                </span>
                <span className={styles.detail}>{detail}</span>
              </span>
            </div>
          );
        })
      )}
    </div>
  );
}
