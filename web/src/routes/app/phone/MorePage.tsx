import { Link, Navigate } from "react-router-dom";

import { useAuth } from "../../../context/AuthContext";
import { useTheme, type ThemePreference } from "../../../context/ThemeContext";
import { useLiveData } from "../../../data/LiveDataContext";
import { useIsPhone } from "../../../hooks/useViewport";
import styles from "./phone.module.css";

const THEMES: { value: ThemePreference; label: string }[] = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "system", label: "Auto" },
];

/** `/app/more` on phones (E1); desktop keeps its Settings page. */
export function MoreRoute() {
  return useIsPhone() ? <MorePage /> : <Navigate to="/app/settings" replace />;
}

/** E1 · More: settings entry points, theme, and sign out. */
export function MorePage() {
  const { machines } = useLiveData();
  const { preference, setPreference } = useTheme();
  const { logout } = useAuth();
  const online = machines.filter((machine) => machine.status === "ONLINE").length;

  return (
    <div className={styles.page}>
      <h1 className={styles.pageTitle}>More</h1>

      <section aria-label="Settings">
        <div className={styles.sectionHead}>Settings</div>
        <Link className={styles.linkRow} to="/app/settings/machines">
          <span className={styles.rowTitle}>Machines</span>
          <span className={styles.rowMeta}>
            {online} of {machines.length} online
          </span>
          <span className={styles.chevron} aria-hidden="true">›</span>
        </Link>
        <Link className={styles.linkRow} to="/app/settings/notifications">
          <span className={styles.rowTitle}>Notifications</span>
          <span className={styles.rowMeta}>Push and per-Session rules</span>
          <span className={styles.chevron} aria-hidden="true">›</span>
        </Link>
        <Link className={styles.linkRow} to="/app/settings/account">
          <span className={styles.rowTitle}>Account</span>
          <span className={styles.rowMeta}>Password and this browser</span>
          <span className={styles.chevron} aria-hidden="true">›</span>
        </Link>
      </section>

      <section aria-label="Appearance">
        <div className={styles.sectionHead}>Theme</div>
        <div className={styles.segmented} role="radiogroup" aria-label="Theme">
          {THEMES.map((option) => (
            <button
              key={option.value}
              type="button"
              role="radio"
              aria-checked={preference === option.value}
              className={styles.segment}
              onClick={() => setPreference(option.value)}
            >
              {option.label}
            </button>
          ))}
        </div>
      </section>

      <section aria-label="Signed in">
        <div className={styles.sectionHead}>Owner</div>
        <div className={styles.ownerRow}>
          <span className={styles.rowTitle}>Signed in on this browser</span>
          <button type="button" className={styles.destructiveText} onClick={() => void logout()}>
            Sign out
          </button>
        </div>
      </section>
    </div>
  );
}
