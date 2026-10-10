import { extractPort } from "../../data/format";
import type { Session } from "../../data/types";

const TERMINAL_STATES = new Set(["COMPLETED", "FAILED", "INTERRUPTED"]);

/**
 * Param validation is keyed by what the param's *name* implies (port/context/model),
 * not by recipe id — see SentinalRecipes.dc.html's own `errs` logic, generalized so a
 * future recipe with the same kinds of params validates the same way.
 */
export function validateParam(key: string, value: string): string | undefined {
  const k = key.toLowerCase();
  if (k.includes("port")) {
    return /^\d+$/.test(value) && Number(value) >= 1 && Number(value) <= 65535 ? undefined : "Port must be 1–65535";
  }
  if (k.includes("context") || k.includes("len")) {
    return /^\d+$/.test(value) && Number(value) > 0 ? undefined : "Must be a positive integer";
  }
  if (k.includes("model")) {
    return value.trim() ? undefined : "Required";
  }
  return undefined;
}

/** "already used" warnings for port parameters that a live Session on the target Machine already uses. */
export function portWarnings(
  keys: string[],
  values: Record<string, string>,
  errors: Record<string, string | undefined>,
  sessions: Session[],
  machineId: string,
  machineName: string,
): Record<string, string> {
  const warnings: Record<string, string> = {};
  for (const key of keys) {
    if (!key.toLowerCase().includes("port") || errors[key]) continue;
    const port = values[key];
    const inUse = sessions.some(
      (session) => session.machine === machineId && !TERMINAL_STATES.has(session.state) && extractPort(session) === port,
    );
    if (inUse) warnings[key] = `:${port} is already used on ${machineName}`;
  }
  return warnings;
}
