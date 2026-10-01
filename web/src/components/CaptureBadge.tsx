import type { CaptureLevel } from "../protocol/CaptureLevel";
import type { SessionOrigin } from "../protocol/SessionOrigin";
import { captureExplanation, originLabel } from "../data/capture";
import styles from "./CaptureBadge.module.css";

export interface CaptureBadgeProps {
  capture: CaptureLevel;
  /** How the Session entered Sentinal; absent means UI. */
  origin?: SessionOrigin;
}

/** Outline badge naming a Session's origin and capture; the tooltip says what it captures. */
export function CaptureBadge({ capture, origin }: CaptureBadgeProps) {
  const label = originLabel(origin);
  return (
    <span
      className={styles.badge}
      title={`${label} · ${capture}: ${captureExplanation(capture)}`}
      aria-label={`${capture} capture`}
      data-origin={origin ?? "UI"}
    >
      <span className={styles.origin}>{label}</span>
      <span aria-hidden="true">·</span>
      {capture.replace("_", " ")}
    </span>
  );
}
