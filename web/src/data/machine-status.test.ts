import { describe, expect, it } from "vitest";

import { isDelayed, lastSeenText, machineStatusText, withMachineReach } from "./machine-status";
import type { Machine, Session } from "./types";

const base: Machine = { id: "rtx", name: "RTX-PC", os: "Linux", hardware: "", status: "ONLINE" };

describe("Machine status text", () => {
  it("says a live Machine is online", () => {
    expect(isDelayed({ ...base, freshness: "LIVE" })).toBe(false);
    expect(machineStatusText({ ...base, freshness: "LIVE" }, 0)).toBe("online");
  });

  it("says a delayed Machine is online and when it last updated", () => {
    const delayed = { ...base, freshness: "STALE" as const, syncedAt: 1_000 };
    expect(isDelayed(delayed)).toBe(true);
    expect(machineStatusText(delayed, 15_400)).toBe("online · updated 14 s ago");
    expect(machineStatusText(delayed, 1_000 + 125_000)).toBe("online · updated 2 min ago");
    expect(machineStatusText({ ...delayed, syncedAt: undefined }, 0)).toBe("online · delayed");
  });

  it("keeps the heartbeat verdict for Machines that are not online", () => {
    expect(machineStatusText({ ...base, status: "OFFLINE", freshness: "OFFLINE" }, 0)).toBe("offline");
    expect(machineStatusText({ ...base, status: "AUTH_EXPIRED" }, 0)).toBe("auth expired");
    expect(isDelayed({ ...base, status: "OFFLINE", freshness: "STALE" })).toBe(false);
  });
});

describe("last seen", () => {
  const now = Date.UTC(2026, 10, 1, 12, 0, 0);
  it("reads as a short relative age", () => {
    expect(lastSeenText(now - 20_000, now)).toBe("last seen just now");
    expect(lastSeenText(now - 5 * 60_000, now)).toBe("last seen 5 min ago");
    expect(lastSeenText(now - 13 * 3_600_000, now)).toBe("last seen 13 h ago");
    expect(lastSeenText(now - 3 * 86_400_000, now)).toBe("last seen 3 days ago");
  });

  it("never reads as the future when clocks disagree", () => {
    expect(lastSeenText(now + 60_000, now)).toBe("last seen just now");
  });
});

describe("withMachineReach (X24)", () => {
  const session = (id: string, machine: string, state: Session["state"]): Session =>
    ({ id, machine, state, name: id, prompt: state === "WAITING" ? "Continue?" : undefined }) as Session;
  const sessions = [
    session("idle", "mac", "IDLE"),
    session("asks", "mac", "WAITING"),
    session("done", "mac", "COMPLETED"),
    session("live", "rtx", "RUNNING"),
    session("unlisted", "gone", "RUNNING"),
  ];

  it("says a live Session on an offline Machine is disconnected, not idle", () => {
    const shown = withMachineReach(sessions, [{ id: "mac", status: "OFFLINE" }, { id: "rtx", status: "ONLINE" }]);
    expect(shown.map((s) => s.state)).toEqual(["DISCONNECTED", "DISCONNECTED", "COMPLETED", "RUNNING", "RUNNING"]);
    expect(shown[1].prompt).toBeUndefined();
  });

  it("changes nothing while every Machine is online, and keeps the real state for later", () => {
    const online = [{ id: "mac", status: "ONLINE" as const }, { id: "rtx", status: "ONLINE" as const }];
    expect(withMachineReach(sessions, online)).toBe(sessions);
    withMachineReach(sessions, [{ id: "mac", status: "DISCONNECTED" }]);
    expect(sessions[0].state).toBe("IDLE");
  });
});
