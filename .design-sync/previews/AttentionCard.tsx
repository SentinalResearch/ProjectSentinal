import { AgentBadge, AttentionCard, PromptChoices, StateTag } from "sentinal-ui";

// The preview card forces a white page; restore the page background that global.css sets (dark by default).
document.body.style.background = "var(--bg-0)";

const card = { width: 380, padding: "var(--space-4)", display: "grid", gap: "var(--space-3)" } as const;
const top = { display: "flex", alignItems: "center", gap: "var(--space-2)" } as const;
const where = { marginLeft: "auto", color: "var(--text-3)", fontSize: 12 } as const;
const title = { fontSize: 16, fontWeight: 700 } as const;
const well = {
  padding: "var(--space-2) var(--space-3)",
  background: "var(--well)",
  borderRadius: "var(--radius-key)",
  fontFamily: "var(--font-mono)",
  fontSize: 13,
  color: "var(--text-2)",
} as const;

const waiting = {
  id: "studio:7",
  machine: "studio",
  name: "claude",
  state: "WAITING" as const,
  prompt: "Do you want to run cargo test --workspace?",
  promptOptions: [
    { label: "Yes", keys: "1" },
    { label: "Yes, and don't ask again for cargo test commands", keys: "2" },
    { label: "No, and tell Claude what to do differently", keys: "3" },
  ],
};

export const WaitingForAnAnswer = () => (
  <AttentionCard state="WAITING" style={card}>
    <div style={top}>
      <StateTag state="WAITING" size="sm" />
      <AgentBadge agent={{ agent: "CLAUDE", working: false, toolUses: 14 }} />
      <span style={where}>studio · 4m</span>
    </div>
    <h3 style={title}>Claude Code · sentinal</h3>
    <span style={well}>{waiting.prompt}</span>
    <PromptChoices session={waiting} />
  </AttentionCard>
);

export const Failed = () => (
  <AttentionCard state="FAILED" style={card}>
    <div style={top}>
      <StateTag state="FAILED" size="sm" />
      <span style={where}>gpu-box-01 · 12m ago</span>
    </div>
    <h3 style={title}>pnpm build</h3>
    <span style={well}>error TS2345: Argument of type 'string' is not assignable to parameter of type 'number'.</span>
  </AttentionCard>
);

export const StateTints = () => (
  <div style={{ display: "grid", gridTemplateColumns: "repeat(2, 180px)", gap: "var(--space-3)" }}>
    {(["RUNNING", "STALLED", "COMPLETED", "INTERRUPTED"] as const).map((state) => (
      <AttentionCard key={state} state={state} style={{ padding: "var(--space-3)", display: "grid", gap: "var(--space-2)" }}>
        <StateTag state={state} size="sm" />
        <span className="mono" style={{ fontSize: 13 }}>
          {state === "RUNNING" ? "train.py" : state === "STALLED" ? "rsync backup" : state === "COMPLETED" ? "cargo test" : "npm run dev"}
        </span>
      </AttentionCard>
    ))}
  </div>
);
