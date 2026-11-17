import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";

import { formatSessionFact } from "../../../data/format";
import { inboxItems, lifecycle, type InboxGroup, type InboxItem, useLifecycle } from "../../../data/lifecycle";
import { useSessionRemoval } from "../useSessionRemoval";
import type { Session } from "../../../data/types";
import styles from "./LiveIsland.module.css";

type Filter = "all" | InboxGroup;

const FILTERS: Array<[Filter, string]> = [
  ["all", "All"],
  ["needs", "Needs you"],
  ["done", "Done"],
];

/** Unread notifications, for the island's blue count. */
export function useUnreadCount(terminals: Session[]): number {
  const life = useLifecycle();
  return inboxItems(terminals, life).filter((item) => item.unread).length;
}

/**
 * Design L3: notifications live in the island. Needs you first, then Done;
 * Open reads and goes there, ✕ clears the notification only, and a read Done
 * item offers to hide its terminal. Escape closes.
 */
export function IslandInbox({
  terminals,
  machineName,
  onClose,
}: {
  terminals: Session[];
  machineName: (id: string) => string;
  onClose: () => void;
}) {
  const navigate = useNavigate();
  const life = useLifecycle();
  const removal = useSessionRemoval();
  const [filter, setFilter] = useState<Filter>("all");
  const first = useRef<HTMLButtonElement>(null);
  const items = inboxItems(terminals, life);
  const shown = items.filter((item) => filter === "all" || item.group === filter);
  const unread = items.filter((item) => item.unread);

  useEffect(() => {
    first.current?.focus();
  }, []);

  const open = (item: InboxItem) => {
    lifecycle.markRead([item.id]);
    onClose();
    navigate(`/app/sessions/${encodeURIComponent(item.session.id)}`);
  };

  const groups: InboxGroup[] = filter === "all" ? ["needs", "done"] : [filter];

  return (
    <div
      className={styles.inbox}
      role="dialog"
      aria-label="Notifications"
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.stopPropagation();
          onClose();
        }
      }}
    >
      <div className={styles.inboxHead}>
        <span className={styles.inboxTitle}>
          Notifications {unread.length > 0 && <span className={styles.inboxNew}>{unread.length} new</span>}
        </span>
        <button
          ref={first}
          type="button"
          className={styles.inboxLink}
          disabled={unread.length === 0}
          onClick={() => lifecycle.markRead(unread.map((item) => item.id))}
        >
          Mark all read
        </button>
      </div>
      <div className={styles.inboxFilters} role="group" aria-label="Show">
        {FILTERS.map(([key, label]) => (
          <button key={key} type="button" className={styles.inboxFilter} aria-pressed={filter === key} onClick={() => setFilter(key)}>
            {label}
          </button>
        ))}
      </div>

      {shown.length === 0 && <p className={styles.inboxEmpty}>Nothing here. You're all caught up.</p>}
      {groups.map((group) => {
        const rows = shown.filter((item) => item.group === group);
        if (rows.length === 0) return null;
        return (
          <section key={group} aria-label={group === "needs" ? "Needs you" : "Done"}>
            <h3 className={styles.inboxGroup}>{group === "needs" ? "Needs you" : "Done"}</h3>
            <ul className={styles.inboxList}>
              {rows.map((item) => (
                <li key={item.id} className={styles.inboxRow} data-unread={item.unread || undefined}>
                  <span className={styles.inboxSquare} data-state={item.session.state} aria-hidden="true" />
                  <button type="button" className={styles.inboxText} onClick={() => open(item)}>
                    <span className={styles.inboxName}>
                      {item.session.name} {WORD[item.session.state]}
                      {item.unread && <span className={styles.visuallyHidden}> · new</span>}
                    </span>
                    <span className={styles.inboxMeta}>
                      {[machineName(item.session.machine), item.session.state === "WAITING" ? item.session.prompt : detail(item.session)]
                        .filter((part) => part && part !== "—")
                        .join(" · ")}
                    </span>
                  </button>
                  {item.group === "done" && !item.unread ? (
                    <button
                      type="button"
                      className={styles.inboxLink}
                      disabled={removal.busy}
                      onClick={() => void removal.hide([item.session.id])}
                    >
                      Hide terminal
                    </button>
                  ) : (
                    <button type="button" className={styles.inboxLink} onClick={() => open(item)}>
                      Open
                    </button>
                  )}
                  <button
                    type="button"
                    className={styles.inboxClear}
                    onClick={() => lifecycle.clear([item.id])}
                    aria-label={`Clear the notification for ${item.session.name}`}
                    title="Clear this notification (the terminal stays)"
                  >
                    ×
                  </button>
                </li>
              ))}
            </ul>
          </section>
        );
      })}
      {removal.error && (
        <p className={styles.inboxEmpty} role="alert">
          {removal.error}
        </p>
      )}
      <div className={styles.inboxFoot}>
        <span>Phone alerts follow your rules.</span>
        <button
          type="button"
          className={styles.inboxLink}
          onClick={() => {
            onClose();
            navigate("/app/settings/notifications");
          }}
        >
          Rules…
        </button>
      </div>
    </div>
  );
}

const WORD: Partial<Record<Session["state"], string>> = {
  WAITING: "asks",
  STALLED: "stalled",
  FAILED: "failed",
  COMPLETED: "finished",
  INTERRUPTED: "stopped",
};

function detail(session: Session): string {
  if (session.state === "FAILED") return session.result?.summary ?? formatSessionFact(session);
  if (session.state === "INTERRUPTED") return "stopped";
  return session.result?.summary ?? formatSessionFact(session);
}
