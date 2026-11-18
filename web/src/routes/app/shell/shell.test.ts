import { describe, expect, it } from "vitest";

import type { Machine, Session } from "../../../data/types";
import { islandLocation } from "./location";
import { islandFocus, needCount, tickerItems } from "./ticker";

function session(id: string, state: Session["state"], extra: Partial<Session> = {}): Session {
  return { id, machine: "rtx", name: id, state, ...extra };
}

const machines: Machine[] = [{ id: "rtx", name: "RTX-PC", os: "Linux", hardware: "", status: "ONLINE" }];

describe("island ticker", () => {
  it("orders FAILED > WAITING > STALLED > RUNNING and keeps three", () => {
    const items = tickerItems([
      session("train", "RUNNING", { progress: { current: 68, total: 100, unit: "epoch" } }),
      session("stuck", "STALLED"),
      session("npm", "WAITING"),
      session("pytest", "FAILED"),
    ]);
    expect(items.map((item) => item.text)).toEqual(["pytest failed", "npm waiting", "stuck stalled"]);
  });

  it("shows RUNNING only with progress, as a percentage", () => {
    const items = tickerItems([
      session("server", "RUNNING"),
      session("Qwen Training", "RUNNING", { progress: { current: 68, total: 100, unit: "epoch" } }),
    ]);
    expect(items.map((item) => item.text)).toEqual(["Qwen Training 68 %"]);
  });

  it("puts the newest first within a state and shortens long names", () => {
    const items = tickerItems([
      session("old", "FAILED", { updatedAt: 1 }),
      session("a very long session title that keeps going", "FAILED", { updatedAt: 2 }),
    ]);
    expect(items[0].text).toBe("a very long session t… failed");
    expect(items[1].text).toBe("old failed");
  });

  it("counts what needs the owner", () => {
    expect(needCount([session("a", "WAITING"), session("b", "FAILED"), session("c", "RUNNING")])).toBe(2);
  });
});

describe("island location", () => {
  const sessions = [session("rtx:7", "RUNNING", { name: "Qwen Training", project: "TIM" })];

  it("names pages", () => {
    expect(islandLocation("/app/overview", "", machines, sessions)).toEqual({ kind: "page", name: "Overview" });
    expect(islandLocation("/app/settings/machines", "", machines, sessions)).toEqual({ kind: "page", name: "Settings" });
    expect(islandLocation("/app/overview", "?machine=rtx", machines, sessions)).toEqual({ kind: "page", name: "RTX-PC" });
    expect(islandLocation("/app/projects/tim", "", machines, sessions)).toEqual({ kind: "page", name: "#TIM" });
  });

  it("names a Session as machine › title", () => {
    expect(islandLocation("/app/sessions/rtx%3A7/terminal", "", machines, sessions)).toEqual({
      kind: "session",
      sessionId: "rtx:7",
      machine: "RTX-PC",
      title: "Qwen Training",
    });
  });
});

describe("island focus", () => {
  it("puts a yes/no question you can answer first", () => {
    const focus = islandFocus([
      session("pytest", "FAILED"),
      session("npm", "WAITING", { prompt: "Continue? [y/N]" }),
    ]);
    expect(focus).toEqual({ kind: "answer", session: expect.objectContaining({ id: "npm" }) });
  });

  it("falls back to the ticker when the question was set aside or cannot be answered here", () => {
    const sessions = [session("npm", "WAITING", { prompt: "Continue? [y/N]" }), session("log", "WAITING", { prompt: "Password:" })];
    expect(islandFocus(sessions, new Set(["npm"]))).toEqual({ kind: "ticker" });
  });

  it("shows the running job closest to done when nothing needs you", () => {
    const focus = islandFocus([
      session("a", "RUNNING", { progress: { current: 20, total: 100, unit: "epoch" } }),
      session("b", "RUNNING", { progress: { current: 7, total: 8, unit: "epoch" } }),
      session("shell", "IDLE"),
    ]);
    expect(focus).toEqual({ kind: "progress", session: expect.objectContaining({ id: "b" }), percent: 88 });
  });

  it("is idle when nothing runs or needs you", () => {
    expect(islandFocus([session("shell", "IDLE")])).toEqual({ kind: "idle" });
  });
});

describe("agents that report themselves blocked (6.4)", () => {
  const session = (id: string, extra: Partial<Session>): Session => ({ id, machine: "m", name: id, state: "RUNNING", ...extra });

  it("appear in the island after failures and questions, but are not a need", () => {
    const terminals = [
      session("plain", {}),
      session("claude", { agentState: { status: "BLOCKED", detail: "needs the key", reportedAt: 1 } }),
      session("busy", { agentState: { status: "WORKING", reportedAt: 1 } }),
      session("broken", { state: "FAILED" }),
    ];
    expect(tickerItems(terminals).map((item) => item.text)).toEqual(["broken failed", "claude blocked"]);
    expect(needCount(terminals)).toBe(1);
  });
});

describe("agents that report steps (6.5)", () => {
  const agent = (id: string, step: number, steps: number, extra: Partial<Session> = {}): Session => ({
    id,
    machine: "m",
    name: id,
    state: "RUNNING",
    codingAgent: { agent: "CLAUDE", working: true, toolUses: 1 },
    agentState: { status: "WORKING", task: "migrate tests", step, steps, reportedAt: 1 },
    ...extra,
  });

  it("get a progress ring in the island, marked as what claude says", () => {
    expect(islandFocus([agent("claude", 3, 8)])).toEqual({
      kind: "progress",
      session: expect.objectContaining({ id: "claude" }),
      percent: 38,
      reported: { who: "claude", label: "step 3 of 8" },
    });
  });

  it("compete with measured progress on how close to done they are", () => {
    const focus = islandFocus([agent("claude", 3, 8), session("train", "RUNNING", { progress: { current: 9, total: 10, unit: "epoch" } })]);
    expect(focus).toEqual({ kind: "progress", session: expect.objectContaining({ id: "train" }), percent: 90 });
    expect(tickerItems([agent("claude", 3, 8)]).map((item) => item.text)).toEqual(["claude 38 %"]);
  });

  it("do not ring when blocked or without a plan", () => {
    expect(islandFocus([agent("a", 3, 8, { agentState: { status: "BLOCKED", step: 3, steps: 8, reportedAt: 1 } })]).kind).not.toBe("progress");
    expect(islandFocus([agent("b", 0, 0)])).toEqual({ kind: "idle" });
  });
});
