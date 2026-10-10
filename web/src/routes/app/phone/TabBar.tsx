import { Link, useLocation } from "react-router-dom";

import styles from "./phone.module.css";

const TABS = [
  { label: "Overview", glyph: "▦", path: "/app/overview", owns: ["/app/overview", "/app/sessions", "/app/projects"] },
  { label: "Terminals", glyph: "❐", path: "/app/split", owns: ["/app/split"] },
  { label: "History", glyph: "↺", path: "/app/history", owns: ["/app/history"] },
  { label: "Recipes", glyph: "≡", path: "/app/recipes", owns: ["/app/recipes"] },
  { label: "More", glyph: "⋯", path: "/app/more", owns: ["/app/more", "/app/settings"] },
] as const;

/** The phone's bottom tab bar (MOBILE_SPEC.md → Navigation). */
export function TabBar() {
  const { pathname } = useLocation();
  const active = TABS.find((tab) => tab.owns.some((prefix) => pathname === prefix || pathname.startsWith(`${prefix}/`)))
    ?? (pathname === "/app" ? TABS[0] : undefined);
  return (
    <nav className={styles.tabBar} aria-label="Tabs">
      {TABS.map((tab) => {
        const current = tab === active;
        return (
          <Link key={tab.path} to={tab.path} className={styles.tab} aria-current={current ? "page" : undefined}>
            <span className={styles.tabGlyph} aria-hidden="true">{tab.glyph}</span>
            <span>{tab.label}</span>
          </Link>
        );
      })}
    </nav>
  );
}
