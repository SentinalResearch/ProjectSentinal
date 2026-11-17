import { useOptionalTheme } from "../context/ThemeContext";
import styles from "./ThemeSwitch.module.css";

/**
 * One tap between light and dark, for the island. Shows the theme you would
 * switch to (a sun in dark, a moon in light). Renders nothing without a
 * ThemeProvider.
 */
export function ThemeSwitch({ className }: { className?: string }) {
  const theme = useOptionalTheme();
  if (!theme) return null;
  const next = theme.theme === "dark" ? "light" : "dark";
  return (
    <button
      type="button"
      className={`${styles.switch} ${className ?? ""}`}
      onClick={theme.toggleTheme}
      aria-label={`Switch to ${next} theme`}
      title={`Switch to ${next} theme`}
    >
      {next === "light" ? (
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true">
          <circle cx="12" cy="12" r="4" />
          <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
        </svg>
      ) : (
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z" />
        </svg>
      )}
    </button>
  );
}
