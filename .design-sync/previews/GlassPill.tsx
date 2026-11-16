import type { ReactNode } from "react";
import { DevBadge, GlassPill, SentinalMark, StateTag } from "sentinal-ui";

// The preview card forces a white page; restore the page background that global.css sets (dark by default).
document.body.style.background = "var(--bg-0)";

/** Glass needs something behind it to blur: the app's deep background with an accent glow. */
const Backdrop = ({ children }: { children: ReactNode }) => (
  <div
    style={{
      padding: "var(--space-8)",
      background: "radial-gradient(circle at 30% 0%, var(--glow-accent), transparent 60%), var(--bg-app)",
    }}
  >
    {children}
  </div>
);

const island = {
  display: "inline-flex",
  alignItems: "center",
  gap: "var(--space-3)",
  height: "var(--island-height)",
  padding: "0 var(--space-5)",
} as const;

export const Island = () => (
  <Backdrop>
    <GlassPill as="nav" style={island} aria-label="Sentinal">
      <SentinalMark size={20} />
      <span style={{ fontWeight: 700 }}>Sentinal</span>
      <DevBadge dev />
      <span style={{ color: "var(--text-2)", fontSize: 13 }}>3 Machines · 7 Sessions</span>
      <StateTag state="WAITING" size="sm" label="2 WAITING" />
    </GlassPill>
  </Backdrop>
);

export const Strong = () => (
  <Backdrop>
    <GlassPill strong style={{ ...island, width: 420 }}>
      <span className="mono" style={{ color: "var(--text-3)" }}>
        ›
      </span>
      <span style={{ color: "var(--text-2)" }}>Ask about any Session…</span>
    </GlassPill>
  </Backdrop>
);
