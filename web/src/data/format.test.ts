import { describe, expect, it } from "vitest";

import {
  agentActionWord,
  agentFinishedText,
  agentUsageText,
  codingAgentName,
  eventKindLabel,
  formatEventTime,
  machineSummary,
  mergeProgress,
  progressCount,
  progressText,
  sessionDisplayName,
  sessionElapsedMs,
} from "./format";

describe("Event time", () => {
  it("is 24-hour HH:mm:ss in local time, with zero padding", () => {
    expect(formatEventTime(new Date(2026, 9, 28, 9, 5, 7))).toBe("09:05:07");
    expect(formatEventTime(new Date(2026, 9, 28, 21, 30, 0))).toBe("21:30:00");
  });

  it("accepts epoch milliseconds", () => {
    const at = new Date(2026, 9, 28, 0, 0, 59).getTime();
    expect(formatEventTime(at)).toBe("00:00:59");
  });
});

describe("Machine summary", () => {
  it("prefers hardware, then the Sentinal version, then the OS", () => {
    expect(machineSummary({ hardware: "M2 Pro, 32 GB", daemonVersion: "0.2.8", os: "macOS" })).toBe("M2 Pro, 32 GB");
    expect(machineSummary({ hardware: "", daemonVersion: "0.2.8", os: "macOS" })).toBe("Sentinal 0.2.8");
    expect(machineSummary({ hardware: "", os: "Linux" })).toBe("Linux");
  });
});

describe("Session elapsed time", () => {
  const hour = 3_600_000;
  const now = 300 * hour;

  it("counts to now while a Session is live", () => {
    expect(sessionElapsedMs({ state: "RUNNING", createdAt: 10 * hour, updatedAt: 11 * hour }, now)).toBe(290 * hour);
  });

  it("stops at the end for a finished Session instead of growing forever", () => {
    // An INTERRUPTED Session from days ago used to show 219h and counting.
    const interrupted = { state: "INTERRUPTED", createdAt: 10 * hour, updatedAt: 12 * hour };
    expect(sessionElapsedMs(interrupted, now)).toBe(2 * hour);
    expect(sessionElapsedMs({ ...interrupted, finishedAt: 11 * hour }, now)).toBe(1 * hour);
    expect(sessionElapsedMs({ state: "COMPLETED", createdAt: 10 * hour, updatedAt: 10 * hour }, now)).toBe(0);
  });

  it("is never negative when clocks disagree", () => {
    expect(sessionElapsedMs({ state: "RUNNING", createdAt: now + hour, updatedAt: now + hour }, now)).toBe(0);
  });
});

describe("AI coding agents (6.1)", () => {
  it("names agents and what they did and used", () => {
    expect(codingAgentName("CLAUDE")).toBe("Claude Code");
    expect(agentActionWord("RAN_TESTS")).toBe("ran tests");
    expect(agentFinishedText(1)).toBe("agent finished · 1 tool call");
    expect(agentUsageText({ agent: "CLAUDE", working: false, toolUses: 12, tokens: 120_000, costUsd: 1.25 })).toBe(
      "12 tool calls · 120k tokens · $1.25",
    );
    expect(agentUsageText({ agent: "AIDER", working: false, toolUses: 1, tokens: 312 })).toBe("1 tool call · 312 tokens");
  });
});

describe("sessionDisplayName (P5, X20)", () => {
  const claude = { agent: "CLAUDE" as const, working: false, toolUses: 0 };

  it("prefers the title, then agent and folder", () => {
    expect(sessionDisplayName({ id: 4, title: "train base", codingAgent: claude })).toBe("train base");
    expect(sessionDisplayName({ id: 4, cwd: "/home/me/src/sentinal", codingAgent: claude })).toBe("Claude Code · sentinal");
  });

  it("tells agent Sessions apart when no folder is known, as on the control dashboard", () => {
    expect(sessionDisplayName({ id: 4, codingAgent: claude })).toBe("Claude Code · Session 4");
    expect(sessionDisplayName({ id: 7, codingAgent: claude })).not.toBe(sessionDisplayName({ id: 4, codingAgent: claude }));
  });

  it("falls back to Session N", () => {
    expect(sessionDisplayName({ id: 9 })).toBe("Session 9");
  });
});

describe("eventKindLabel", () => {
  it("names known Event kinds in words", () => {
    expect(eventKindLabel("CHECKPOINT_CREATED")).toBe("Checkpoint saved");
    expect(eventKindLabel("PROGRESS_UPDATED")).toBe("Progress");
    expect(eventKindLabel("WAITING_FOR_INPUT")).toBe("Waiting for input");
  });

  it("falls back to sentence case for a kind it does not know", () => {
    expect(eventKindLabel("NEW_THING_HAPPENED")).toBe("New thing happened");
    expect(eventKindLabel("")).toBe("Event");
  });
});

describe("progress text (QA 2026-11-19)", () => {
  const epochs = { current: 7, total: 40, unit: "epoch" };
  const percent = { current: 17.5, total: 100, unit: "%" };

  it("shows the program's own count, and a bare percentage as a percentage", () => {
    expect(progressText(epochs)).toBe("epoch 7/40");
    expect(progressText(percent)).toBe("17.5 %");
    expect(progressCount(percent)).toBeUndefined();
  });

  it("never lets the dashboard's percentage replace the program's counts", () => {
    expect(mergeProgress(epochs, percent)).toBe(epochs);
    expect(mergeProgress(percent, epochs)).toBe(epochs);
    expect(mergeProgress(undefined, percent)).toBe(percent);
    expect(mergeProgress(epochs, undefined)).toBe(epochs);
  });
});
