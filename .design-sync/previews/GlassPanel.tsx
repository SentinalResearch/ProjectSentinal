import type { ReactNode } from "react";
import { CaptureBadge, GlassPanel, StateTag } from "sentinal-ui";

// The preview card forces a white page; restore the page background that global.css sets (dark by default).
document.body.style.background = "var(--bg-0)";

const Backdrop = ({ children }: { children: ReactNode }) => (
  <div
    style={{
      padding: "var(--space-8)",
      background: "radial-gradient(circle at 70% 0%, var(--glow-accent), transparent 60%), var(--bg-app)",
    }}
  >
    {children}
  </div>
);

const sessions = [
  { name: "train.py", state: "RUNNING", detail: "epoch 37/50 · 2h 14m" },
  { name: "claude", state: "WAITING", detail: "asked 4m ago" },
  { name: "pnpm build", state: "FAILED", detail: "exit 1 · 12m ago" },
] as const;

export const MachineLane = () => (
  <Backdrop>
    <GlassPanel aria-label="studio" style={{ width: 380, padding: "var(--space-4)", display: "grid", gap: "var(--space-3)" }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline" }}>
        <h3 style={{ fontSize: 16, fontWeight: 700 }}>studio</h3>
        <span style={{ color: "var(--text-3)", fontSize: 12 }}>macOS · 3 Sessions</span>
      </div>
      <hr className="rule" />
      {sessions.map((s) => (
        <div key={s.name} style={{ display: "flex", alignItems: "center", gap: "var(--space-3)" }}>
          <StateTag state={s.state} size="sm" />
          <span className="mono" style={{ fontSize: 13, flex: 1 }}>
            {s.name}
          </span>
          <span style={{ color: "var(--text-3)", fontSize: 12 }}>{s.detail}</span>
        </div>
      ))}
      <div>
        <CaptureBadge capture="FULL" origin="WATCH" />
      </div>
    </GlassPanel>
  </Backdrop>
);

export const MetricTiles = () => (
  <Backdrop>
    <div style={{ display: "flex", gap: "var(--space-3)" }}>
      {[
        { label: "GPU", value: "92%", note: "RTX 4090 · 71°C" },
        { label: "Memory", value: "41.2 GB", note: "of 64 GB" },
        { label: "Loss", value: "0.184", note: "improving" },
      ].map((tile) => (
        <GlassPanel key={tile.label} tile as="div" style={{ width: 150, padding: "var(--space-4)", display: "grid", gap: "var(--space-1)" }}>
          <span className="mono" style={{ color: "var(--text-3)", fontSize: 11, textTransform: "uppercase", letterSpacing: "0.06em" }}>
            {tile.label}
          </span>
          <span style={{ fontSize: 24, fontWeight: 700 }}>{tile.value}</span>
          <span style={{ color: "var(--text-2)", fontSize: 12 }}>{tile.note}</span>
        </GlassPanel>
      ))}
    </div>
  </Backdrop>
);
