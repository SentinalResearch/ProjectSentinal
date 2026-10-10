import type { Session } from "../../../data/types";

/** Ctrl-C asks for confirmation once a job has run this long (approved 4.F rule). */
export const CONFIRM_AFTER_SECONDS = 60;

const UNIT_SECONDS: Record<string, number> = { d: 86_400, h: 3_600, m: 60, s: 1 };

/** Seconds a Session has been running, from `createdAt` or a label like "1h31m"; null if unknown. */
export function runningSeconds(session: Pick<Session, "createdAt" | "runningFor">, now = Date.now()): number | null {
  if (typeof session.createdAt === "number") return Math.max(0, Math.floor((now - session.createdAt) / 1000));
  const label = session.runningFor?.replace(/\s+/g, "");
  if (!label || !/^(\d+[dhms])+$/.test(label)) return null;
  let total = 0;
  for (const [, amount, unit] of label.matchAll(/(\d+)([dhms])/g)) total += Number(amount) * UNIT_SECONDS[unit];
  return total;
}

/** Long or unknown-length jobs confirm before Ctrl-C; short ones interrupt directly. */
export function needsInterruptConfirm(session: Pick<Session, "createdAt" | "runningFor">, now = Date.now()): boolean {
  const seconds = runningSeconds(session, now);
  return seconds === null || seconds > CONFIRM_AFTER_SECONDS;
}
