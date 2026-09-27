import styles from "./AuthMark.module.css";
import type { AuthMode } from "./copy";

export interface AuthMarkProps {
  mode: AuthMode;
  /** Sign-in/setup request in flight — the slash shows a scanning segment instead of a static draw. */
  busy?: boolean;
  /** Password accepted — the dot grows to fill the frame just before navigating away. */
  ok?: boolean;
  /** Retriggers the shake on each wrong-password attempt. */
  shaking?: boolean;
}

function dotColor(mode: AuthMode): string {
  if (mode === "wrong" || mode === "locked") return "var(--s-failed)";
  if (mode === "expired") return "var(--s-waiting)";
  return "var(--accent)";
}

/**
 * The animated Sentinal mark used on the auth screens: idle = blue dot with a
 * ping ring; checking = a segment scans along the slash; success = the dot
 * grows to fill the frame before routing away; failure = a shake + red dot.
 * A separate component from the static `SentinalMark` because the animation
 * states here are auth-specific and would otherwise bloat that shared primitive.
 */
export function AuthMark({ mode, busy = false, ok = false, shaking = false }: AuthMarkProps) {
  const dot = dotColor(mode);
  const pulsing = !busy && !ok && mode !== "locked";

  return (
    <div className={`${styles.wrap} tg-motion ${shaking ? styles.shaking : ""}`} aria-hidden="true">
      <svg viewBox="0 0 24 24" width={88} height={88} className={styles.svg}>
        <path
          className={`${styles.slash} tg-motion ${busy ? styles.scan : styles.draw}`}
          d="M5 22L13.5 2"
          pathLength={1}
        />
        <circle
          className={`${styles.ring} tg-motion ${pulsing ? styles.pulsing : ""}`}
          cx="18"
          cy="18.5"
          r="3.4"
          stroke={dot}
        />
        <circle
          className={`${styles.dot} tg-motion ${ok ? styles.success : ""}`}
          cx="18"
          cy="18.5"
          r="3.4"
          fill={dot}
        />
      </svg>
    </div>
  );
}
