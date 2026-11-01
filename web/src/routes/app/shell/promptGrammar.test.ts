import { describe, expect, it } from "vitest";

import type { Machine, Recipe, Session } from "../../../data/types";
import { settingsTabs } from "../settings-tabs";
import type { SearchResult } from "../../../data/search";
import { matchScore, parseRun, searchText, suggest, type PromptIndex } from "./promptGrammar";

const machines: Machine[] = [
  { id: "rtx", name: "RTX-PC", os: "Linux", hardware: "", status: "ONLINE" },
  { id: "mini", name: "Mac Mini", os: "macOS", hardware: "", status: "ONLINE" },
];

const sessions: Session[] = [
  { id: "mini:1", machine: "mini", name: "npm install", state: "WAITING", prompt: "Continue? [y/N]", runningFor: "85m", updatedAt: 5 },
  { id: "rtx:2", machine: "rtx", name: "pytest", state: "FAILED", result: { passed: 182, failed: 3, duration: "42s", summary: "3 failures" } },
  { id: "rtx:3", machine: "rtx", name: "Qwen Training", state: "RUNNING", project: "TIM" },
  { id: "rtx:4", machine: "rtx", name: "tailed log", state: "WAITING", prompt: "Overwrite? [y/N]", capture: "PROCESS_LOG" },
];

const recipes: Recipe[] = [
  {
    id: "qwen-vllm",
    name: "Qwen vLLM Server",
    category: "SERVERS",
    machine: "rtx",
    machineIds: ["rtx", "mini"],
    template: "vllm serve {model}",
    params: { model: "Qwen/Qwen3-8B", port: "8000", gpu: "0.85" },
  },
];

const index: PromptIndex = { sessions, machines, recipes, settingsTabs: settingsTabs(false), theme: "dark" };

