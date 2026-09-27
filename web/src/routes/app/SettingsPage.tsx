import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";

import { MockBadge } from "../../components/MockBadge";
import { AccountTab } from "./AccountTab";
import { MachinesTab } from "./MachinesTab";
import { NotificationsTab } from "./NotificationsTab";
import { PairMachineDialog } from "./PairMachineDialog";
import styles from "./SettingsPage.module.css";

const TABS = [
  { key: "machines", label: "Machines & tokens" },
  { key: "notifications", label: "Notifications" },
  { key: "account", label: "Account" },
] as const;

type TabKey = (typeof TABS)[number]["key"];

function normalizeTab(tab: string | undefined): TabKey {
  return TABS.some((t) => t.key === tab) ? (tab as TabKey) : "machines";
}

/** design_handoff_sentinal/design/SentinalSettings.dc.html */
export function SettingsPage() {
  const { tab } = useParams<{ tab?: string }>();
  const navigate = useNavigate();
  const active = normalizeTab(tab);
  const [pairOpen, setPairOpen] = useState(false);

  return (
    <div className={styles.page}>
      <aside className={styles.aside}>
        <h1 className={styles.asideTitle}>Settings</h1>
        <div className={styles.asideDivider} />
        {TABS.map((t) => {
          const on = t.key === active;
          return (
            <button
              key={t.key}
              type="button"
              className={styles.tabBtn}
              style={{
                background: on ? "var(--bg-1)" : "transparent",
                color: on ? "var(--text-1)" : "var(--text-2)",
                fontWeight: on ? 700 : 500,
                boxShadow: on ? "inset 2px 0 0 var(--accent)" : "none",
              }}
              onClick={() => navigate(`/app/settings/${t.key}`)}
            >
              {t.label}
            </button>
          );
        })}
      </aside>

      <section className={styles.main}>
        <div className={styles.mainInner}>
          <MockBadge />
          {active === "machines" && <MachinesTab onOpenPair={() => setPairOpen(true)} />}
          {active === "notifications" && <NotificationsTab />}
          {active === "account" && <AccountTab />}
        </div>
      </section>

      <PairMachineDialog open={pairOpen} onClose={() => setPairOpen(false)} />
    </div>
  );
}
