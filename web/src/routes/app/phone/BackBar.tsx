import { useEffect, useRef, type ReactNode } from "react";
import { useLocation, useNavigate } from "react-router-dom";

import styles from "./phone.module.css";

export interface BackBarProps {
  parentLabel: string;
  parentPath: string;
  /** Optional trailing control, e.g. a ⋯ menu button. */
  trailing?: ReactNode;
}

/** A back swipe starts this close to the left edge… */
const EDGE_PX = 24;
/** …travels at least this far right… */
const MIN_TRAVEL_PX = 72;
/** …and drifts vertically less than this. */
const MAX_DRIFT_PX = 48;

/**
 * The 44 px detail-screen bar. Back returns to the previous in-app screen, or
 * to the parent after a deep link (a tapped push has no in-app history), so it
 * never leaves the app. A swipe from the left edge does the same.
 */
export function BackBar({ parentLabel, parentPath, trailing }: BackBarProps) {
  const navigate = useNavigate();
  const location = useLocation();
  const hasInAppHistory = location.key !== "default";
  const goBack = useRef<() => void>(() => undefined);
  goBack.current = () => (hasInAppHistory ? navigate(-1) : navigate(parentPath, { replace: true }));

  useEffect(() => {
    let start: { x: number; y: number } | null = null;
    function onStart(event: TouchEvent) {
      const touch = event.touches[0];
      start = touch && touch.clientX <= EDGE_PX ? { x: touch.clientX, y: touch.clientY } : null;
    }
    function onEnd(event: TouchEvent) {
      const touch = event.changedTouches[0];
      if (start && touch && touch.clientX - start.x >= MIN_TRAVEL_PX && Math.abs(touch.clientY - start.y) < MAX_DRIFT_PX) {
        goBack.current();
      }
      start = null;
    }
    window.addEventListener("touchstart", onStart, { passive: true });
    window.addEventListener("touchend", onEnd, { passive: true });
    return () => {
      window.removeEventListener("touchstart", onStart);
      window.removeEventListener("touchend", onEnd);
    };
  }, []);

  return (
    <div className={styles.backBar}>
      <button type="button" className={styles.backButton} onClick={() => goBack.current()}>
        ‹ {parentLabel}
      </button>
      {trailing}
    </div>
  );
}
