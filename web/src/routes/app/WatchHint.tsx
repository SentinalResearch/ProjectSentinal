import { CopyButton } from "../../components/CopyButton";
import styles from "./ReducedCapturePanel.module.css";

/** Copyable `sentinal watch` line so the next run gets FULL capture. */
export function WatchHint({ command }: { command: string }) {
  return (
    <div className={styles.watchHint} aria-label="Watch fully next time">
      <span className={styles.overline}>WATCH FULLY NEXT TIME</span>
      <code className={`${styles.watchCommand} mono`}>{command}</code>
      <CopyButton text={command} label="Copy" />
    </div>
  );
}
