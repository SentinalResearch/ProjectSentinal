import type {
  AuthState,
  HistoryEntry,
  JournalNotification,
  Machine,
  MachineToken,
  NotificationRuleSet,
  NotificationRules,
  PairingToken,
  Recipe,
  Session,
  SessionEvent,
} from "./types";

export interface HistoryFilters {
  sessionId?: string;
  machine?: string;
  project?: string;
  result?: "ok" | "failed" | "running";
  query?: string;
}

export interface CreateSessionOptions {
  shell?: string;
  cwd?: string;
  title?: string;
  cols?: number;
  rows?: number;
}

export interface OutputReplayResult {
  warning?: string;
  bytesRead: number;
  truncated?: boolean;
}

export interface OutputReplayOptions {
  maxBytes?: number;
}

export type LiveEvent =
  | { type: "DATA_RESET"; machines: Machine[]; sessions: Session[] }
  | { type: "SESSION_UPDATED"; session: Session }
  | { type: "COMMAND_UPDATED"; sessionId: string; command: string | undefined }
  | { type: "SESSION_METRICS"; sessionId: string; metrics: Session["metrics"] }
  | { type: "EVENT_APPENDED"; sessionId: string; event: SessionEvent }
  | { type: "MACHINE_UPDATED"; machine: Machine }
  | { type: "NOTIFICATION"; notification: JournalNotification };

export class NotImplemented extends Error {
  constructor(method: string) {
    super(`${method} is not implemented by the real data adapter yet`);
    this.name = "NotImplemented";
  }
}

/**
 * The single seam between UI components and data. `src/data/mock` implements this
 * against `fixtures.json`; `src/data/api` implements the Milestone 1 daemon
 * real HTTP/WebSocket API. Select which one loads via `VITE_DATA=mock|api`.
 */
export interface DataAdapter {
  getMachines(): Promise<Machine[]>;
  getSessions(): Promise<Session[]>;
  createSession(options?: CreateSessionOptions): Promise<Session>;
  continueSession(sessionId: string): Promise<Session>;
  replayOutput(
    sessionId: string,
    onChunk: (chunk: Uint8Array) => void,
    options?: OutputReplayOptions,
  ): Promise<OutputReplayResult>;
  getRecipes(): Promise<Recipe[]>;
  getHistory(filters?: HistoryFilters): Promise<HistoryEntry[]>;

  /** Subscribe to live updates (epoch ticks, event appends, state transitions). Returns an unsubscribe function. */
  subscribe(listener: (event: LiveEvent) => void): () => void;

  sendInput(sessionId: string, data: string): Promise<void>;
  sendSignal(sessionId: string, signal: "SIGINT" | "KILL"): Promise<void>;

  runRecipe(recipeId: string, params: Record<string, string>, machineId: string): Promise<{ sessionId: string }>;

  pairMachine(machineName: string): Promise<PairingToken>;
  rotateToken(machineId: string): Promise<MachineToken>;
  revokeToken(machineId: string): Promise<void>;
  getMachineTokens(): Promise<MachineToken[]>;

  auth: {
    getState(): Promise<AuthState>;
    login(password: string): Promise<AuthState>;
    setup(bootstrapToken: string, password: string): Promise<AuthState>;
    changePassword(current: string, next: string): Promise<void>;
    logout(): Promise<void>;
  };

  notifications: {
    getRules(): Promise<NotificationRules>;
    setRule(sessionId: string, rules: NotificationRuleSet): Promise<void>;
    getJournal(): Promise<JournalNotification[]>;
  };
}
