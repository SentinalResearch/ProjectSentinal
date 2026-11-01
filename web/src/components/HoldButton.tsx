import { useEffect, useRef, useState, type ReactNode } from "react";

import styles from "./HoldButton.module.css";

/** How long a hold must last (MOBILE_GLASS_SPEC §5). */
export const HOLD_MS = 600;

/**
 * A destructive action that fires only after a deliberate hold: pointer, or
 * Enter / Space held down. The fill shows progress; letting go early cancels.
 */
export function HoldButton({ children, onHeld, className }: { children: ReactNode; onHeld: () => void; className?: string }) {
  const [holding, setHolding] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  const fired = useRef(false);

  const stop = () => {
    window.clearTimeout(timer.current);
    timer.current = undefined;
    setHolding(false);
  };
  const start = () => {
    if (timer.current !== undefined) return;
    fired.current = false;
    setHolding(true);
    timer.current = window.setTimeout(() => {
      timer.current = undefined;
      fired.current = true;
      setHolding(false);
      onHeld();
    }, HOLD_MS);
  };
  useEffect(() => () => window.clearTimeout(timer.current), []);

  return (
    <button
      type="button"
      className={`${styles.button} ${className ?? ""}`}
      data-holding={holding || undefined}
      style={{ "--hold-ms": `${HOLD_MS}ms` } as React.CSSProperties}
      aria-description={`Hold for ${HOLD_MS / 1000} seconds`}
      onPointerDown={(event) => {
        event.preventDefault();
        start();
      }}
      onPointerUp={stop}
      onPointerLeave={stop}
      onPointerCancel={stop}
      onKeyDown={(event) => {
        if ((event.key === "Enter" || event.key === " ") && !event.repeat) {
          event.preventDefault();
          start();
        }
      }}
      onKeyUp={(event) => {
        if (event.key === "Enter" || event.key === " ") stop();
      }}
      // A plain click (no hold) never fires the action.
      onClick={(event) => event.preventDefault()}
    >
      <span className={styles.fill} aria-hidden="true" />
      <span className={styles.label}>{children}</span>
    </button>
  );
}
