import { fireEvent, render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it } from "vitest";

import type { Machine, Session } from "../../data/types";
import { AgentsView, agentGroups } from "./AgentsPage";

const MIN = 60_000;
const machines: Machine[] = [
  { id: "mac", name: "Mac Mini", os: "macOS", hardware: "", status: "ONLINE" },
  { id: "rtx", name: "RTX-PC", os: "Linux", hardware: "", status: "ONLINE" },
];

const claude = { agent: "CLAUDE", working: true, toolUses: 1 } as const;

const sessions: Session[] = [
  {
    id: "a",
    machine: "mac",
    name: "claude · web",
    state: "RUNNING",
    project: "website",
    codingAgent: claude,
    agentState: {
      status: "WORKING",
      task: "migrate tests",
      step: 3,
      steps: 8,
      etaSecs: 720,
      estimatedLeftSecs: 900,
      taskStartedAt: 0,
      reportedAt: 0,
    },
  },
  {
    id: "b",
    machine: "mac",
    name: "codex · og",
    state: "WAITING",
    project: "website",
    prompt: "Run pnpm -s check? [y/N]",
    codingAgent: { agent: "CODEX", working: false, toolUses: 0 },
  },
  {
    id: "c",
    machine: "rtx",
    name: "eval sweep",
    state: "RUNNING",
    cwd: "~/training",
    agentState: { status: "DONE", detail: "all green", reportedAt: 0 },
  },
  { id: "d", machine: "rtx", name: "plain shell", state: "IDLE", project: "website" },
];

function view(filter = "all") {
  return render(
    <MemoryRouter>
      <AgentsView sessions={sessions} machines={machines} now={4 * MIN} filter={filter as never} onFilter={() => {}} />
    </MemoryRouter>,
  );
}

describe("agentGroups", () => {
  it("lists only agents, grouped by Project and then by folder", () => {
    const groups = agentGroups(sessions, "all");
    expect(groups.map((group) => [group.label, group.sessions.map((s) => s.id)])).toEqual([
      ["website", ["b", "a"]],
      ["~/training", ["c"]],
    ]);
  });

  it("filters to needs you, working and done", () => {
    const ids = (filter: Parameters<typeof agentGroups>[1]) => agentGroups(sessions, filter).flatMap((g) => g.sessions.map((s) => s.id));
    expect(ids("needs")).toEqual(["b"]);
    expect(ids("working")).toEqual(["a"]);
    expect(ids("done")).toEqual(["c"]);
  });

  it("counts a blocked agent as needing you", () => {
    const blocked: Session = { ...sessions[0], agentState: { status: "BLOCKED", detail: "needs the key", reportedAt: 0 } };
    expect(agentGroups([blocked], "needs")[0].sessions.map((s) => s.id)).toEqual(["a"]);
  });
});

describe("AgentsView", () => {
  it("shows the task, steps, both ETAs labelled apart, and time on task", () => {
    view();
    const row = screen.getByRole("article", { name: /migrate tests/ });
    expect(row).toHaveTextContent("migrate tests");
    expect(row).toHaveTextContent("claude");
    expect(row).toHaveTextContent("Mac Mini");
    expect(row).toHaveTextContent("step 3 of 8");
    expect(row).toHaveTextContent("claude says ~12 m");
    expect(row).toHaveTextContent("about 15 m at this pace");
    expect(row).toHaveTextContent("4 m on task");
    expect(within(row).getByTitle(/Reported by claude, not measured/)).toBeInTheDocument();
  });

  it("keeps a waiting agent's answers inline", () => {
    view();
    const row = screen.getByRole("article", { name: /codex · og/ });
    expect(within(row).getByRole("button", { name: /Answer y to codex · og/ })).toBeInTheDocument();
  });

  it("offers the four filters and an empty state", () => {
    let chosen = "";
    render(
      <MemoryRouter>
        <AgentsView sessions={[sessions[3]]} machines={machines} now={0} filter="all" onFilter={(f) => (chosen = f)} />
      </MemoryRouter>,
    );
    for (const name of ["All", "Needs you", "Working", "Done"]) {
      expect(screen.getByRole("button", { name: new RegExp(`^${name}`) })).toBeInTheDocument();
    }
    fireEvent.click(screen.getByRole("button", { name: /^Working/ }));
    expect(chosen).toBe("working");
    expect(screen.getByText(/No agents here yet/)).toBeInTheDocument();
  });
});
