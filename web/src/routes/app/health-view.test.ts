import { describe, expect, it } from "vitest";

import type { MetricStat } from "../../protocol/MetricStat";
import { emptyText, formatValue, incidentText, pathText, reading } from "./health-view";

const stat = (metric: MetricStat["metric"], p50: number): MetricStat => ({
  metric,
  stat: { count: 10, sum: p50 * 10, min: p50, p50, p95: p50, max: p50 },
  dropped: 0,
});

describe("health page words", () => {
  it("formats durations, sizes, and counts", () => {
    expect(formatValue("TYPING_ECHO_MS", 12.4)).toBe("12 ms");
    expect(formatValue("TERMINAL_OPEN_MS", 1_480)).toBe("1.5 s");
    expect(formatValue("DAEMON_RSS_BYTES", 19_922_944)).toBe("19 MB");
    expect(formatValue("LIVE_SESSIONS", 3)).toBe("3");
  });

  it("highlights a value past its threshold with the reason", () => {
    expect(reading([stat("TYPING_ECHO_MS", 13)], "TYPING_ECHO_MS")).toEqual({ value: "13 ms", level: "ok" });
    expect(reading([stat("TYPING_ECHO_MS", 140)], "TYPING_ECHO_MS")).toEqual({
      value: "140 ms",
      level: "bad",
      reason: "Typing feels late: 140 ms per key. Usually distance or a relayed Tailscale path.",
    });
    expect(reading([stat("LINK_RTT_MS", 80)], "LINK_RTT_MS")?.level).toBe("warn");
    expect(reading([], "LINK_RTT_MS")).toBeUndefined();
  });

  it("describes incidents and the current relay", () => {
    const at = new Date(2026, 9, 24, 9, 14).getTime();
    const offline = { machine_id: "rtx", kind: "OFFLINE" as const, started_at: at, ended_at: at + 120_000 };
    expect(incidentText(offline, "RTX-PC", at)).toMatch(/^RTX-PC offline 2 min at 09.14/);
    const relay = { machine_id: "rtx", kind: "RELAYED" as const, started_at: at, ended_at: null, detail: "fra" };
    expect(incidentText(relay, "RTX-PC", at)).toMatch(/relayed through fra .* \(ongoing\)$/);
    expect(pathText([relay], "rtx")).toBe("Relayed · fra");
    expect(pathText([{ ...relay, ended_at: at + 1 }], "rtx")).toBeUndefined();
  });

  it("explains an empty card", () => {
    const base = { machine_id: "w", display_name: "W", daemon_version: null, latest: [], stats: [], series: [] };
    expect(emptyText({ ...base, role: "WORKER", reports_health: false })).toMatch(/older Sentinal/);
    expect(emptyText({ ...base, role: "CONTROL", reports_health: false })).toMatch(/^No data yet/);
  });
});
