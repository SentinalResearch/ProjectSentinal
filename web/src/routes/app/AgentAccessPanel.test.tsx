import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { InferredEvent } from "../../protocol/InferredEvent";
import { AgentAccessPanel, describeInferred } from "./AgentAccessPanel";

const agents = vi.hoisted(() => ({ outputAllowed: vi.fn(), setOutputAllowed: vi.fn(), inferred: vi.fn() }));
vi.mock("../../data", () => ({ dataAdapter: { agents } }));

const event = (fn: string, args: Record<string, unknown>): InferredEvent => ({
  id: `inf_${fn}`, machine_id: "rtx", session_id: 1, at: Date.now() - 120_000,
  agent_id: "agt_1", agent_name: "translator", function: fn, args,
});

beforeEach(() => {
  vi.clearAllMocks();
  agents.outputAllowed.mockResolvedValue(false);
  agents.setOutputAllowed.mockResolvedValue(undefined);
  agents.inferred.mockResolvedValue([]);
});

describe("AgentAccessPanel", () => {
  it("is off by default and the owner can allow reading", async () => {
    const user = userEvent.setup();
    render(<AgentAccessPanel sessionId="rtx:1" />);
    const toggle = await screen.findByRole("switch", { name: /Let agents read this terminal/ });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    await user.click(toggle);
    expect(agents.setOutputAllowed).toHaveBeenCalledWith("rtx:1", true);
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
  });

  it("lists inferred facts, marked as inferred and by whom", async () => {
    agents.outputAllowed.mockResolvedValue(true);
    agents.inferred.mockResolvedValue([event("progress_updated", { current: 12, total: 40, unit: "epoch" })]);
    render(<AgentAccessPanel sessionId="rtx:1" />);
    expect(await screen.findByText("epoch 12/40")).toBeInTheDocument();
    expect(screen.getByText(/inferred · via translator/)).toBeInTheDocument();
  });

  it("describes each screen function in plain words", () => {
    expect(describeInferred(event("metric_reported", { name: "val_loss", value: 0.37 }))).toBe("val_loss 0.37");
    expect(describeInferred(event("error_detected", { kind: "oom" }))).toBe("error: oom");
    expect(describeInferred(event("checkpoint_created", { path: "runs/a.pt" }))).toBe("checkpoint runs/a.pt");
    expect(describeInferred(event("tests_summary", { passed: 3, failed: 1, skipped: 0 }))).toBe("tests 3 passed, 1 failed, 0 skipped");
    expect(describeInferred(event("something_new", {}))).toBe("something new");
  });
});
