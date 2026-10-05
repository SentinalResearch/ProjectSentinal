import { useEffect, useState } from "react";

import { dataAdapter } from "../../data";
import { useLiveData } from "../../data/LiveDataContext";
import type { JournalNotification, NotificationDeliveryStatus, NotificationRuleSet, NotificationRules } from "../../data/types";
import styles from "./SettingsPage.module.css";

const KINDS: (keyof NotificationRuleSet)[] = ["finished", "error", "stalled", "waiting", "checkpoint", "progress"];

/** Mirrors `NotificationRuleSet::default()`, used until a Machine reports its own default. */
const BASE_DEFAULT: NotificationRuleSet = { finished: true, error: true, stalled: true, waiting: true, checkpoint: false, progress: false };

/** Preview cards use a red/yellow inset keyed by the notification text's own content — the two examples in fixtures.json are a failure and a waiting-for-input prompt, same mapping the prototype hardcodes but derived instead of hardcoded per-string. */
function previewAccent(text: string): string {
  if (/fail/i.test(text)) return "var(--s-failed)";
  if (/waiting/i.test(text)) return "var(--s-waiting)";
  return "var(--line-strong)";
}

export function NotificationsTab() {
  return <NotificationRules />;
}

function NotificationRules() {
  const { sessions, machines } = useLiveData();
  const [rules, setRules] = useState<NotificationRules | null>(null);
  const [journal, setJournal] = useState<JournalNotification[]>([]);
  const [statuses, setStatuses] = useState<NotificationDeliveryStatus[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    dataAdapter.notifications.getRules().then(setRules);
    dataAdapter.notifications.getJournal().then(setJournal);
    dataAdapter.notifications.getStatus().then(setStatuses);
  }, []);

  if (!rules) return <div className={styles.tabLoading}>Loading…</div>;

  /** What the owning daemon applies: the Session override, else that Machine's default. */
  function ruleFor(id: string, machineId: string): NotificationRuleSet {
    return rules?.[id] ?? rules?.[`${machineId}:default`] ?? BASE_DEFAULT;
  }

  async function toggle(id: string, machineId: string, kind: keyof NotificationRuleSet) {
    const previous = rules?.[id];
    const current = ruleFor(id, machineId);
    const next = { ...current, [kind]: !current[kind] };
    setError(null);
    setRules((prev) => ({ ...(prev ?? {}), [id]: next }));
    try {
      await dataAdapter.notifications.setRule(id, next);
    } catch (failure) {
      setRules((prev) => {
        const restored = { ...(prev ?? {}) };
        if (previous) restored[id] = previous;
        else delete restored[id];
        return restored;
      });
      setError(failure instanceof Error ? failure.message : "The Machine refused the rule.");
    }
  }

  const rows: { id: string; name: string; machineId: string; machine: string; isDefault?: boolean }[] = [
    ...machines.map((machine) => ({
      id: `${machine.id}:default`,
      name: "Default for new sessions",
      machineId: machine.id,
      machine: machine.name,
      isDefault: true,
    })),
    ...sessions.map((s) => ({
      id: s.id,
      name: s.name,
      machineId: s.machine,
      machine: machines.find((m) => m.id === s.machine)?.name ?? s.machine,
    })),
  ];

  return (
    <>
      <header className={styles.tabHeader}>
        <div className={styles.tabHeaderText}>
          <h2 className={styles.tabTitle}>Notifications</h2>
          <span className={styles.tabSubtitle}>Choose which events send a push notification, per session.</span>
        </div>
      </header>

      <div className={styles.previewWrap} aria-live="polite">
        {statuses.map((status) => (
          <span key={status.machineId}>
            {machines.find((machine) => machine.id === status.machineId)?.name ?? status.machineId}: {status.configurationState === "INVALID"
              ? `invalid configuration${status.diagnostic ? ` — ${status.diagnostic}` : ""}`
              : status.configured
              ? `${status.server}/${status.topic}${status.tokenConfigured ? " · token configured" : ""}`
              : "notify.toml not configured"}
          </span>
        ))}
      </div>

      {error && (
        <p className={styles.rowError} role="alert">
          {error}
        </p>
      )}

      <div className={styles.tableScroll}>
        <div className={styles.notifHeadRow}>
          <span>SESSION</span>
          {KINDS.map((k) => (
            <span key={k} className={styles.notifHeadCell}>
              {k.toUpperCase()}
            </span>
          ))}
        </div>
        {rows.map((row) => {
          const rule = ruleFor(row.id, row.machineId);
          return (
            <div
              key={row.id}
              className={styles.notifRow}
              style={{ background: row.isDefault ? "var(--bg-1)" : "transparent" }}
            >
              <div className={styles.notifNameCol} style={{ paddingLeft: row.isDefault ? "12px" : "0" }}>
                <span className={styles.machineName}>{row.name}</span>
                {row.machine && <span className={styles.machineOs}>{row.machine}</span>}
              </div>
              {KINDS.map((k) => {
                const on = rule[k];
                return (
                  <button
                    key={k}
                    type="button"
                    aria-pressed={on}
                    aria-label={`${row.name} on ${row.machine}: ${k}`}
                    title={`${row.name}: ${k}`}
                    className={styles.notifCell}
                    onClick={() => void toggle(row.id, row.machineId, k)}
                  >
                    <span
                      className={styles.notifMark}
                      style={{
                        background: on ? "var(--text-1)" : "transparent",
                        boxShadow: `inset 0 0 0 1.5px ${on ? "var(--text-1)" : "var(--line-strong)"}`,
                      }}
                    >
                      {on ? "✓" : ""}
                    </span>
                  </button>
                );
              })}
            </div>
          );
        })}
      </div>

      <div className={styles.previewWrap}>
        <span className={styles.overline}>DELIVERY HISTORY &amp; PREVIEW</span>
        <p>Preview only — phone appearance is controlled by ntfy and your operating system.</p>
        <div className={styles.previewGrid}>
          {journal.map((n) => (
            <div
              key={n.id}
              className={styles.previewCard}
              role={n.state === "PERMANENTLY_FAILED" ? "alert" : "status"}
              style={{ boxShadow: `inset 3px 0 0 ${previewAccent(n.text)}` }}
            >
              <span className={styles.previewSender}>Sentinal</span>
              <span className={styles.previewText}>{n.text}</span>
              <span className={styles.machineOs}>
                {[n.category, n.state, n.attempts === undefined ? undefined : `${n.attempts} attempt${n.attempts === 1 ? "" : "s"}`]
                  .filter(Boolean)
                  .join(" · ")}
              </span>
            </div>
          ))}
          {journal.length === 0 && <p>No stored notification decisions yet.</p>}
        </div>
      </div>
    </>
  );
}
