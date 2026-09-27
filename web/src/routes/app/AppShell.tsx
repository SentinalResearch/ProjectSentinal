import { useEffect, useRef, useState } from "react";
import { Outlet, useNavigate } from "react-router-dom";

import { LiveDataProvider } from "../../data/LiveDataContext";
import { Sidebar } from "./Sidebar";
import styles from "./AppShell.module.css";

const COLLAPSE_BREAKPOINT = 1200;

/**
 * The shared app shell: sidebar + routed main area (design_handoff_sentinal/design/SentinalApp.dc.html).
 * Owns sidebar collapse (auto-collapses under 1200px, but a manual toggle sticks
 * until the breakpoint is crossed again — matches the prototype) and the global
 * ⌘K / Ctrl-K → History shortcut.
 */
export function AppShell() {
  const navigate = useNavigate();
  const [collapsed, setCollapsed] = useState(() => window.innerWidth < COLLAPSE_BREAKPOINT);
  const narrowRef = useRef(window.innerWidth < COLLAPSE_BREAKPOINT);

  useEffect(() => {
    function onResize() {
      const narrow = window.innerWidth < COLLAPSE_BREAKPOINT;
      if (narrow !== narrowRef.current) {
        narrowRef.current = narrow;
        setCollapsed(narrow);
      }
    }
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  useEffect(() => {
    // QUESTION: navigate-only, per the prototype's actual handler — no command-palette
    // overlay is specified anywhere in the handoff. See QUESTIONS.md #8.
    function onKeyDown(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        navigate("/app/history");
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [navigate]);

  return (
    <LiveDataProvider>
      <div className={styles.shell}>
        <Sidebar expanded={!collapsed} onToggle={() => setCollapsed((c) => !c)} />
        <main className={styles.main}>
          <Outlet />
        </main>
      </div>
    </LiveDataProvider>
  );
}
