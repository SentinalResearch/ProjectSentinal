import { useLocation, useNavigate, useSearchParams } from "react-router-dom";

import { SentinalMark } from "../../components/SentinalMark";
import { useAuth } from "../../context/AuthContext";
import { useTheme } from "../../context/ThemeContext";
import { needsAttention, slugifyProject } from "../../data/format";
import { useLiveData } from "../../data/LiveDataContext";
import { dataAdapter, isMockData } from "../../data";
import styles from "./Sidebar.module.css";

interface ActiveStyle {
  background: string;
  color: string;
  fontWeight: number;
}

function activeStyle(on: boolean): ActiveStyle {
  return { background: on ? "var(--bg-2)" : "transparent", color: on ? "var(--text-1)" : "var(--text-2)", fontWeight: on ? 700 : 500 };
}

const NAV_ITEMS = [
  { key: "overview", label: "Overview", glyph: "▦", path: "/app/overview" },
  { key: "split", label: "Split view", glyph: "◫", path: "/app/split" },
  { key: "history", label: "History", glyph: "↺", path: "/app/history" },
  { key: "recipes", label: "Recipes", glyph: "≡", path: "/app/recipes" },
] as const;

export interface SidebarProps {
  expanded: boolean;
  onToggle: () => void;
}

export function Sidebar({ expanded, onToggle }: SidebarProps) {
  const navigate = useNavigate();
  const location = useLocation();
  const [searchParams] = useSearchParams();
  const { machines, sessions } = useLiveData();
  const { theme, toggleTheme } = useTheme();
  const { logout } = useAuth();

  const machineFilter = location.pathname === "/app/overview" ? searchParams.get("machine") : null;
  const needsCount = sessions.filter(needsAttention).length;

  const projects = Array.from(new Set(sessions.map((s) => s.project).filter((p): p is string => Boolean(p)))).map(
    (name) => ({
      id: slugifyProject(name),
      name,
      count: sessions.filter((s) => s.project === name).length,
    }),
  );

  const projectMatch = location.pathname.match(/^\/app\/projects\/(.+)$/);
  const activeProjectId = projectMatch ? projectMatch[1] : null;

  async function newSession() {
    if (isMockData) {
      navigate("/app/recipes");
      return;
    }
    const session = await dataAdapter.createSession();
    navigate(`/app/sessions/${session.id}`);
  }

  return (
    <aside className={styles.aside} style={{ width: expanded ? "264px" : "57px" }}>
      <div className={styles.top}>
        <button
          type="button"
          className={styles.markButton}
          title={expanded ? "Overview" : "Expand sidebar"}
          onClick={() => (expanded ? navigate("/app/overview") : onToggle())}
        >
          <SentinalMark size={20} />
          {expanded && <span className={styles.wordmark}>Sentinal</span>}
        </button>
        {expanded && (
          <button type="button" className={styles.iconButton} title="Toggle sidebar" onClick={onToggle}>
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5">
              <rect x="1.75" y="2.75" width="12.5" height="10.5" />
              <path d="M6 3v10" />
            </svg>
          </button>
        )}
      </div>

      <div className={styles.actions}>
        <button type="button" className={styles.newSession} onClick={() => void newSession()}>
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" style={{ flex: "none" }}>
            <path d="M8 3v10M3 8h10" />
          </svg>
          {expanded && <span>New session</span>}
        </button>
        <button type="button" className={styles.searchHistory} onClick={() => navigate("/app/history")}>
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" style={{ flex: "none" }}>
            <circle cx="7" cy="7" r="4.5" />
            <path d="M10.5 10.5l3 3" />
          </svg>
          {expanded && (
            <>
              <span className={styles.searchHistoryLabel}>Search history</span>
              <span className={styles.kbdHint}>⌘K</span>
            </>
          )}
        </button>
      </div>

      <nav className={styles.nav}>
        {NAV_ITEMS.map((item) => {
          const isOverview = item.key === "overview";
          const active = location.pathname === item.path && !(isOverview && machineFilter);
          const a = activeStyle(active);
          const badge = isOverview ? needsCount : 0;
          return (
            <button
              key={item.key}
              type="button"
              title={item.label}
              className={styles.navItem}
              style={a}
              onClick={() => navigate(item.path)}
            >
              <span className={styles.glyph}>{item.glyph}</span>
              {expanded && (
                <>
                  <span className={styles.navLabel}>{item.label}</span>
                  {badge > 0 && <span className={styles.badge}>{badge}</span>}
                </>
              )}
            </button>
          );
        })}

        {expanded && (
          <>
            <div className={styles.sectionLabel}>MACHINES</div>
            {machines.map((m) => {
              const active = machineFilter === m.id;
              const a = activeStyle(active);
              const dimmed = m.status === "OFFLINE" && !active;
              return (
                <button
                  key={m.id}
                  type="button"
                  className={styles.machineRow}
                  style={{ ...a, color: dimmed ? "var(--text-3)" : a.color }}
                  onClick={() => navigate(`/app/overview?machine=${m.id}`)}
                >
                  <span
                    className={styles.machineDot}
                    style={{
                      background: m.status === "ONLINE" ? "var(--s-running)" : "transparent",
                      boxShadow: m.status === "ONLINE" ? "none" : "inset 0 0 0 1.5px var(--s-disconnected-fg)",
                    }}
                  />
                  <span className={styles.rowLabel}>{m.name}</span>
                  <span className={styles.rowMeta}>
                    {m.status === "ONLINE" ? sessions.filter((s) => s.machine === m.id).length : "offline"}
                  </span>
                </button>
              );
            })}

            {projects.length > 0 && (
              <>
                <div className={styles.sectionLabel}>PROJECTS</div>
                {projects.map((p) => {
                  const active = activeProjectId === p.id;
                  const a = activeStyle(active);
                  return (
                    <button
                      key={p.id}
                      type="button"
                      className={styles.projectRow}
                      style={a}
                      onClick={() => navigate(`/app/projects/${p.id}`)}
                    >
                      <span className={styles.projectHash}>#</span>
                      <span className={styles.rowLabel}>{p.name}</span>
                      <span className={styles.rowMeta}>{p.count}</span>
                    </button>
                  );
                })}
              </>
            )}
          </>
        )}
      </nav>

      <div className={styles.bottom}>
        {(() => {
          const settingsActive = location.pathname.startsWith("/app/settings");
          const a = activeStyle(settingsActive);
          return (
            <button
              type="button"
              title="Settings"
              className={styles.navItem}
              style={a}
              onClick={() => navigate("/app/settings")}
            >
              <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" style={{ flex: "none" }}>
                <path d="M2 4.5h7M12 4.5h2M2 11.5h2M7 11.5h7" />
                <rect x="9" y="3" width="3" height="3" />
                <rect x="4" y="10" width="3" height="3" />
              </svg>
              {expanded && <span>Settings</span>}
            </button>
          );
        })()}

        <button type="button" title="Theme" className={styles.navItem} style={{ color: "var(--text-2)" }} onClick={toggleTheme}>
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" style={{ flex: "none" }}>
            <circle cx="8" cy="8" r="5.5" />
            <path d="M8 2.5a5.5 5.5 0 0 1 0 11z" fill="currentColor" />
          </svg>
          {expanded && <span>{theme === "light" ? "Dark theme" : "Light theme"}</span>}
        </button>

        <div className={styles.account}>
          <span className={styles.avatar}>A</span>
          {expanded && (
            <>
              <span className={styles.accountName}>admin</span>
              <button type="button" className={styles.signOut} onClick={() => void logout()}>
                Sign out
              </button>
            </>
          )}
        </div>
      </div>
    </aside>
  );
}
