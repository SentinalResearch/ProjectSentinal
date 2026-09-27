import type { CSSProperties, ReactNode } from "react";

import type { SessionState } from "../protocol/SessionState";
import styles from "./StateTag.module.css";

export type TagState = SessionState;

export interface StateTagProps {
  state: TagState;
  size?: "md" | "sm";
  variant?: "solid" | "line";
  label?: string;
  /** Hide the text label; the glyph alone carries the state (pair with an adjacent label column). */
  iconOnly?: boolean;
}

const GLYPHS: Record<TagState, ReactNode> = {
  RUNNING: (
    <>
      <circle className={styles.pulse} cx="7" cy="7" r="6.5" strokeWidth="1.5" />
      <circle cx="7" cy="7" r="4" fill="currentColor" stroke="none" />
    </>
  ),
  CREATED: <path d="M7 2.5v9M2.5 7h9" />,
  IDLE: <path d="M2.5 7h9" />,
  WAITING: (
    <>
      <path d="M4.6 4.9a2.4 2.4 0 1 1 3.4 2.2c-.7.4-1 .8-1 1.5v.4" />
      <rect x="6.1" y="10.6" width="1.8" height="1.8" fill="currentColor" stroke="none" />
    </>
  ),
  STALLED: <path d="M4.8 3v8M9.2 3v8" />,
  COMPLETED: <path d="M2.6 7.2l3 3 5.8-6.4" />,
  FAILED: <path d="M3 3l8 8M11 3l-8 8" />,
  INTERRUPTED: <rect x="2.5" y="2.5" width="9" height="9" fill="currentColor" stroke="none" />,
  DISCONNECTED: (
    <>
      <circle cx="7" cy="7" r="5.5" strokeDasharray="2.2 2" />
      <path d="M3.5 10.5l7-7" />
    </>
  ),
};

/** State is never color-only: solid fill (or line/outline) + glyph + uppercase label. */
export function StateTag({ state, size = "md", variant = "solid", label, iconOnly = false }: StateTagProps) {
  const s = state.toLowerCase();
  const isDisconnected = state === "DISCONNECTED";
  const solid = variant === "solid" && !isDisconnected;
  const sm = size === "sm";

  const style: CSSProperties = {
    height: sm ? "20px" : "26px",
    padding: variant === "line" ? "0" : sm ? "0 7px 0 6px" : "0 10px 0 8px",
    background: solid ? `var(--s-${s})` : "transparent",
    color: solid ? "var(--ink)" : `var(--s-${s}-fg)`,
    boxShadow: isDisconnected && variant !== "line" ? `inset 0 0 0 1.5px var(--s-${s}-fg)` : "none",
    fontSize: sm ? "10.5px" : "12px",
    gap: sm ? "5px" : "7px",
  };

  const icon = sm ? 11 : 13;

  return (
    <span className={styles.tag} style={style}>
      <svg
        width={icon}
        height={icon}
        viewBox="0 0 14 14"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="square"
        strokeLinejoin="miter"
        className={styles.icon}
        aria-hidden="true"
      >
        {GLYPHS[state]}
      </svg>
      {!iconOnly && <span>{label ?? state}</span>}
    </span>
  );
}