describe("prompt grammar", () => {
  it("answers the waiting prompt with just `y` (the 3-keystroke path: / y ↵)", () => {
    const [first] = suggest("y", index);
    expect(first.group).toBe("ANSWER");
    expect(first.command).toBe("y → mac-mini/npm-install");
    expect(first.detail).toBe("Continue? [y/N] · waiting 85m");
    expect(first.action).toEqual({ kind: "answer", sessionId: "mini:1", key: "y" });
  });

  it("never offers to answer a Session the owner cannot type into", () => {
    expect(suggest("y tailed", index).filter((s) => s.group === "ANSWER")).toHaveLength(0);
  });

  it("narrows `n <session>` by name", () => {
    expect(suggest("n npm", index)[0].action).toEqual({ kind: "answer", sessionId: "mini:1", key: "n" });
  });

  it("goes to a Session by machine/session path", () => {
    const [first] = suggest("go rtx-pc/pyt", index);
    expect(first.command).toBe("go rtx-pc/pytest");
    expect(first.action).toEqual({ kind: "navigate", to: "/app/sessions/rtx%3A2" });
    expect(first.detail).toBe("Failed · 3 failures");
  });

  it("reaches every page, Settings tab, Machine and project", () => {
    const go = (text: string) => suggest(`go ${text}`, index)[0].action;
    expect(go("history")).toEqual({ kind: "navigate", to: "/app/history" });
    expect(go("recipes")).toEqual({ kind: "navigate", to: "/app/recipes" });
    expect(go("dashboard")).toEqual({ kind: "navigate", to: "/app/overview" });
    expect(go("split")).toEqual({ kind: "navigate", to: "/app/split" });
    expect(go("diagnostics")).toEqual({ kind: "navigate", to: "/app/settings/diagnostics" });
    expect(go("account")).toEqual({ kind: "navigate", to: "/app/settings/account" });
    expect(go("mac-mini")).toEqual({ kind: "navigate", to: "/app/overview?machine=mini" });
    expect(go("#tim")).toEqual({ kind: "navigate", to: "/app/projects/tim" });
  });

  it("run opens the prefilled form, never runs", () => {
    const [first] = suggest('run "Qwen vLLM Server" on mac-mini', index);
    expect(first.command).toBe('run "Qwen vLLM Server" on mac-mini');
    expect(first.detail).toBe("recipe · Mac Mini · 3 parameters");
    expect(first.action).toEqual({ kind: "navigate", to: "/app/recipes/qwen-vllm?machine=mini" });
  });

  it("parses quoted and plain recipe names with an optional machine", () => {
    expect(parseRun('"Qwen vLLM Server" on rtx-pc')).toEqual({ recipe: "Qwen vLLM Server", machine: "rtx-pc" });
    expect(parseRun("qwen on rtx")).toEqual({ recipe: "qwen", machine: "rtx" });
    expect(parseRun('"Qwen vLL')).toEqual({ recipe: "Qwen vLL", machine: undefined });
  });

  it("find searches history", () => {
    expect(suggest("find npm install", index)[0].action).toEqual({ kind: "navigate", to: "/app/history?q=npm%20install" });
  });

  it("split opens the named terminals side by side", () => {
    const [first] = suggest("split npm pytest", index);
    expect(first.command).toBe("split mac-mini/npm-install rtx-pc/pytest");
    expect(first.action).toEqual({ kind: "navigate", to: "/app/split?sessions=mini%3A1,rtx%3A2" });
  });

  it("kill only asks — it opens the confirmation for a live Session", () => {
    const [first] = suggest("kill qwen", index);
    expect(first.action).toEqual({ kind: "kill", sessionId: "rtx:3" });
    expect(first.detail).toBe("Running · asks before sending Ctrl-C");
    expect(suggest("kill pytest", index).some((s) => s.action.kind === "kill")).toBe(false);
  });

  it("bare text searches Sessions, Machines, recipes, history and actions", () => {
    const results = suggest("qwen", index);
    expect(results.map((s) => s.group)).toEqual(["GO", "RUN", "FIND"]);
    expect(results[0].command).toBe("go rtx-pc/qwen-training");
    expect(suggest("theme", index).find((s) => s.group === "DO")?.action).toEqual({ kind: "toggleTheme" });
    expect(suggest("sign", index).find((s) => s.group === "DO")?.action).toEqual({ kind: "signOut" });
  });

  it("text that names an action selects it first (`new session` must not search history)", () => {
    expect(suggest("new session", index)[0].action).toEqual({ kind: "newSession" });
    expect(suggest("theme", index)[0].action).toEqual({ kind: "toggleTheme" });
    expect(suggest("sign out", index)[0].action).toEqual({ kind: "signOut" });
  });

  it("an empty prompt leads with what needs answering", () => {
    const results = suggest("", index);
    expect(results[0].action.kind).toBe("answer");
    expect(results.some((s) => s.command === "go rtx-pc/pytest")).toBe(true);
  });

  it("tab completes to the full command", () => {
    expect(suggest("go pyt", index)[0].completion).toBe("go rtx-pc/pytest");
    expect(suggest("split npm py", index)[1].completion).toBe("split npm rtx-pc/pytest ");
  });

  it("scores word starts above inner matches and rejects non-matches", () => {
    expect(matchScore("ins", ["npm install"])).toBeGreaterThan(matchScore("tal", ["npm install"]));
    expect(matchScore("zzz", ["npm install"])).toBe(0);
    expect(matchScore("nmi", ["npm install"])).toBeGreaterThan(0);
  });

  describe("search everywhere (5.S)", () => {
    const oom: SearchResult = {
      id: "rtx:EVENTS:9",
      kind: "EVENTS",
      title: "Error · Qwen Training",
      detail: "CUDA out of memory",
      score: 54.15,
      machineId: "rtx",
      to: "/app/sessions/rtx%3A3",
      sessionId: "rtx:3",
    };
    const train: SearchResult = {
      id: "rtx:COMMANDS:4",
      kind: "COMMANDS",
      title: "python train.py",
      detail: "exit 1 · ~/ml",
      score: 34.15,
      machineId: "rtx",
      to: "/app/sessions/rtx%3A3",
      sessionId: "rtx:3",
    };
    const found = (query: string, results: SearchResult[]): PromptIndex => ({ ...index, found: { query, results } });

    it("asks the Machines for bare text and for find, never for other commands", () => {
      expect(searchText("oom")).toBe("oom");
      expect(searchText("  find failed pytest on rtx-pc ")).toBe("failed pytest on rtx-pc");
      expect(searchText("o")).toBeUndefined();
      expect(searchText("find")).toBeUndefined();
      for (const text of ["go rtx", "run eval", "y npm", "split a b", "kill npm", "n", ""]) {
        expect(searchText(text), text).toBeUndefined();
      }
    });

    it("shows what the Machines found under Find, each leading to its Session", () => {
      const rows = suggest("find oom", found("oom", [oom, train])).filter((row) => row.group === "FIND");
      expect(rows.map((row) => row.command)).toEqual(["Error · Qwen Training", "python train.py", "find oom"]);
      expect(rows[0].detail).toBe("Event · RTX-PC · CUDA out of memory");
      expect(rows[0].action).toEqual({ kind: "navigate", to: "/app/sessions/rtx%3A3" });
      expect(rows[0].sessionId).toBe("rtx:3");
      // The last row still opens History with the query.
      expect(rows[2].action).toEqual({ kind: "navigate", to: "/app/history?q=oom" });
    });

    it("adds found results to bare text, and ignores results for older text", () => {
      expect(suggest("oom", found("oom", [oom])).some((row) => row.id === "found:rtx:EVENTS:9")).toBe(true);
      expect(suggest("oomx", found("oom", [oom])).some((row) => row.id === "found:rtx:EVENTS:9")).toBe(false);
    });

    it("uses the ranking shared with the daemon, initials included", () => {
      expect(matchScore("oom", ["CUDA out of memory"])).toBe(40);
    });
  });
});
