const SEPARATOR = ":";

/** Browser-only identity for a daemon-local numeric object. */
export function compositeId(machine: string, local: number): string {
  if (!machine || machine.includes(SEPARATOR) || !Number.isSafeInteger(local) || local < 0) {
    throw new Error("cannot build composite Machine identity");
  }
  return `${machine}${SEPARATOR}${local}`;
}

function splitComposite(value: string): [string, number] {
  const at = value.lastIndexOf(SEPARATOR);
  const machine = value.slice(0, at);
  const local = Number(value.slice(at + 1));
  if (at <= 0 || machine.includes(SEPARATOR) || !Number.isSafeInteger(local) || local < 0) {
    throw new Error(`invalid composite Machine identity '${value}'`);
  }
  return [machine, local];
}

/** Owning Machine from a browser composite identity. */
export function machineId(value: string): string {
  return splitComposite(value)[0];
}

/** Daemon-local numeric ID from a browser composite identity. */
export function localId(value: string): number {
  return splitComposite(value)[1];
}

/** Map with a strict upper bound while preserving input order and failures. */
export async function mapConcurrent<T, R>(
  values: readonly T[],
  limit: number,
  map: (value: T, index: number) => Promise<R>,
): Promise<R[]> {
  if (!Number.isInteger(limit) || limit < 1) throw new Error("concurrency limit must be positive");
  const output = new Array<R>(values.length);
  let next = 0;
  const worker = async () => {
    while (next < values.length) {
      const index = next;
      next += 1;
      output[index] = await map(values[index], index);
    }
  };
  await Promise.all(Array.from({ length: Math.min(limit, values.length) }, worker));
  return output;
}

interface RetryPolicy {
  baseMs: number;
  maximumMs: number;
  wakeAfterMs: number;
}

/** Deterministic per-Machine retry state. */
export class RetryState {
  private failures = 0;
  private lastAttemptAt: number | null = null;

  constructor(private readonly policy: RetryPolicy) {}

  failedAt(now: number): number {
    this.lastAttemptAt = now;
    const delay = Math.min(this.policy.maximumMs, this.policy.baseMs * 2 ** this.failures);
    this.failures += 1;
    return now + delay;
  }

  succeeded(): void {
    this.failures = 0;
    this.lastAttemptAt = null;
  }

  resumeAt(now: number): number {
    if (this.lastAttemptAt !== null && now - this.lastAttemptAt >= this.policy.wakeAfterMs) {
      this.succeeded();
    }
    return now;
  }
}

/** Per-Machine Event IDs seen during the current connection generation. */
export class EventDeduper {
  private readonly seen = new Map<string, Set<number>>();

  accept(machine: string, event: number): boolean {
    const events = this.seen.get(machine) ?? new Set<number>();
    this.seen.set(machine, events);
    if (events.has(event)) return false;
    events.add(event);
    return true;
  }

  reset(machine: string): void {
    this.seen.delete(machine);
  }
}
