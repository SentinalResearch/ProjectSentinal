import type { CaptureLevel } from "../protocol/CaptureLevel";
import type { SessionLog } from "../protocol/SessionLog";
import type { SessionOrigin } from "../protocol/SessionOrigin";
import type { SessionState } from "../protocol/SessionState";
import type { ProjectionFreshness } from "../protocol/ProjectionFreshness";

export type MachineStatus = "ONLINE" | "OFFLINE" | "AUTH_EXPIRED" | "DISCONNECTED" | "REVOKED";

export interface Machine {
  id: string;
  name: string;
  os: "Linux" | "macOS" | "Windows";
  hardware: string;
  status: MachineStatus;
  freshness?: ProjectionFreshness;
  syncedAt?: number;
  lastSeen?: string;
  endpoint?: string;
  capabilities?: string[];
  credentialStatus?: "ACTIVE" | "EXPIRED" | "MISSING";
}

export interface SessionProgress {
  current: number;
  total: number;
  unit: string;
}

export interface SessionMetrics {
  loss?: { from: number; to: number };
  cpuPct?: number;
  ramGB?: number;
  ramTotalGB?: number;
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
  /** Training numbers the watcher read from the run's output. */
  training?: TrainingView;
  events?: SessionEvent[];
  /** How the Session entered Sentinal; absent means UI. */
  origin?: SessionOrigin;
  /** What Sentinal can observe; absent means FULL. Authorization is enforced by the daemon. */
  capture?: CaptureLevel;
  /** Shell exit code; `null` on an ended Session means the exit code is unknown. */
  exitCode?: number | null;
  /** Executable used for this Session's shell; never a command line. */
  shell?: string;
  /** Adopted process log; active tails use PROCESS_LOG capture. */
  log?: SessionLog;
  /** Direct predecessor in a persisted linear Session history. */
  continuedFrom?: string;
  /** Unix milliseconds when this generation was created. */
  createdAt?: number;
  /** Unix milliseconds when this generation last changed lifecycle state. */
  updatedAt?: number;
}

export interface HistoryEntry {
  id: string;
  ok: boolean | null;
  command: string;
  duration: string;
  /** Machine id (matches `Machine.id` / `Session.machine`), not a display name. */
  machine?: string;
  project?: string;
  /** Working directory reported when the command started. */
  cwd?: string;
  /** Relative display label, e.g. "Today" / "Yesterday" / "3 days ago" — see config.historyRelativeDates. */
  date?: string;
  /** Days since the command started, for the History screen's Today/7 days/30 days/All time filter (SentinalHistory.dc.html's own `dateMax` buckets). */
  daysAgo?: number;
  sessionId?: string;
}

/** Aggregated history plus Machines that could not answer this query. */
export type HistoryResults = HistoryEntry[] & { unavailableMachines?: string[] };

export interface RecipeParam {
  key: string;
  default: string;
}

export interface Recipe {
  id: string;
  sourceId?: string;
  name: string;
  category: string;
  machine: string;
  machineIds?: string[];
  project?: string;
  template: string;
  params: Record<string, string>;
}

export interface NotificationRuleSet {
  finished: boolean;
  error: boolean;
  stalled: boolean;
  waiting: boolean;
  checkpoint: boolean;
  progress: boolean;
}

export type NotificationRules = Record<string, NotificationRuleSet>;

export interface JournalNotification {
  id: string;
  text: string;
  machineId?: string;
  sessionId?: string;
  category?: string;
  state?: string;
  attempts?: number;
  at?: number;
  delivered?: boolean;
  resultClass?: string;
  requestBytes?: number;
}

export interface NotificationDeliveryStatus {
  machineId: string;
  configured: boolean;
  server?: string;
  topic?: string;
  tokenConfigured: boolean;
  configurationState?: "UNAVAILABLE" | "CONFIGURED" | "INVALID";
  diagnostic?: string;
  reloadedAt?: number;
}

export type AuthStatus = "setup" | "signedOut" | "signedIn" | "expired" | "locked" | "wrong";

export interface AuthState {
  status: AuthStatus;
  attempts: number;
  lockUntil: number | null;
  message?: string;
}

/** One process under a Session, without its command line. */
export interface ProcessSummaryView {
  pid: number;
  name: string;
  cpuPct: number;
  rssBytes: number;
  gpuMemBytes?: number;
}

/** Host, daemon, link, and per-Session process facts for the Machine view. */
export interface MachineInsight {
  hostname: string;
  osVersion?: string;
  kernelVersion?: string;
  daemonVersion: string;
  startedAt: number;
  bind: string;
  link?: { state: string; controlUrl?: string; lastHeartbeatAt?: number; message?: string };
  processes: { sessionId: string; processes: ProcessSummaryView[] }[];
}

export type MetricRangeKey = "HOUR" | "DAY" | "WEEK";

/** One Machine-wide history point, as percentages. */
export interface MachineMetricSample {
  at: number;
  cpuPct: number;
  ramPct?: number;
  gpuPct?: number;
  vramPct?: number;
}

export interface TrainingView {
  loss?: number;
  firstLoss?: number;
  bestValLoss?: number;
  learningRate?: number;
  trend: "UNKNOWN" | "IMPROVING" | "STABLE" | "WORSENING";
}
