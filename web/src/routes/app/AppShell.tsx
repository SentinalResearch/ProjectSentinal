import { useEffect, useRef, useState } from "react";
import { Outlet, useNavigate } from "react-router-dom";

import { LiveDataProvider } from "../../data/LiveDataContext";
import { useIsGlassDesktop, useIsPhone } from "../../hooks/useViewport";
import { GlassShell } from "./shell/GlassShell";
import { PhoneShell } from "./phone/PhoneShell";
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
  const isPhone = useIsPhone();
  const isGlass = useIsGlassDesktop();
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
    // The glass shell owns ⌘K (it opens the prompt there).
    if (isGlass) return;
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
  }, [navigate, isGlass]);

  // Phones get the top bar and menu, never the sidebar or its icon rail.
  // LiveDataProvider stays above the switch so rotation keeps live data.
  if (isPhone) {
    return (
      <LiveDataProvider>
        <PhoneShell />
      </LiveDataProvider>
    );
  }

  // Desktop ≥ 1024 px: the island and prompt replace the sidebar.
  if (isGlass) {
    return (
      <LiveDataProvider>
        <GlassShell>
          <Outlet />
        </GlassShell>
      </LiveDataProvider>
    );
  }

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
