import { DevBadge, SentinalMark } from "sentinal-ui";

// The preview card forces a white page; restore the page background that global.css sets (dark by default).
document.body.style.background = "var(--bg-0)";

export const Default = () => <DevBadge dev />;

export const BesideTheWordmark = () => (
  <div style={{ display: "flex", gap: "var(--space-2)", alignItems: "center" }}>
    <SentinalMark size={20} />
    <span style={{ fontWeight: 700, letterSpacing: "0.02em" }}>Sentinal</span>
    <DevBadge dev />
  </div>
);
