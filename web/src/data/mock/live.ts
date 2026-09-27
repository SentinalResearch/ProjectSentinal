import { appendEvent, findSession, store, updateSession } from "./store";

const TICK_MS = 5000;

function tick() {
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
        const nextTo = Math.max(0.05, to - (Math.random() * 0.004 + 0.001));
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

export function acquireLiveUpdates(): () => void {
  refCount += 1;
  if (interval === null) {
    interval = setInterval(tick, TICK_MS);
  }
  return () => {
    refCount = Math.max(0, refCount - 1);
    if (refCount === 0 && interval !== null) {
      clearInterval(interval);
      interval = null;
    }
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
