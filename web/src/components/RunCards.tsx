import { Link } from "react-router-dom";

import { shortDuration } from "../data/agentState";
import { estimateShort } from "../data/estimate";
import { FINISHED_STATES } from "../data/format";
import { partFailed, runGroups, type RunMember } from "../data/groups";
import type { Session } from "../data/types";
import { useSessionRemoval } from "../routes/app/useSessionRemoval";
import styles from "./RunCards.module.css";

/**
 * Grouped Sessions as one card per run (`sentinal watch --group`), like a
 * Live Activity: overall progress, what each part is doing, time left, and
 * one tap to hide what has finished.
 */
export function RunCards({ sessions }: { sessions: Session[] }) {
  const removal = useSessionRemoval();
  const groups = runGroups(sessions);
  if (groups.length === 0) return null;
  return (
    <section className={styles.runs} aria-label="Runs">
      {groups.map((group) => {
        const state = group.failed > 0 ? "failed" : group.waiting > 0 ? "waiting" : group.running > 0 ? "running" : "done";
        const counts = [
          group.running > 0 && `${group.running} running`,
          group.waiting > 0 && `${group.waiting} waiting`,
          group.failed > 0 && `${group.failed} failed`,
          group.finished.length > 0 && `${group.finished.length} done`,
        ].filter(Boolean);
        return (
          <article key={group.name} className={styles.card} data-state={state}>
            <header className={styles.head}>
              <span className={styles.dot} aria-hidden="true" />
              {group.lead ? (
                <Link className={styles.name} to={`/app/sessions/${group.lead.session.id}`}>
                  {group.name}
                </Link>
              ) : (
                <span className={styles.name}>{group.name}</span>
              )}
              {group.percent != null && <span className={styles.percent}>{group.percent}%</span>}
            </header>
            {group.percent != null && (
              <span className={styles.track} aria-hidden="true">
                <span className={styles.fill} style={{ width: `${group.percent}%` }} />
              </span>
            )}
            <p className={styles.summary}>
              {[counts.join(" · ") || "done", group.lead && estimateShort(group.lead.session.estimate), !group.lead && group.leftSecs != null && `longest ${shortDuration(group.leftSecs)} left`]
                .filter(Boolean)
                .join(" · ")}
            </p>
            {group.live.length > 0 && (
              <ul className={styles.members}>
                {group.live.map((entry) => (
                  <Member key={entry.session.id} entry={entry} />
                ))}
              </ul>
            )}
            {group.finished.filter(partFailed).map((session) => (
              <Link key={session.id} className={styles.failed} to={`/app/sessions/${session.id}`}>
                Failed · {(session.group ?? "").split("/").slice(1).join(" / ") || session.name}
              </Link>
            ))}
            {/* Only ended Sessions hide; a part still at its prompt stays open. */}
            {hideable(group.finished).length > 0 && (
              <button
                type="button"
                className={styles.hide}
                onClick={() => void removal.hide(hideable(group.finished).map((session) => session.id))}
              >
                Hide {hideable(group.finished).length} finished
              </button>
            )}
          </article>
        );
      })}
    </section>
  );
}

function hideable(sessions: Session[]): Session[] {
  return sessions.filter((session) => FINISHED_STATES.has(session.state));
}

function Member({ entry }: { entry: RunMember }) {
  const left = estimateShort(entry.session.estimate);
  const task = entry.session.agentState?.task;
  return (
    <li>
      <Link className={styles.member} to={`/app/sessions/${entry.session.id}`} data-state={entry.session.state}>
        <span className={styles.memberLabel}>{entry.label}</span>
        <span className={styles.memberFact}>
          {[entry.percent != null && `${entry.percent}%`, left, task && task !== entry.label ? task : undefined]
            .filter(Boolean)
            .join(" · ")}
        </span>
        {entry.percent != null && (
          <span className={styles.memberTrack} aria-hidden="true">
            <span className={styles.fill} style={{ width: `${entry.percent}%` }} />
          </span>
        )}
      </Link>
    </li>
  );
}
