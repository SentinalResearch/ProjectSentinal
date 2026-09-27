import type { SessionState } from "../protocol/SessionState";

export type MachineStatus = "ONLINE" | "OFFLINE";

export interface Machine {
  id: string;
  name: string;
  os: "Linux" | "macOS" | "Windows";
  hardware: string;
  status: MachineStatus;
  lastSeen?: string;
}

export interface SessionProgress {
  current: number;
  total: number;
  unit: string;
}

export interface SessionMetrics {
  loss?: { from: number; to: number };
  gpuPct?: number;
  vramGB?: number;
  vramTotalGB?: number;
}

export interface SessionResult {
  passed: number;
  failed: number;
  duration: string;
  summary: string;
}

// QUESTION: the design's health-card values aren't a fixed enum (Foundations v2 §4.7
// and SentinalSession.dc.html show free-form strings — "GOOD", "NONE 14m", "CPU 0 %",
// even a literal failure count "3" — each mapped to a color by a lookup with a
// text-1 fallback for anything unrecognized). Modeled as `string` here instead of
// the stricter union this field used to have; see QUESTIONS.md #10.
export type OverallHealth = "HEALTHY" | "POSSIBLY STALLED" | "UNHEALTHY" | "WAITING" | "FAILED";

export interface SessionHealth {
  progress?: string;
  activity?: string;
  resources?: string;
  stability?: string;
  errors?: string;
  overall: OverallHealth | string;
  confidence?: number;
  /** Missing entirely for some sessions (e.g. the STALLED fixture has no evidence array). */
  evidence?: string[];
}

/** [time, event kind, detail] — kind is one of EventKind (see protocol/EventKind.ts) plus free-form ones used only in the mock (e.g. WARNING). */
export type SessionEvent = [time: string, kind: string, detail: string];

export interface Session {
  id: string;
  machine: string;
  name: string;
  command?: string;
  project?: string;
  cwd?: string;
  state: SessionState;
  progress?: SessionProgress;
  metrics?: SessionMetrics;
  runningFor?: string;
  details?: string;
  result?: SessionResult;
  prompt?: string;
  health?: SessionHealth;
  events?: SessionEvent[];
}

export interface HistoryEntry {
  id: string;
  ok: boolean | null;
  command: string;
  duration: string;
  /** Machine id (matches `Machine.id` / `Session.machine`), not a display name. */
  machine?: string;
  project?: string;
  /** Relative display label, e.g. "Today" / "Yesterday" / "3 days ago" — see config.historyRelativeDates. */
  date?: string;
  /** Days since the command started, for the History screen's Today/7 days/30 days/All time filter (SentinalHistory.dc.html's own `dateMax` buckets). */
  daysAgo?: number;
  sessionId?: string;
}

export interface RecipeParam {
  key: string;
  default: string;
}

export interface Recipe {
  id: string;
  name: string;
  category: string;
  machine: string;
  template: string;
  params: Record<string, string>;
}

export interface NotificationRuleSet {
  finished: boolean;
  error: boolean;
  stalled: boolean;
  checkpoint: boolean;
  progress: boolean;
}

export type NotificationRules = Record<string, NotificationRuleSet>;

export interface JournalNotification {
  id: string;
  text: string;
  sessionId?: string;
}

export type AuthStatus = "setup" | "signedOut" | "signedIn" | "expired" | "locked" | "wrong";

export interface AuthState {
  status: AuthStatus;
  attempts: number;
  lockUntil: number | null;
}

export interface PairingToken {
  machineName: string;
  token: string;
  command: string;
  expiresAt: number;
}

export interface MachineToken {
  machineId: string;
  machineName: string;
  maskedToken: string;
  createdAt: string;
}
