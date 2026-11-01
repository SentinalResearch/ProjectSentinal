import { describe, expect, it } from "vitest";

import type { Machine, Session } from "../../../data/types";
import { islandLocation } from "./location";
import { needCount, tickerItems } from "./ticker";

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
    expect(islandLocation("/app/overview", "", machines, sessions)).toEqual({ kind: "page", name: "Dashboard" });
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
