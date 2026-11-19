import type { ProgressEstimate } from "../protocol/ProgressEstimate";
import { shortDuration } from "./agentState";

/**
 * The time left, said honestly (B1): what it rests on and how sure it is.
 * "about 4 m left", "usually 6 m · about 2 m left", "no progress for 3 m".
 */
export function estimateText(estimate: ProgressEstimate | undefined): string | undefined {
  if (!estimate) return undefined;
  if (estimate.stalled_secs != null) return `no progress for ${shortDuration(estimate.stalled_secs)}`;
  const usually = estimate.typical_secs != null ? `usually ${shortDuration(estimate.typical_secs)}` : undefined;
  if (estimate.longer_than_usual) return `taking longer than usual${usually ? ` (${usually})` : ""}`;
  const rough = estimate.confidence === "LOW" ? "roughly " : "about ";
  const left =
    estimate.left_secs == null
      ? undefined
      : estimate.left_secs === 0
        ? "almost done"
        : `${rough}${shortDuration(estimate.left_secs)} left`;
  if (estimate.basis === "HISTORY") return [usually, left].filter(Boolean).join(" · ") || undefined;
  return left;
}

/** "4 m left" for a compact row; nothing when there is no countdown. */
export function estimateShort(estimate: ProgressEstimate | undefined): string | undefined {
  if (!estimate) return undefined;
  if (estimate.stalled_secs != null) return "stalled?";
  if (estimate.longer_than_usual) return "longer than usual";
  if (estimate.left_secs == null) return undefined;
  return estimate.left_secs === 0 ? "almost done" : `${shortDuration(estimate.left_secs)} left`;
}

/** When it should finish, as a clock time, from `now`. */
export function finishClock(estimate: ProgressEstimate | undefined, now: number): string | undefined {
  if (!estimate || estimate.left_secs == null || estimate.left_secs === 0 || estimate.stalled_secs != null) return undefined;
  const at = new Date(now + estimate.left_secs * 1000);
  return `${String(at.getHours()).padStart(2, "0")}:${String(at.getMinutes()).padStart(2, "0")}`;
}

/** "40 epoch/min" from the pace. */
export function paceText(estimate: ProgressEstimate | undefined, unit: string | undefined): string | undefined {
  if (!estimate?.per_minute || !unit || unit === "%") return undefined;
  const value = estimate.per_minute;
  return value >= 1 ? `${value} ${unit}/min` : `${Math.round(value * 60 * 10) / 10} ${unit}/h`;
}

/** What the estimate rests on, for a tooltip. */
export function estimateBasisText(estimate: ProgressEstimate | undefined): string | undefined {
  if (!estimate) return undefined;
  const sure = { LOW: "a rough guess", MEDIUM: "a fair guess", HIGH: "a steady pace" }[estimate.confidence];
  return estimate.basis === "COUNTS"
    ? `From the program's own counts, ${sure}. Sentinal's estimate, not the program's.`
    : `From ${estimate.runs ?? "earlier"} earlier successful runs of this command here, ${sure}.`;
}
