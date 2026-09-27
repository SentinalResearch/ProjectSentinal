import { useEffect, useState } from "react";

import { dataAdapter } from "../../data";
import { useLiveData } from "../../data/LiveDataContext";
import type { JournalNotification, NotificationRuleSet, NotificationRules } from "../../data/types";
import styles from "./SettingsPage.module.css";

const KINDS: (keyof NotificationRuleSet)[] = ["finished", "error", "stalled", "checkpoint", "progress"];
const DEFAULT_ID = "_default";

/** Base defaults mirrored from SentinalSettings.dc.html's `_default` rule row. */
const BASE_DEFAULT: NotificationRuleSet = { finished: true, error: true, stalled: true, checkpoint: false, progress: false };

/** Preview cards use a red/yellow inset keyed by the notification text's own content — the two examples in fixtures.json are a failure and a waiting-for-input prompt, same mapping the prototype hardcodes but derived instead of hardcoded per-string. */
function previewAccent(text: string): string {
  if (/fail/i.test(text)) return "var(--s-failed)";
  if (/waiting/i.test(text)) return "var(--s-waiting)";
  return "var(--line-strong)";
}

export function NotificationsTab() {
  const { sessions, machines } = useLiveData();
  const [rules, setRules] = useState<NotificationRules | null>(null);
  const [journal, setJournal] = useState<JournalNotification[]>([]);

  useEffect(() => {
    dataAdapter.notifications.getRules().then(setRules);
    dataAdapter.notifications.getJournal().then(setJournal);
  }, []);

  if (!rules) return <div className={styles.tabLoading}>Loading…</div>;

  function ruleFor(id: string): NotificationRuleSet {
    return rules?.[id] ?? { ...BASE_DEFAULT, checkpoint: sessions.find((s) => s.id === id)?.progress !== undefined };
  }

  function toggle(id: string, kind: keyof NotificationRuleSet) {
    const current = ruleFor(id);
    const next = { ...current, [kind]: !current[kind] };
    setRules((prev) => ({ ...(prev ?? {}), [id]: next }));
    void dataAdapter.notifications.setRule(id, next);
  }

  const rows: { id: string; name: string; machine: string; isDefault?: boolean }[] = [
    { id: DEFAULT_ID, name: "Default for new sessions", machine: "", isDefault: true },
    ...sessions.map((s) => ({ id: s.id, name: s.name, machine: machines.find((m) => m.id === s.machine)?.name ?? s.machine })),
  ];

  return (
    <>
      <header className={styles.tabHeader}>
        <div className={styles.tabHeaderText}>
          <h2 className={styles.tabTitle}>Notifications</h2>
          <span className={styles.tabSubtitle}>Choose which events send a push notification, per session.</span>
        </div>
      </header>

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
          const rule = ruleFor(row.id);
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
                    title={`${row.name}: ${k}`}
                    className={styles.notifCell}
                    onClick={() => toggle(row.id, k)}
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
        <span className={styles.overline}>PREVIEW</span>
        <div className={styles.previewGrid}>
          {journal.map((n) => (
            <div key={n.id} className={styles.previewCard} style={{ boxShadow: `inset 3px 0 0 ${previewAccent(n.text)}` }}>
              <span className={styles.previewSender}>Sentinal</span>
              <span className={styles.previewText}>{n.text}</span>
            </div>
          ))}
        </div>
      </div>
    </>
  );
}
