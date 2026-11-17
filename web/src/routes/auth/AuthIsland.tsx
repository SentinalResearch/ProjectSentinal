import type { ReactNode } from "react";

import { SentinalMark } from "../../components/SentinalMark";
import { useTheme } from "../../context/ThemeContext";
import { defaultDaemonHttpUrl } from "../../data/api";
import styles from "./AuthIsland.module.css";

/** Colour of the status square; every tone also carries text, never colour alone. */
export type IslandTone = "ok" | "pending" | "failed" | "waiting" | "off";

export interface AuthIslandProps {
  tone: IslandTone;
  /** Short status text after the server, e.g. "Signed out" or "4 tries left". */
  status: ReactNode;
  latencyMs?: number | null;
}

/** Host shown in the island: the daemon's host when it lives elsewhere, else this page's. */
export function serverHost(daemonUrl: string = defaultDaemonHttpUrl(), pageHost: string = window.location.host): string {
  try {
    if (/^https?:\/\//.test(daemonUrl)) return new URL(daemonUrl).host;
  } catch {
    // Fall through to the page host.
  }
  return pageHost;
}

/**
 * The pre-sign-in island: the same black Dynamic Island as the app, showing only
 * the server and the sign-in state. Nothing about machines or sessions is
 * shown until the user has signed in.
 */
export function AuthIsland({ tone, status, latencyMs = null }: AuthIslandProps) {
  const { theme, toggleTheme } = useTheme();
  return (
    <header className={`${styles.island} island-ink`} aria-label="Sentinal status">
      <span className={styles.brand}>
        <SentinalMark size={18} />
        <span className={styles.name}>Sentinal</span>
      </span>
      <span className={styles.divider} aria-hidden="true" />
      <span className={styles.server}>
        <span className={`${styles.square} ${styles[tone]} ${tone === "pending" ? "tg-motion" : ""}`} aria-hidden="true" />
        <span className={styles.host}>{serverHost()}</span>
        {latencyMs !== null && <span className={styles.latency}>{latencyMs} ms</span>}
      </span>
      <span className={styles.divider} aria-hidden="true" />
      <span className={`${styles.status} ${styles[`${tone}Text`]}`} role="status">
        {status}
      </span>
      <button type="button" className={styles.theme} onClick={toggleTheme}>
        {theme === "light" ? "Dark theme" : "Light theme"}
      </button>
    </header>
  );
}
