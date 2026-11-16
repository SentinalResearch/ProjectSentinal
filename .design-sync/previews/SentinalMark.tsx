import { SentinalMark } from "sentinal-ui";

// The preview card forces a white page; restore the page background that global.css sets (dark by default).
document.body.style.background = "var(--bg-0)";

const row = { display: "flex", gap: "var(--space-5)", alignItems: "flex-end" } as const;

export const Sizes = () => (
  <div style={row}>
    <SentinalMark size={16} />
    <SentinalMark size={24} />
    <SentinalMark size={40} />
    <SentinalMark size={64} />
  </div>
);

export const Mono = () => (
  <div style={row}>
    <SentinalMark size={40} mono />
    <SentinalMark size={40} mono color="var(--text-2)" />
  </div>
);

export const Wordmark = () => (
  <div style={{ display: "flex", gap: "var(--space-3)", alignItems: "center" }}>
    <SentinalMark size={32} />
    <span style={{ fontSize: 24, fontWeight: 800, letterSpacing: "-0.01em" }}>Sentinal</span>
  </div>
);
