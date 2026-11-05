import styles from "./DevBadge.module.css";

/** Whether this UI was started by `just dev`. */
export function isDevEnvironment(value: string | undefined = import.meta.env.VITE_SENTINAL_ENV): boolean {
  return value === "development";
}

/** The tab title for this environment, so production and dev cannot be confused. */
export function appTitle(dev: boolean = isDevEnvironment()): string {
  return dev ? "Sentinal DEV" : "Sentinal";
}

/** Shown in every island in the isolated dev environment; nothing in production. */
export function DevBadge({ dev = isDevEnvironment() }: { dev?: boolean }) {
  if (!dev) return null;
  return (
    <span className={styles.badge} title="Isolated development environment: production is not touched">
      DEV
    </span>
  );
}
