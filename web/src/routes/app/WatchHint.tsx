import { useState } from "react";

import styles from "./ReducedCapturePanel.module.css";

/** Copyable `sentinal watch` line so the next run gets FULL capture. */
export function WatchHint({ command }: { command: string }) {
  const [copied, setCopied] = useState(false);

  async function copy() {
    try {
      await navigator.clipboard.writeText(command);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  }

  return (
    <div className={styles.watchHint} aria-label="Watch fully next time">
      <span className={styles.overline}>WATCH FULLY NEXT TIME</span>
      <code className={`${styles.watchCommand} mono`}>{command}</code>
      <button type="button" className={styles.copyButton} onClick={() => void copy()}>
        {copied ? "Copied" : "Copy"}
      </button>
    </div>
  );
}
