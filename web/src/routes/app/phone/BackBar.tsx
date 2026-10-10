import type { ReactNode } from "react";
import { useLocation, useNavigate } from "react-router-dom";

import styles from "./phone.module.css";

export interface BackBarProps {
  parentLabel: string;
  parentPath: string;
  /** Optional trailing control, e.g. a ⋯ menu button. */
  trailing?: ReactNode;
}

/**
 * The 44 px detail-screen bar. Back returns to the previous in-app screen, or
 * to the parent after a deep link (a tapped push has no in-app history), so it
 * never leaves the app.
 */
export function BackBar({ parentLabel, parentPath, trailing }: BackBarProps) {
  const navigate = useNavigate();
  const location = useLocation();
  const hasInAppHistory = location.key !== "default";
  return (
    <div className={styles.backBar}>
      <button
        type="button"
        className={styles.backButton}
        onClick={() => (hasInAppHistory ? navigate(-1) : navigate(parentPath, { replace: true }))}
      >
        ‹ {parentLabel}
      </button>
      {trailing}
    </div>
  );
}
