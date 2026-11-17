import type { SessionState } from "../protocol/SessionState";

/** The glyph drawn inside the circle, one shape per state so state is never colour alone. */
const GLYPH: Record<SessionState, string> = {
  CREATED: "M12 8v8M8 12h8",
  IDLE: "M8 12h8",
  RUNNING: "",
  WAITING: "M9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .8-1 1.5v.4M12 17v.3",
  STALLED: "M10 9v6M14 9v6",
  COMPLETED: "M8 12.5l2.5 2.5L16 9.5",
  FAILED: "M9 9l6 6M15 9l-6 6",
  INTERRUPTED: "M9.5 9.5h5v5h-5z",
  DISCONNECTED: "",
};

/**
 * A quiet line icon for a Session state (the light dashboard's marker): a thin
 * circle in the state colour with the state's own glyph inside. Decorative —
 * the row's text names the state for assistive tech.
 */
export function StateIcon({ state, size = 22 }: { state: SessionState; size?: number }) {
  const colour = `var(--s-${state.toLowerCase()}-fg)`;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke={colour}
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      style={{ flex: "none", display: "block" }}
      data-state={state}
    >
      <circle cx="12" cy="12" r="9" strokeDasharray={state === "DISCONNECTED" ? "3 3" : undefined} />
      {state === "RUNNING" ? <circle cx="12" cy="12" r="3.5" fill={colour} stroke="none" /> : GLYPH[state] && <path d={GLYPH[state]} />}
    </svg>
  );
}
