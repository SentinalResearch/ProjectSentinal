import { describe, expect, it } from "vitest";

import {
  EventDeduper,
  RetryState,
  compositeId,
  localId,
  machineId,
  mapConcurrent,
} from "./machine-connections";

describe("multi-Machine identities", () => {
  it("keeps the same numeric Session ID distinct on two Machines", () => {
    expect(compositeId("machine-a", 1)).not.toBe(compositeId("machine-b", 1));
    expect(machineId(compositeId("machine-a", 1))).toBe("machine-a");
    expect(localId(compositeId("machine-a", 1))).toBe(1);
  });

  it("rejects malformed composite IDs instead of routing ambiguously", () => {
    expect(() => machineId("1")).toThrow(/composite/);
    expect(() => localId("machine-a:not-a-number")).toThrow(/composite/);
  });
});

describe("bounded aggregation", () => {
  it("limits concurrent Machine queries and preserves input order", async () => {
    let active = 0;
    let maximum = 0;
    const releases: Array<() => void> = [];
    const result = mapConcurrent([1, 2, 3, 4], 2, async (value) => {
      active += 1;
      maximum = Math.max(maximum, active);
      await new Promise<void>((resolve) => releases.push(resolve));
      active -= 1;
      return value * 10;
    });

    await Promise.resolve();
    expect(active).toBe(2);
    releases.shift()?.();
    releases.shift()?.();
    await Promise.resolve();
    await Promise.resolve();
    expect(active).toBe(2);
    releases.shift()?.();
    releases.shift()?.();
    await expect(result).resolves.toEqual([10, 20, 30, 40]);
    expect(maximum).toBe(2);
  });
});

describe("deterministic reconnect", () => {
  it("backs off to a cap and resets after success or a long wake", () => {
    const retry = new RetryState({ baseMs: 1_000, maximumMs: 8_000, wakeAfterMs: 30_000 });
    expect(retry.failedAt(1_000)).toBe(2_000);
    expect(retry.failedAt(2_000)).toBe(4_000);
    expect(retry.failedAt(4_000)).toBe(8_000);
    expect(retry.failedAt(8_000)).toBe(16_000);
    expect(retry.failedAt(16_000)).toBe(24_000);
    retry.succeeded();
    expect(retry.failedAt(25_000)).toBe(26_000);
    expect(retry.resumeAt(60_001)).toBe(60_001);
    expect(retry.failedAt(60_001)).toBe(61_001);
  });
});

describe("Event replay deduplication", () => {
  it("deduplicates within one Machine and never across Machines", () => {
    const deduper = new EventDeduper();
    expect(deduper.accept("machine-a", 7)).toBe(true);
    expect(deduper.accept("machine-a", 7)).toBe(false);
    expect(deduper.accept("machine-b", 7)).toBe(true);
    deduper.reset("machine-a");
    expect(deduper.accept("machine-a", 7)).toBe(true);
  });
});
