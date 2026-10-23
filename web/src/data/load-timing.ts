/**
 * When each step of this page load happened, for Settings → Diagnostics
 * ("Last page load: snapshot 120 ms, live updates 340 ms, …"). Times are
 * milliseconds since the browser started loading the page. Each step is
 * recorded once; later reconnects are not page loads.
 */
export type LoadStep = "script" | "snapshot" | "live" | "shown";

const LABELS: Record<LoadStep, string> = {
  script: "app started",
  snapshot: "snapshot",
  live: "live updates",
  shown: "Sessions shown",
};

/** Requests whose Server-Timing explains a step. */
const SERVER_PATHS: Partial<Record<LoadStep, string>> = {
  snapshot: "/api/v1/control/snapshot",
};

const marks = new Map<LoadStep, number>();

export interface LoadTiming {
  step: LoadStep;
  label: string;
  ms: number;
  /** Time the control itself spent, from its Server-Timing header. */
  serverMs?: number;
}

export function markLoad(step: LoadStep): void {
  if (marks.has(step) || typeof performance === "undefined") return;
  const at = performance.now();
  marks.set(step, at);
  try {
    performance.mark(`sentinal:${step}`, { startTime: at });
  } catch {
    // The timeline above does not depend on the browser's mark buffer.
  }
}

function serverMs(path: string): number | undefined {
  const entries = performance.getEntriesByType("resource") as PerformanceResourceTiming[];
  const entry = entries.find((candidate) => {
    try {
      return new URL(candidate.name).pathname === path;
    } catch {
      return false;
    }
  });
  const timings = entry?.serverTiming ?? [];
  if (timings.length === 0) return undefined;
  return Math.round(timings.reduce((total, timing) => total + timing.duration, 0) * 10) / 10;
}

export function loadTimeline(): LoadTiming[] {
  return [...marks.entries()]
    .sort((left, right) => left[1] - right[1])
    .map(([step, ms]) => {
      const path = SERVER_PATHS[step];
      const server = path === undefined ? undefined : serverMs(path);
      return { step, label: LABELS[step], ms, ...(server === undefined ? {} : { serverMs: server }) };
    });
}

/** Tests only. */
export function resetLoadTiming(): void {
  marks.clear();
}
