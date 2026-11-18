import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { Session } from "../data/types";
import { AgentStateChip, SandboxBadge } from "./AgentStateChip";

const base: Session = { id: "s", machine: "m", name: "claude · web", state: "RUNNING" };

describe("AgentStateChip", () => {
  it("says who reported it and what, marked as a report", () => {
    render(
      <AgentStateChip
        session={{
          ...base,
          codingAgent: { agent: "CLAUDE", working: true, toolUses: 3 },
          agentState: { status: "BLOCKED", detail: "needs approval for cargo test", reportedAt: 1 },
        }}
      />,
    );
    expect(screen.getByText("claude says · blocked · needs approval for cargo test")).toBeInTheDocument();
    expect(screen.getByTitle(/Reported by claude, not measured by Sentinal/)).toBeInTheDocument();
  });

  it("names an unknown reporter 'agent' and shows nothing without a report", () => {
    const { container, rerender } = render(<AgentStateChip session={{ ...base, agentState: { status: "DONE", reportedAt: 1 } }} />);
    expect(container).toHaveTextContent("agent says · done");
    rerender(<AgentStateChip session={base} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("explains a sandbox honestly", () => {
    render(<SandboxBadge session={{ ...base, sandbox: true }} />);
    expect(screen.getByTitle(/not a security boundary/)).toHaveTextContent("Sandbox");
  });
});
