import { Link, Navigate } from "react-router-dom";

import { useAuth } from "../../../context/AuthContext";
import { useTheme, type ThemePreference } from "../../../context/ThemeContext";
import { useLiveData } from "../../../data/LiveDataContext";
import { projectsOf } from "./menuSearch";
import { PageTitle } from "./PageTitle";
import styles from "./phone.module.css";

const THEMES: { value: ThemePreference; label: string }[] = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "system", label: "Auto" },
];

/** The former `/app/more` (bookmarks, notifications) now lands on Settings. */
export function MoreRedirect() {
  return <Navigate to="/app/settings" replace />;
}

/** E1 · Settings (formerly More), opened from the Menu: Machines, Projects, settings, theme, sign out. */
export function PhoneSettingsHome() {
  const { machines, terminals: sessions } = useLiveData();
  const { preference, setPreference } = useTheme();
  const { logout } = useAuth();
  const online = machines.filter((machine) => machine.status === "ONLINE").length;
  const projects = projectsOf(sessions);

  return (
    <div className={styles.page}>
      <PageTitle>Settings</PageTitle>

      <section aria-label="Machines">
        <div className={styles.sectionHead}>Machines</div>
        {machines.map((machine) => (
          <Link key={machine.id} className={styles.linkRow} to={`/app/overview?machine=${encodeURIComponent(machine.id)}`}>
            <span className={styles.rowTitle}>{machine.name}</span>
            <span className={styles.rowMeta}>{machine.status === "ONLINE" ? "online" : machine.status.toLowerCase()}</span>
            <span className={styles.chevron} aria-hidden="true">›</span>
          </Link>
        ))}
      </section>

      {projects.length > 0 && (
        <section aria-label="Projects">
          <div className={styles.sectionHead}>Projects</div>
          {projects.map((project) => (
            <Link key={project.slug} className={styles.linkRow} to={`/app/projects/${project.slug}`}>
              <span className={styles.rowTitle}>{project.name}</span>
              <span className={styles.chevron} aria-hidden="true">›</span>
            </Link>
          ))}
        </section>
      )}

      <section aria-label="Settings">
        <div className={styles.sectionHead}>Settings</div>
        <Link className={styles.linkRow} to="/app/settings/machines">
          <span className={styles.rowTitle}>Machines &amp; tokens</span>
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
        <Link className={styles.linkRow} to="/app/settings/diagnostics">
          <span className={styles.rowTitle}>Diagnostics</span>
          <span className={styles.rowMeta}>Network test between devices</span>
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
