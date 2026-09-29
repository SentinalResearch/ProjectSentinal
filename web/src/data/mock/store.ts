import raw from "./fixtures.json";
import { config } from "../config";
import type {
  AuthState,
  HistoryEntry,
  JournalNotification,
  Machine,
  MachineToken,
  NotificationRules,
  Recipe,
  Session,
  SessionEvent,
} from "../types";

export function delay<T>(value: T, minMs = 300, maxMs = 800): Promise<T> {
  if (import.meta.env.MODE === "test") return Promise.resolve(value);
  const ms = minMs + Math.random() * (maxMs - minMs);
  return new Promise((resolve) => setTimeout(() => resolve(value), ms));
}

interface Fixtures {
  machines: Machine[];
  sessions: (Omit<Session, "events"> & { events?: [string, string, string][] })[];
  history: { ok: boolean | null; command: string; duration: string }[];
  recipes: Recipe[];
  notifications: string[];
}

const fixtures = raw as unknown as Fixtures;

function cloneSessions(): Session[] {
  return fixtures.sessions.map((s) => ({
    ...structuredClone(s),
    events: s.events ? (structuredClone(s.events) as SessionEvent[]) : undefined,
  }));
}

// QUESTION: fixtures.json's `history` array only carries {ok, command, duration} —
// no machine/project/sessionId, so the History screen's Machine/Project filter chips
// would have nothing to filter by. SentinalHistory.dc.html's own working prototype
// (not just the static fixture) has a richer 6-row demo dataset that enriches exactly
// these commands with machine/project/id — mirrored here rather than inventing new
// values, plus daysAgo taken from that same prototype's `dateMax` bucket data. See
// QUESTIONS.md.
const HISTORY_ENRICHMENT: Record<string, { machine: string; project?: string; sessionId?: string; daysAgo: number }> =
  {
    "cargo build": { machine: "mbp", daysAgo: 3 }, // config.cargoBuildMachine assumption: MacBook
    "pytest tests/": { machine: "rtx", project: "MentalAnimAItion", sessionId: "pytest", daysAgo: 0 },
    "python benchmark.py": { machine: "rtx", project: "TIM", daysAgo: 1 },
    "python train.py": { machine: "rtx", project: "TIM", sessionId: "qwen", daysAgo: 0 },
  };

const DAYS_AGO_TO_DATE_LABEL = ["Today", "Yesterday", "3 days ago"];

function dateLabelForDaysAgo(daysAgo: number): string {
  return DAYS_AGO_TO_DATE_LABEL[daysAgo] ?? config.historyRelativeDates[daysAgo % config.historyRelativeDates.length];
}

// The prototype's demo dataset also surfaces each *live* RUNNING/WAITING session's
// current command as a history row (not just completed commands) — vllm and npm
// aren't in fixtures.json's history array, so add them the same way.
const LIVE_SESSION_HISTORY_ROWS: { sessionId: string; project?: string }[] = [
  { sessionId: "vllm" },
  { sessionId: "npm" },
];

function buildHistory(): HistoryEntry[] {
  const fromFixtures = fixtures.history.map((h, i): HistoryEntry => {
    const enrichment = HISTORY_ENRICHMENT[h.command];
    const daysAgo = enrichment?.daysAgo ?? i;
    return {
      id: `h${i}`,
      ok: h.ok,
      command: h.command,
      duration: h.duration,
      machine: enrichment?.machine,
      project: enrichment?.project,
      sessionId: enrichment?.sessionId,
      daysAgo,
      date: dateLabelForDaysAgo(daysAgo),
    };
  });

  const fromLiveSessions = LIVE_SESSION_HISTORY_ROWS.flatMap(({ sessionId, project }): HistoryEntry[] => {
    const session = fixtures.sessions.find((s) => s.id === sessionId);
    if (!session?.command) return [];
    return [
      {
        id: `h-live-${sessionId}`,
        ok: null,
        command: session.command,
        duration: session.state === "WAITING" ? "waiting" : "running",
        machine: session.machine,
        project: project ?? session.project,
        sessionId,
        daysAgo: 0,
        date: dateLabelForDaysAgo(0),
      },
    ];
  });

  return [...fromFixtures, ...fromLiveSessions];
}

export const store = {
  machines: structuredClone(fixtures.machines) as Machine[],
  sessions: cloneSessions(),
  history: buildHistory(),
  recipes: structuredClone(fixtures.recipes) as Recipe[],
  journal: fixtures.notifications.map((text, i): JournalNotification => ({ id: `n${i}`, text })),
  notificationRules: {} as NotificationRules,
  tokens: fixtures.machines.map(
    (m, i): MachineToken => ({
      machineId: m.id,
      machineName: m.name,
      maskedToken: config.tokenMask.replace("xxxx", String(1000 + i)),
      createdAt: ["Today", "3 days ago", "2 weeks ago"][i % 3] as string,
    }),
  ),
  auth: {
    status: "signedOut",
    attempts: 0,
    lockUntil: null,
  } as AuthState,
};

/** Fixture-backed state that may be replaced by the user-testing harness. */
export type MockStoreSnapshot = Pick<
  typeof store,
  "machines" | "sessions" | "history" | "recipes" | "journal" | "notificationRules" | "tokens"
>;

/** Return an isolated copy of the current fixture-backed product state. */
export function snapshotStore(): MockStoreSnapshot {
  return structuredClone({
    machines: store.machines,
    sessions: store.sessions,
    history: store.history,
    recipes: store.recipes,
    journal: store.journal,
    notificationRules: store.notificationRules,
    tokens: store.tokens,
  });
}

/** Replace simulated product state while preserving the facilitator's auth state. */
export function replaceStore(snapshot: MockStoreSnapshot): void {
  const next = structuredClone(snapshot);
  store.machines = next.machines;
  store.sessions = next.sessions;
  store.history = next.history;
  store.recipes = next.recipes;
  store.journal = next.journal;
  store.notificationRules = next.notificationRules;
  store.tokens = next.tokens;
  emit({
    type: "DATA_RESET",
    machines: structuredClone(store.machines),
    sessions: structuredClone(store.sessions),
  });
}

type Listener = (event: import("../adapter").LiveEvent) => void;
const listeners = new Set<Listener>();

export function subscribe(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function emit(event: import("../adapter").LiveEvent) {
  for (const listener of listeners) listener(event);
}

export function findSession(id: string): Session {
  const session = store.sessions.find((s) => s.id === id);
  if (!session) throw new Error(`unknown session '${id}'`);
  return session;
}

export function updateSession(id: string, patch: Partial<Session>) {
  const session = findSession(id);
  Object.assign(session, patch);
  emit({ type: "SESSION_UPDATED", session });
}

export function appendEvent(id: string, event: SessionEvent) {
  const session = findSession(id);
  session.events = [...(session.events ?? []), event];
  emit({ type: "EVENT_APPENDED", sessionId: id, event });
}
