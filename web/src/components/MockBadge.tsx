import { isMockData } from "../data/mode";

const style = {
  display: "inline-flex",
  alignItems: "center",
  height: "16px",
  padding: "0 5px",
  border: "1px dashed var(--line-strong)",
  color: "var(--text-3)",
  fontFamily: "var(--font-mono)",
  fontSize: "11px",
  letterSpacing: "0.04em",
  flex: "none",
} as const;

/** Marks a feature backed by the mock adapter rather than a real API. Dev builds only. */
export function MockBadge() {
  if (!import.meta.env.DEV || !isMockData) return null;
  return (
    <span style={style} title="Backed by the mock data adapter">
      MOCK
    </span>
  );
}
