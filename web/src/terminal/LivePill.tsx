import type { Terminal } from "@xterm/xterm";
import { useEffect, useState } from "react";

import styles from "./ResumeTag.module.css";

/** Shown while the viewer is scrolled up: one tap back to the newest output. */
export function LivePill({ terminal }: { terminal: Terminal | null }) {
  const [scrolledUp, setScrolledUp] = useState(false);
  useEffect(() => {
    if (!terminal) return;
    const check = () => {
      const buffer = terminal.buffer.active;
      setScrolledUp(buffer.viewportY < buffer.baseY);
    };
    const scrolled = terminal.onScroll(check);
    const written = terminal.onWriteParsed(check);
    check();
    return () => {
      scrolled.dispose();
      written.dispose();
    };
  }, [terminal]);
  if (!terminal || !scrolledUp) return null;
  return (
    <button type="button" className={styles.livePill} onClick={() => terminal.scrollToBottom()}>
      ↓ Back to the latest
    </button>
  );
}
