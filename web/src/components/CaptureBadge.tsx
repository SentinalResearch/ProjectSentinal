import type { CaptureLevel } from "../protocol/CaptureLevel";
import { captureExplanation } from "../data/capture";
import styles from "./CaptureBadge.module.css";

export interface CaptureBadgeProps {
  capture: CaptureLevel;
}

/** Outline badge naming what Sentinal can observe; the tooltip says what is unavailable. */
export function CaptureBadge({ capture }: CaptureBadgeProps) {
  const explanation = captureExplanation(capture) ?? "FULL capture: output, input, Commands, and Metrics.";
  return (
    <span className={styles.badge} title={explanation} aria-label={`${capture} capture`}>
      {capture.replace("_", " ")}
    </span>
  );
}
