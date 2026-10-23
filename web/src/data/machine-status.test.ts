import { describe, expect, it } from "vitest";

import { isDelayed, machineStatusText } from "./machine-status";
import type { Machine } from "./types";

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
