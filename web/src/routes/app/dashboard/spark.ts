import type { Session } from "../../../data/types";

/** Bars in a lane card's sparkline (GLASS_SPEC.md §7). */
export const SPARK_SAMPLES = 20;

/**
 * What a Session's sparkline plots: its progress when it reports one,
 * otherwise its GPU or CPU use (the daemon does not send an output rate to
 * the browser), otherwise nothing.
 */
export function sparkValue(session: Session): number | null {
  const progress = session.progress;
  if (progress && progress.total > 0) return (progress.current / progress.total) * 100;
  if (session.state !== "RUNNING" && session.state !== "STALLED" && session.state !== "WAITING") return null;
  return session.metrics?.gpuPct ?? session.metrics?.cpuPct ?? null;
}

export type SparkHistory = Record<string, Array<number | null>>;

/** Append one sample per live Session, keeping the last 20; finished Sessions are dropped. */
export function recordSample(history: SparkHistory, sessions: Session[]): SparkHistory {
  const next: SparkHistory = {};
  for (const session of sessions) {
    next[session.id] = [...(history[session.id] ?? []), sparkValue(session)].slice(-SPARK_SAMPLES);
  }
  return next;
}

/** Heights 0..1 for exactly 20 bars; missing or empty samples are null (drawn as a dash). */
export function sparkBars(samples: Array<number | null> = []): Array<number | null> {
  const padded = [...Array<null>(Math.max(0, SPARK_SAMPLES - samples.length)).fill(null), ...samples];
  const top = Math.max(1, ...padded.map((value) => value ?? 0));
  return padded.map((value) => (value === null || value <= 0 ? null : Math.max(0.12, value / top)));
}
