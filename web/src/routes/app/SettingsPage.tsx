import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";

import { MockBadge } from "../../components/MockBadge";
import { isMockData } from "../../data";
import { AccountTab } from "./AccountTab";
import { DiagnosticsTab } from "./DiagnosticsTab";
import { MachinesTab } from "./MachinesTab";
import { NotificationsTab } from "./NotificationsTab";
import { SimulationTab } from "./SimulationTab";
import { settingsTabs, type SettingsTabKey } from "./settings-tabs";
import styles from "./SettingsPage.module.css";
import { useIsPhone } from "../../hooks/useViewport";
import { BackBar } from "./phone/BackBar";
import { PhoneSettingsHome } from "./phone/SettingsHome";
import phone from "./phone/phone.module.css";

const TABS = settingsTabs(isMockData);

function normalizeTab(tab: string | undefined): SettingsTabKey {
  return TABS.some((t) => t.key === tab) ? (tab as SettingsTabKey) : "machines";
}

/** design_handoff_sentinal/design/SentinalSettings.dc.html */
export function SettingsPage() {
  const { tab } = useParams<{ tab?: string }>();
  const navigate = useNavigate();
  const active = normalizeTab(tab);
  const isPhone = useIsPhone();

  // On phones bare /app/settings is the Settings home (E1, reached from the
  // Menu); each Settings screen backs out to it.
  if (isPhone) {
    if (!tab) return <PhoneSettingsHome />;
    return (
      <>
        <BackBar parentLabel="Settings" parentPath="/app/settings" />
        <div className={phone.page}>
          {active === "machines" && <MachinesTab />}
          {active === "notifications" && <NotificationsTab />}
          {active === "diagnostics" && <DiagnosticsTab />}
          {active === "account" && <AccountTab />}
          {active === "simulation" && isMockData && <SimulationTab />}
        </div>
      </>
    );
  }

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
          {active === "machines" && <MachinesTab />}
          {active === "notifications" && <NotificationsTab />}
          {active === "diagnostics" && <DiagnosticsTab />}
          {active === "account" && <AccountTab />}
          {active === "simulation" && isMockData && <SimulationTab />}
        </div>
      </section>

    </div>
  );
}
