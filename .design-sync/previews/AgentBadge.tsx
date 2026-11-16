import { AgentBadge } from "sentinal-ui";

// The preview card forces a white page; restore the page background that global.css sets (dark by default).
document.body.style.background = "var(--bg-0)";

const row = { display: "flex", gap: "var(--space-3)", alignItems: "center", flexWrap: "wrap" } as const;

export const Working = () => (
  <div style={row}>
    <AgentBadge agent={{ agent: "CLAUDE", working: true, toolUses: 14, tokens: 48200, costUsd: 0.86 }} />
  </div>
);

export const Idle = () => (
  <div style={row}>
    <AgentBadge agent={{ agent: "CLAUDE", working: false, toolUses: 14, tokens: 48200, costUsd: 0.86 }} />
  </div>
);

export const WithUsage = () => (
  <div style={row}>
    <AgentBadge usage agent={{ agent: "CODEX", working: true, toolUses: 31, tokens: 127400, costUsd: 2.14 }} />
  </div>
);

export const Agents = () => (
  <div style={row}>
    {(["CLAUDE", "CODEX", "AIDER", "GEMINI"] as const).map((agent) => (
      <AgentBadge key={agent} agent={{ agent, working: false, toolUses: 3 }} />
    ))}
  </div>
);
