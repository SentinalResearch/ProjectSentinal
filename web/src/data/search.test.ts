import { describe, expect, it } from "vitest";

import fixture from "../../../crates/protocol/tests/fixtures/search_ranking.json";
import type { Machine } from "./types";
import { clip, matchScore, mergeResults, parseFind, rankScore, resultFromHit } from "./search";

describe("ranking shared with the daemon", () => {
  it.each(fixture.match)("$query in $haystacks scores $score", ({ query, haystacks, score }) => {
    expect(matchScore(query, haystacks)).toBe(score);
  });

  it.each(fixture.rank)("match $match, age $age_ms, needs you $needs_you → $score", (row) => {
    expect(rankScore(row.match, row.age_ms, row.needs_you)).toBe(row.score);
  });

  it.each(fixture.order)("$comment", ({ query, items, expected }) => {
    const order = items
      .map((item) => ({ text: item.text, score: rankScore(matchScore(query, [item.text]), item.age_ms, item.needs_you) }))
      .sort((left, right) => right.score - left.score)
      .map((item) => item.text);
    expect(order).toEqual(expected);
  });

  it("clips like the daemon", () => {
    expect(clip("abcdefghij", 5)).toBe("abcd…");
    expect(clip("ééééé", 3)).toBe("éé…");
    expect(clip("  padded\nline  ", 20)).toBe("padded line");
  });
});

const machines = [
  { id: "m-rtx", name: "RTX-PC" },
  { id: "m-mini", name: "Mac mini" },
] as Machine[];
const NOW = new Date(2026, 10, 1, 15, 30).getTime();
const HOUR = 3_600_000;

describe("find grammar", () => {
  it("reads outcome, Machine, and time words and keeps the rest as text", () => {
    expect(parseFind("failed pytest on rtx-pc since yesterday", machines, NOW)).toEqual({
      text: "pytest",
      outcome: "FAILED",
      machine: "m-rtx",
      since: new Date(2026, 9, 31).getTime(),
    });
  });

  it("knows today, hours, days, and weeks", () => {
    expect(parseFind("train since today", machines, NOW).since).toBe(new Date(2026, 10, 1).getTime());
    expect(parseFind("train since 3h", machines, NOW).since).toBe(NOW - 3 * HOUR);
    expect(parseFind("train since 2d", machines, NOW).since).toBe(NOW - 48 * HOUR);
    expect(parseFind("train since week", machines, NOW).since).toBe(NOW - 7 * 24 * HOUR);
  });

  it("matches Machines by name or slug, and leaves unknown words as text", () => {
    expect(parseFind("running on mac-mini", machines, NOW)).toEqual({ text: "", outcome: "RUNNING", machine: "m-mini" });
    expect(parseFind("on nowhere", machines, NOW)).toEqual({ text: "on nowhere" });
    expect(parseFind("since then", machines, NOW)).toEqual({ text: "since then" });
    expect(parseFind("  cuda   oom ", machines, NOW)).toEqual({ text: "cuda oom" });
  });
});

describe("results", () => {
  it("lead to the Session, Command, Event, or Recipe on its Machine", () => {
    const at = NOW;
    expect(
      resultFromHit("m-rtx", {
        kind: "EVENTS",
        title: "Error · train",
        detail: "CUDA out of memory",
        target: { type: "EVENT", session_id: 3, event_id: 9 },
        at,
        score: 54.15,
      }),
    ).toMatchObject({ id: "m-rtx:EVENTS:9", sessionId: "m-rtx:3", to: "/app/sessions/m-rtx%3A3", machineId: "m-rtx" });
    expect(
      resultFromHit("m-rtx", {
        kind: "RECIPES",
        title: "Eval",
        detail: "python eval.py",
        target: { type: "RECIPE", recipe_id: "eval" },
        at: null,
        score: 100,
      }),
    ).toMatchObject({ to: "/app/recipes/eval?machine=m-rtx" });
  });

  it("merge every Machine's hits best first, newest first on a tie", () => {
    const hit = (id: string, score: number, at: number) =>
      ({ id, kind: "COMMANDS", title: id, detail: "", score, at, machineId: "m", to: "/" }) as const;
    expect(
      mergeResults([[hit("a", 50, 1), hit("b", 90, 1)], [hit("c", 90, 2)], []], 2).map((result) => result.id),
    ).toEqual(["c", "b"]);
  });
});
