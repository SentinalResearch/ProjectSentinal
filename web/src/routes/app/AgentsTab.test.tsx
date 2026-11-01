import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentInfo } from "../../protocol/AgentInfo";
import { AgentsTab } from "./AgentsTab";

const calls = vi.hoisted(() => ({
  list: vi.fn(),
  create: vi.fn(),
  revoke: vi.fn(),
}));

vi.mock("../../data", () => ({
  dataAdapter: { agents: calls },
}));

const CLAUDE: AgentInfo = { id: "agt_3f9c1a2b4d5e6f70", name: "claude", level: "OBSERVE", created_at: Date.now() - 86_400_000 };
const TOKEN = `sat_agt_00aa11bb22cc33dd_${"f".repeat(64)}`;

beforeEach(() => {
  vi.clearAllMocks();
  calls.list.mockResolvedValue([CLAUDE]);
  calls.revoke.mockResolvedValue(undefined);
});

describe("AgentsTab", () => {
  it("lists agents with their level and never a token", async () => {
    render(<AgentsTab />);
    const row = await screen.findByRole("row", { name: /claude/ });
    expect(within(row).getByText("0 · Observe")).toBeInTheDocument();
    expect(document.body.textContent).not.toMatch(/sat_/);
  });

  it("offers levels 0 to 2 only", async () => {
    render(<AgentsTab />);
    const level = await screen.findByLabelText("Level");
    const options = within(level).getAllByRole("option").map((option) => option.textContent);
    expect(options).toEqual(["0 · Observe", "1 · Annotate", "2 · Suggest"]);
  });

  it("shows a new token once with the MCP command, then forgets it", async () => {
    const user = userEvent.setup();
    calls.create.mockResolvedValue({ agent: { ...CLAUDE, id: "agt_00aa11bb22cc33dd", name: "codex" }, token: TOKEN });
    render(<AgentsTab />);
    await user.type(await screen.findByLabelText("Name"), "codex");
    await user.selectOptions(screen.getByLabelText("Level"), "SUGGEST");
    await user.click(screen.getByRole("button", { name: "Create agent" }));

    expect(calls.create).toHaveBeenCalledWith("codex", "SUGGEST", false);
    const reveal = await screen.findByRole("region", { name: "New agent token" });
    expect(within(reveal).getByText(TOKEN)).toBeInTheDocument();
    expect(within(reveal).getByText(/sentinal mcp --control/)).toBeInTheDocument();
    expect(within(reveal).getByText(/shown only now/i)).toBeInTheDocument();

    await user.click(within(reveal).getByRole("button", { name: "I've saved it" }));
    expect(screen.queryByText(TOKEN)).not.toBeInTheDocument();
    expect(calls.list).toHaveBeenCalledTimes(2);
  });

  it("asks for terminal output access only when ticked", async () => {
    const user = userEvent.setup();
    calls.create.mockResolvedValue({ agent: { ...CLAUDE, name: "translator", output_access: true }, token: TOKEN });
    render(<AgentsTab />);
    await user.type(await screen.findByLabelText("Name"), "translator");
    await user.selectOptions(screen.getByLabelText("Level"), "ANNOTATE");
    await user.click(screen.getByRole("checkbox", { name: /Can read terminal output/ }));
    await user.click(screen.getByRole("button", { name: "Create agent" }));
    expect(calls.create).toHaveBeenCalledWith("translator", "ANNOTATE", true);
  });

  it("checks the name before asking the control", async () => {
    const user = userEvent.setup();
    render(<AgentsTab />);
    await user.type(await screen.findByLabelText("Name"), "rm -rf");
    await user.click(screen.getByRole("button", { name: "Create agent" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/letters, digits/);
    expect(calls.create).not.toHaveBeenCalled();
  });

  it("revokes only after confirming", async () => {
    const user = userEvent.setup();
    render(<AgentsTab />);
    const row = await screen.findByRole("row", { name: /claude/ });
    await user.click(within(row).getByRole("button", { name: "Revoke…" }));
    expect(calls.revoke).not.toHaveBeenCalled();
    await user.click(within(row).getByRole("button", { name: "Keep" }));
    await user.click(within(row).getByRole("button", { name: "Revoke…" }));
    await user.click(within(row).getByRole("button", { name: "Revoke" }));
    await waitFor(() => expect(calls.revoke).toHaveBeenCalledWith(CLAUDE.id));
  });

  it("explains when this Machine is not the control", async () => {
    calls.list.mockRejectedValue(Object.assign(new Error("this daemon is not a control Machine"), { code: "NOT_CONTROL" }));
    render(<AgentsTab />);
    expect(await screen.findByText(/only on the control Machine/i)).toBeInTheDocument();
  });
});
