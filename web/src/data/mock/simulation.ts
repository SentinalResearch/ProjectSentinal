import type { Session } from "../types";
import { advanceLiveSimulation, configureLivePlayback } from "./live";
import { replaceStore, snapshotStore, type MockStoreSnapshot } from "./store";

export type SimulationScenarioId =
  | "mixed"
  | "first-run"
  | "needs-attention"
  | "healthy-training"
  | "capture-limits"
  | "offline-worker";

export type SimulationPlayback = "paused" | "normal" | "fast";

export interface SimulationScenario {
  id: SimulationScenarioId;
  name: string;
  description: string;
}

export interface SimulationState {
  scenario: SimulationScenarioId;
  playback: SimulationPlayback;
}

const SCENARIO_KEY = "sentinal.simulation.scenario";
const PLAYBACK_KEY = "sentinal.simulation.playback";
const PLAYBACK_MS: Record<SimulationPlayback, number | null> = {
  paused: null,
  normal: 5000,
  fast: 1000,
};

export const simulationScenarios: SimulationScenario[] = [
  {
    id: "mixed",
    name: "Mixed fleet",
    description: "The complete design fixture: healthy, failed, waiting, stalled, adopted, and offline states.",
  },
  {
    id: "first-run",
    name: "First run",
    description: "One online Machine with no Sessions or history, for empty-state and onboarding research.",
  },
  {
    id: "needs-attention",
    name: "Needs attention",
    description: "Only waiting, failed, and stalled Sessions, for triage and prioritization tasks.",
  },
  {
    id: "healthy-training",
    name: "Healthy training",
    description: "One active training Session with deterministic progress, resources, and checkpoints.",
  },
  {
    id: "capture-limits",
    name: "Capture limits",
    description: "Read-only adopted processes, including an ended process whose exit code is unknown.",
  },
  {
    id: "offline-worker",
    name: "Offline worker",
    description: "A worker with retained Sessions but no connection, for offline-state comprehension.",
  },
];

const scenarioIds = new Set<SimulationScenarioId>(simulationScenarios.map((scenario) => scenario.id));
const playbackIds = new Set<SimulationPlayback>(["paused", "normal", "fast"]);
const baseline = snapshotStore();

let current: SimulationState = {
  scenario: storedScenario(),
  playback: storedPlayback(),
};

function storedScenario(): SimulationScenarioId {
  const value = sessionStorage.getItem(SCENARIO_KEY);
  return scenarioIds.has(value as SimulationScenarioId) ? (value as SimulationScenarioId) : "mixed";
}

function storedPlayback(): SimulationPlayback {
  const value = sessionStorage.getItem(PLAYBACK_KEY);
  return playbackIds.has(value as SimulationPlayback) ? (value as SimulationPlayback) : "normal";
}

function sessionsFor(snapshot: MockStoreSnapshot, predicate: (session: Session) => boolean): Session[] {
  return snapshot.sessions.filter(predicate);
}

function machinesForSessions(snapshot: MockStoreSnapshot, sessions: Session[]) {
  const ids = new Set(sessions.map((session) => session.machine));
  return snapshot.machines.filter((machine) => ids.has(machine.id));
}

function tokensForMachines(snapshot: MockStoreSnapshot, machineIds: Set<string>) {
  return snapshot.tokens.filter((token) => machineIds.has(token.machineId));
}

function buildScenario(id: SimulationScenarioId): MockStoreSnapshot {
  const snapshot = structuredClone(baseline);
  if (id === "mixed") return snapshot;

  if (id === "first-run") {
    const machine = snapshot.machines.find((entry) => entry.status === "ONLINE");
    const machines = machine ? [machine] : [];
    return { ...snapshot, machines, sessions: [], history: [], journal: [], tokens: tokensForMachines(snapshot, new Set(machines.map((entry) => entry.id))) };
  }

  if (id === "needs-attention") {
    const sessions = sessionsFor(snapshot, (session) => ["WAITING", "FAILED", "STALLED"].includes(session.state));
    const machines = machinesForSessions(snapshot, sessions);
    return {
      ...snapshot,
      machines,
      sessions,
      history: snapshot.history.filter((entry) => entry.ok === false),
      tokens: tokensForMachines(snapshot, new Set(machines.map((entry) => entry.id))),
    };
  }

  if (id === "healthy-training") {
    const sessions = sessionsFor(snapshot, (session) => session.id === "qwen");
    const machines = machinesForSessions(snapshot, sessions);
    return {
      ...snapshot,
      machines,
      sessions,
      history: snapshot.history.filter((entry) => entry.sessionId === "qwen"),
      journal: [],
      tokens: tokensForMachines(snapshot, new Set(machines.map((entry) => entry.id))),
    };
  }

  if (id === "capture-limits") {
    const sessions = sessionsFor(snapshot, (session) => session.origin === "ADOPTED");
    const machines = machinesForSessions(snapshot, sessions);
    return {
      ...snapshot,
      machines,
      sessions,
      history: [],
      journal: [],
      tokens: tokensForMachines(snapshot, new Set(machines.map((entry) => entry.id))),
    };
  }

  const worker = snapshot.machines.find((machine) => machine.id === "rtx");
  if (!worker) return { ...snapshot, machines: [], sessions: [], history: [], journal: [], tokens: [] };
  const machine = { ...worker, status: "OFFLINE" as const, lastSeen: "2m" };
  const sessions = sessionsFor(snapshot, (session) => session.machine === machine.id);
  return {
    ...snapshot,
    machines: [machine],
    sessions,
    tokens: tokensForMachines(snapshot, new Set([machine.id])),
  };
}

/** Current simulation settings. */
export function getSimulationState(): SimulationState {
  return { ...current };
}

/** Replace demo state with one repeatable scenario. */
export function applyScenario(id: SimulationScenarioId): void {
  if (!scenarioIds.has(id)) throw new Error(`unknown simulation scenario '${id}'`);
  replaceStore(buildScenario(id));
  current = { ...current, scenario: id };
  sessionStorage.setItem(SCENARIO_KEY, id);
}

/** Change automatic playback without affecting scenario data. */
export function setPlayback(playback: SimulationPlayback): void {
  if (!playbackIds.has(playback)) throw new Error(`unknown simulation playback '${playback}'`);
  current = { ...current, playback };
  sessionStorage.setItem(PLAYBACK_KEY, playback);
  configureLivePlayback(PLAYBACK_MS[playback]);
}

/** Advance one deterministic tick even while automatic playback is paused. */
export function advanceSimulation(): void {
  advanceLiveSimulation();
}

/** Restore the complete fixture baseline and normal playback. */
export function resetSimulation(): void {
  applyScenario("mixed");
  setPlayback("normal");
}

if (current.scenario !== "mixed") replaceStore(buildScenario(current.scenario));
configureLivePlayback(PLAYBACK_MS[current.playback]);
