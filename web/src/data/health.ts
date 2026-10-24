/**
 * How Sentinal feels from this browser (milestone 4.H §1.4): typing echo,
 * terminal open time, page-load steps, reconnects, redraws, and error
 * codes, sent to the control in batches. Durations, counts, and closed codes
 * only: never keys, output, Commands, or messages. Nothing is kept in
 * browser storage.
 */
import type { HealthLabel } from "../protocol/HealthLabel";
import type { HealthMetric } from "../protocol/HealthMetric";
import type { HealthSample } from "../protocol/HealthSample";
import type { HealthSampleBatch } from "../protocol/HealthSampleBatch";

/** Samples held in memory between sends; older ones are dropped first. */
export const MAX_BUFFERED = 500;
export const FLUSH_MS = 30_000;
/** An echo later than this is not a keystroke echo. */
export const ECHO_TIMEOUT_MS = 1_000;
const CONTROL = "";

export type HealthSender = (batch: HealthSampleBatch, beacon: boolean) => void;

/** Times one printable keystroke to the next output frame of its terminal. */
export function createEchoTimer(now: () => number = () => performance.now()) {
  let started: number | null = null;
  return {
    keystroke(data: string) {
      if (started !== null) return;
      // One printable character: not a paste, Enter, Backspace, or an escape sequence.
      if (data.length !== 1 || data < " " || data === "\x7f") return;
      started = now();
    },
    output(): number | undefined {
      if (started === null) return undefined;
      const elapsed = now() - started;
      started = null;
      return elapsed <= ECHO_TIMEOUT_MS ? elapsed : undefined;
    },
  };
}

export function createHealthCollector({ now = Date.now }: { now?: () => number } = {}) {
  let buffer: HealthSample[] = [];
  let sender: HealthSender | null = null;
  let controlMachine: string | null = null;
  let timer: ReturnType<typeof setInterval> | null = null;

  const push = (machine: string, metric: HealthMetric, value: number, label?: HealthLabel) => {
    if (!Number.isFinite(value) || value < 0) return;
    const sample: HealthSample = { machine_id: machine, metric, at: now(), value: Math.round(value * 10) / 10 };
    if (label !== undefined) sample.label = label;
    buffer.push(sample);
    if (buffer.length > MAX_BUFFERED) buffer = buffer.slice(buffer.length - MAX_BUFFERED);
    // The timer starts with the first sample, so an idle page sets none.
    if (sender !== null && timer === null) timer = setInterval(() => flush(false), FLUSH_MS);
  };

  const flush = (beacon: boolean) => {
    if (sender === null || controlMachine === null || buffer.length === 0) return;
    const control = controlMachine;
    const samples = buffer.map((sample) =>
      sample.machine_id === CONTROL ? { ...sample, machine_id: control } : sample);
    buffer = [];
    sender({ samples }, beacon);
  };

  return {
    /** A measured value for `machine`. */
    record(metric: HealthMetric, machine: string, value: number, label?: HealthLabel) {
      push(machine, metric, value, label);
    },
    /** A measured value for the control, whose ID may not be known yet. */
    recordForControl(metric: HealthMetric, value: number, label?: HealthLabel) {
      push(CONTROL, metric, value, label);
    },
    /** One occurrence of a counted metric. */
    count(metric: HealthMetric, machine: string | null, label?: HealthLabel) {
      push(machine ?? CONTROL, metric, 1, label);
    },
    setControlMachine(id: string) {
      controlMachine = id;
    },
    flush,
    /** Begin sending; until then samples are only buffered. */
    start(send: HealthSender) {
      sender = send;
      if (timer === null && buffer.length > 0) timer = setInterval(() => flush(false), FLUSH_MS);
    },
    stop() {
      if (timer !== null) clearInterval(timer);
      timer = null;
      sender = null;
    },
  };
}

/** This page's collector. The real-data adapter starts it. */
export const health = createHealthCollector();
