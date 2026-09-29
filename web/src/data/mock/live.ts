import { appendEvent, findSession, store, updateSession } from "./store";

const DEFAULT_TICK_MS = 5000;
let tickMs: number | null = DEFAULT_TICK_MS;

/** Advance every eligible simulated Session by one deterministic product tick. */
export function advanceLiveSimulation() {
  const now = new Date();
  const time = `${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`;

  for (const session of store.sessions) {
    if (session.state !== "RUNNING") continue;

    if (session.progress && session.progress.current < session.progress.total) {
      const next = Math.min(session.progress.total, session.progress.current + 1);
      const patch: Parameters<typeof updateSession>[1] = {
        progress: { ...session.progress, current: next },
      };
      if (session.metrics?.loss) {
        const { from, to } = session.metrics.loss;
        const nextTo = Math.max(0.05, to - 0.002);
        patch.metrics = { ...session.metrics, loss: { from, to: Number(nextTo.toFixed(3)) } };
      }
      updateSession(session.id, patch);
      appendEvent(session.id, [time, "PROGRESS", `${session.progress.unit} ${next}/${session.progress.total}`]);
      if (next % 10 === 0) {
        appendEvent(session.id, [time, "CHECKPOINT_CREATED", `ckpt/epoch_${next}.pt`]);
      }
      if (next === session.progress.total) {
        updateSession(session.id, { state: "COMPLETED" });
      }
    }
  }
}

let interval: ReturnType<typeof setInterval> | null = null;
let refCount = 0;

function stopInterval(): void {
  if (interval !== null) {
    clearInterval(interval);
    interval = null;
  }
}

function startInterval(): void {
  stopInterval();
  if (refCount > 0 && tickMs !== null) {
    interval = setInterval(advanceLiveSimulation, tickMs);
  }
}

/** Configure automatic fixture playback; `null` pauses it. */
export function configureLivePlayback(nextTickMs: number | null): void {
  tickMs = nextTickMs;
  startInterval();
}

export function acquireLiveUpdates(): () => void {
  refCount += 1;
  startInterval();
  return () => {
    refCount = Math.max(0, refCount - 1);
    if (refCount === 0) stopInterval();
  };
}

export function resolvePrompt(sessionId: string, answer: "y" | "n") {
  const session = findSession(sessionId);
  const now = new Date();
  const time = `${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`;
  if (answer === "y") {
    updateSession(sessionId, { state: "RUNNING", prompt: undefined });
    appendEvent(sessionId, [time, "COMMAND_STARTED", "continuing"]);
  } else {
    updateSession(sessionId, { state: "INTERRUPTED", prompt: undefined });
    appendEvent(sessionId, [time, "WARNING", "cancelled by user"]);
  }
}
