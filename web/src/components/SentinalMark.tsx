/**
 * Sentinal logo mark — final variant `g` (slash + dot), viewBox 24.
 * See design_handoff_sentinal/assets/sentinal-mark-*.svg for the source artwork.
 */
export interface SentinalMarkProps {
  size?: number;
  /** Single-color rendering (dot uses currentColor instead of the accent). */
  mono?: boolean;
  color?: string;
  accent?: string;
}

export function SentinalMark({ size = 24, mono = false, color, accent }: SentinalMarkProps) {
  const ink = color ?? "var(--text-1)";
  const acc = mono ? "currentColor" : (accent ?? "var(--accent)");
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      style={{ display: "block", flex: "none", color: ink }}
      aria-label="Sentinal"
      role="img"
    >
      <path d="M5 22L13.5 2" stroke="currentColor" strokeWidth="3.4" strokeLinecap="square" />
      <circle cx="18" cy="18.5" r="3.4" fill={acc} />
    </svg>
  );
}
