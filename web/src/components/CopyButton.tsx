import { useEffect, useRef, useState } from "react";

import styles from "./CopyButton.module.css";

export interface CopyButtonProps {
  /** Text placed on the clipboard. */
  text: string;
  /** Idle label, e.g. "Copy code". */
  label: string;
  /** How long the confirmation stays visible. */
  resetMs?: number;
}

/**
 * Copies `text` and confirms with an animated check and "Copied to
 * clipboard", announced to screen readers; it reverts after `resetMs`.
 * A failed copy says so instead of pretending.
 */
export function CopyButton({ text, label, resetMs = 2000 }: CopyButtonProps) {
  const [state, setState] = useState<"idle" | "copied" | "failed">("idle");
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => () => {
    if (timer.current) clearTimeout(timer.current);
  }, []);

  async function copy() {
    if (timer.current) clearTimeout(timer.current);
    try {
      await navigator.clipboard.writeText(text);
      setState("copied");
    } catch {
      setState("failed");
    }
    timer.current = setTimeout(() => setState("idle"), resetMs);
  }

  return (
    <>
    <button
      type="button"
      className={`${styles.button} ${state === "copied" ? styles.copied : ""}`}
      aria-label={state === "copied" ? "Copied to clipboard" : label}
      onClick={() => void copy()}
    >
      {state === "copied" && (
        <svg className={styles.check} viewBox="0 0 16 16" aria-hidden="true">
          <path d="M3 8.5l3.2 3.2L13 4.8" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      )}
      <span key={state} className={state === "idle" ? undefined : styles.label}>
        {state === "copied" ? "Copied to clipboard" : state === "failed" ? "Copy failed — select and copy" : label}
      </span>
    </button>
    <span role="status" aria-live="polite" className="visually-hidden">
      {state === "copied" ? "Copied to clipboard" : ""}
    </span>
    </>
  );
}
