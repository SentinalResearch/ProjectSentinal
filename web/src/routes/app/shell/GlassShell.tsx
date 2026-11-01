import { useCallback, useEffect, useMemo, type ReactNode } from "react";
import { useNavigate } from "react-router-dom";

import { LiveIsland } from "./LiveIsland";
import { ShellContext, type ShellValue } from "./ShellContext";
import styles from "./GlassShell.module.css";

/** True when a key press is aimed at a text field (so "/" must type, not open the prompt). */
export function typingInto(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  if (target.closest(".xterm")) return true;
  return ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
}

/**
 * Desktop (≥ 1024 px) shell: the Live island on top and the routed page below,
 * with no sidebar (GLASS_SPEC.md §4). ⌘K / Ctrl-K and "/" open the prompt.
 */
export function GlassShell({ children }: { children: ReactNode }) {
  const navigate = useNavigate();

  // Until the prompt lands, opening it keeps the previous ⌘K behaviour.
  const openPrompt = useCallback((_prefill?: string) => navigate("/app/history"), [navigate]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      const commandK = (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k";
      const slash = event.key === "/" && !event.metaKey && !event.ctrlKey && !event.altKey && !typingInto(event.target);
      if (commandK || slash) {
        event.preventDefault();
        openPrompt();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [openPrompt]);

  const value = useMemo<ShellValue>(() => ({ openPrompt }), [openPrompt]);

  return (
    <ShellContext.Provider value={value}>
      <div className={styles.shell}>
        <div className={styles.glow} aria-hidden="true" />
        <LiveIsland onOpenPrompt={openPrompt} />
        <main className={styles.main} id="main">
          {children}
        </main>
      </div>
    </ShellContext.Provider>
  );
}
