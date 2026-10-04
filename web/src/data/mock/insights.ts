import type { MachineInsight, MachineMetricSample, MetricRangeKey } from "../types";
import { delay, store } from "./store";

const RANGE: Record<MetricRangeKey, { spanMs: number; bucketMs: number }> = {
  HOUR: { spanMs: 3_600_000, bucketMs: 15_000 },
  DAY: { spanMs: 86_400_000, bucketMs: 300_000 },
  WEEK: { spanMs: 604_800_000, bucketMs: 1_800_000 },
};

/** Deterministic per-Machine facts so the Machine view has something to show. */
export function mockInsight(machineId: string): Promise<MachineInsight> {
  const machine = store.machines.find((entry) => entry.id === machineId);
  if (!machine) return Promise.reject(new Error(`unknown Machine '${machineId}'`));
  const running = store.sessions.filter((session) => session.machine === machineId && session.state === "RUNNING");
  return delay({
    hostname: machine.name.toLowerCase(),
    osVersion: machine.os === "macOS" ? "macOS 26.6.2" : "Ubuntu 24.04 LTS",
    kernelVersion: machine.os === "macOS" ? "25.6.0" : "6.8.0-45-generic",
    daemonVersion: "0.1.9",
    startedAt: Date.now() - 3 * 3_600_000,
    bind: "100.64.0.10:7420",
    link: { state: "ACTIVE", controlUrl: "https://control.tailnet.ts.net", lastHeartbeatAt: Date.now() - 4_000 },
    processes: running.map((session, index) => ({
      sessionId: session.id,
      processes: [
        { pid: 4_200 + index * 10, name: session.command?.split(" ")[0] ?? "shell", cpuPct: 92.5 - index * 20, rssBytes: 2_147_483_648 },
        { pid: 4_201 + index * 10, name: "worker", cpuPct: 12.1, rssBytes: 536_870_912 },
      ],
    })),
  });
}

export function mockMetrics(machineId: string, range: MetricRangeKey): Promise<MachineMetricSample[]> {
  const { spanMs, bucketMs } = RANGE[range];
  const end = Math.floor(Date.now() / bucketMs) * bucketMs;
  const seed = [...machineId].reduce((total, character) => total + character.charCodeAt(0), 0);
  const hasGpu = store.machines.find((entry) => entry.id === machineId)?.hardware.includes("RTX") ?? false;
  const points: MachineMetricSample[] = [];
  for (let at = end - spanMs; at <= end; at += bucketMs) {
    const phase = (at / bucketMs + seed) / 12;
    const cpuPct = Math.round((45 + 35 * Math.sin(phase)) * 10) / 10;
    points.push({
      at,
      cpuPct,
      ramPct: Math.round((55 + 15 * Math.sin(phase / 3)) * 10) / 10,
      gpuPct: hasGpu ? Math.round((70 + 25 * Math.cos(phase)) * 10) / 10 : undefined,
      vramPct: hasGpu ? 82 : undefined,
    });
  }
  return delay(points);
}
