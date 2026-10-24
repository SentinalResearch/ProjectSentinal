import { describe, expect, it, vi } from "vitest";

import type { HealthSampleBatch } from "../protocol/HealthSampleBatch";
import { createEchoTimer, createHealthCollector, MAX_BUFFERED } from "./health";

function collector() {
  const sent: { batch: HealthSampleBatch; beacon: boolean }[] = [];
  let clock = 1_000;
  const health = createHealthCollector({ now: () => clock });
  health.start((batch, beacon) => {
    sent.push({ batch, beacon });
  });
  return { health, sent, tick: (ms: number) => (clock += ms) };
}

describe("typing echo timer", () => {
  it("times one printable keystroke to the next output", () => {
    let now = 100;
    const echo = createEchoTimer(() => now);
    echo.keystroke("a");
    now = 112;
    expect(echo.output()).toBe(12);
    expect(echo.output()).toBeUndefined();
  });

  it("ignores pastes, control keys, a second key in flight, and echoes over a second", () => {
    let now = 0;
    const echo = createEchoTimer(() => now);
    echo.keystroke("ls -la\r");
    echo.keystroke("\r");
    echo.keystroke("\x1b[A");
    expect(echo.output()).toBeUndefined();

    echo.keystroke("a");
    now = 5;
    echo.keystroke("b"); // not restarted: the first key is still in flight
    now = 30;
    expect(echo.output()).toBe(30);

    echo.keystroke("c");
    now = 1_200;
    expect(echo.output()).toBeUndefined();
  });
});

describe("health collector", () => {
  it("does nothing until started and keeps at most the buffer bound", () => {
    let clock = 0;
    const health = createHealthCollector({ now: () => clock });
    for (let index = 0; index < MAX_BUFFERED + 50; index += 1) {
      health.record("TYPING_ECHO_MS", "rtx", 10);
      clock += 1;
    }
    const send = vi.fn();
    health.setControlMachine("ctrl");
    health.start(send);
    health.flush(false);
    health.stop();
    expect(send).toHaveBeenCalledTimes(1);
    expect(send.mock.calls[0][0].samples).toHaveLength(MAX_BUFFERED);
  });

  it("sends only the allowed fields, fills in the control, and clears after sending", () => {
    const { health, sent } = collector();
    health.record("TYPING_ECHO_MS", "rtx", 12.345);
    health.recordForControl("PAGE_LOAD_STEP_MS", 120, "LOAD_SNAPSHOT");
    health.count("WS_RECONNECTS", "rtx", "SOCKET_TERMINAL");
    health.flush(true);
    expect(sent).toHaveLength(0); // control unknown yet: nothing is guessed

    health.setControlMachine("ctrl");
    health.flush(true);
    expect(sent).toHaveLength(1);
    expect(sent[0].beacon).toBe(true);
    const samples = sent[0].batch.samples;
    expect(samples.map((sample) => [sample.machine_id, sample.metric, sample.label ?? null, sample.value])).toEqual([
      ["rtx", "TYPING_ECHO_MS", null, 12.3],
      ["ctrl", "PAGE_LOAD_STEP_MS", "LOAD_SNAPSHOT", 120],
      ["rtx", "WS_RECONNECTS", "SOCKET_TERMINAL", 1],
    ]);
    for (const sample of samples) {
      expect(Object.keys(sample).every((key) => ["machine_id", "metric", "label", "at", "value"].includes(key))).toBe(true);
    }
    health.flush(false);
    expect(sent).toHaveLength(1);
  });

  it("flushes every 30 seconds while started", () => {
    vi.useFakeTimers();
    try {
      const { health, sent } = collector();
      health.setControlMachine("ctrl");
      health.record("TERMINAL_OPEN_MS", "rtx", 140);
      vi.advanceTimersByTime(29_999);
      expect(sent).toHaveLength(0);
      vi.advanceTimersByTime(1);
      expect(sent).toHaveLength(1);
      expect(sent[0].beacon).toBe(false);
      health.stop();
    } finally {
      vi.useRealTimers();
    }
  });
});
