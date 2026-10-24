import { encodeTerminalFrame } from "../../frame";
import type { ClientRequest } from "../../protocol/ClientRequest";
import type { Command } from "../../protocol/Command";
import type { DaemonMessage } from "../../protocol/DaemonMessage";
import type { Envelope } from "../../protocol/Envelope";
import type { Event as WireEvent } from "../../protocol/Event";
import type { EventKind } from "../../protocol/EventKind";
import type { Health } from "../../protocol/Health";
import type { Machine as WireMachine } from "../../protocol/Machine";
import type { MachineDetails as WireMachineDetails } from "../../protocol/MachineDetails";
import type { MachineMetricPoint } from "../../protocol/MachineMetricPoint";
import type { MachinePing } from "../../protocol/MachinePing";
import type { Recipe as WireRecipe } from "../../protocol/Recipe";
import type { RegistryMachine } from "../../protocol/RegistryMachine";
import type { Session as WireSession } from "../../protocol/Session";
import type { ControlProjectionMessage } from "../../protocol/ControlProjectionMessage";
import type { ControlProjectionSnapshot } from "../../protocol/ControlProjectionSnapshot";
import { RegistryError, type DataAdapter, type HistoryFilters, type LiveEvent } from "../adapter";
import type {
  MachineInsight,
  MachineMetricSample,
  HistoryEntry, HistoryResults, JournalNotification, Machine, NotificationDeliveryStatus,
  NotificationRules as UiNotificationRules, Recipe, Session, SessionEvent, SessionHealth,
} from "../types";
import {
  AUTH_EXPIRED_EVENT,
  createAuthApi,
  defaultDaemonHttpUrl,
  type TokenStorage,
} from "./auth";
import {
  EventDeduper,
  RetryState,
  compositeId,
  localId,
  machineId,
  mapConcurrent,
} from "./machine-connections";
import { createRegistryApi } from "./registry";
import { applyProjectionMessage, projectionView, type ProjectionView } from "./projection-view";
import { health, type createHealthCollector } from "../health";
import { markLoad } from "../load-timing";

export { AUTH_EXPIRED_EVENT, defaultDaemonHttpUrl, SESSION_TOKEN_KEY } from "./auth";

const PROTOCOL_VERSION = 1;
const SOCKET_OPEN = 1;
const INTEGRITY_INTERVAL_MS = 60_000;
const QUERY_CONCURRENCY = 3;
const encoder = new TextEncoder();

export function defaultDaemonUrl(): string {
  return (
    import.meta.env.VITE_DAEMON_URL ??
    `${window.location.protocol === "https:" ? "wss" : "ws"}://${window.location.host}/api/v1/ws`
  );
}

export interface ApiSocket {
  readyState: number;
  binaryType: string;
  onopen: ((event: Event) => unknown) | null;
  onclose: ((event: CloseEvent) => unknown) | null;
  onerror: ((event: Event) => unknown) | null;
  onmessage: ((event: MessageEvent<string | ArrayBuffer | Blob>) => unknown) | null;
  send(data: string | ArrayBufferLike | Blob | ArrayBufferView): void;
  close(): void;
}

interface PendingReply {
  accepts: (message: DaemonMessage) => boolean;
  resolve: (message: DaemonMessage) => void;
  reject: (error: Error) => void;
  timeout: ReturnType<typeof setTimeout>;
}

export interface ApiAdapterOptions {
  daemonUrl?: string;
  daemonHttpUrl?: string;
  storage?: TokenStorage;
  socketFactory?: (url: string) => ApiSocket;
  fetcher?: typeof fetch;
  now?: () => number;
  /** Starts sending this browser's health samples to the control (4.H). */
  health?: ReturnType<typeof createHealthCollector>;
}

interface MachineConnection {
  id: string;
  transport: DaemonTransport;
  machine: Machine;
  sessions: Session[];
  retry: RetryState;
  retryAt: number;
  registry?: RegistryMachine;
  /** The daemon's own hardware line from GET_MACHINE; registry rows only know its version. */
  hardware?: string;
}

function isFullSessionUpdate(reply: DaemonMessage, id?: number): boolean {
  return reply.type === "SESSION_UPDATED" && reply.session.capture === "FULL" &&
    (id === undefined || reply.session.id === id);
}

function daemonError(message: string): Error {
  return new Error(`daemon request failed: ${message}`);
}

function formatDuration(milliseconds: number): string {
  const seconds = Math.max(0, Math.floor(milliseconds / 1_000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  return `${Math.floor(minutes / 60)}h${minutes % 60}m`;
}

function formatLastSeen(timestamp: number | null, now: number): string | undefined {
  return timestamp === null ? undefined : formatDuration(now - timestamp);
}

function mapMachine(machine: WireMachine): Machine {
  // The design's compact form: "Ryzen 9, 64 GB, RTX 4090 24 GB".
  const gigabytes = (bytes: number) => {
    const value = bytes / 1024 ** 3;
    return `${value.toFixed(value >= 10 ? 0 : 1)} GB`;
  };
  const hardware = [
    machine.cpu === "unknown" ? `${machine.cpu_cores} cores` : machine.cpu,
    machine.ram_bytes > 0 ? gigabytes(machine.ram_bytes) : null,
    ...machine.gpus.map((gpu) => (gpu.vram_bytes ? `${gpu.name} ${gigabytes(gpu.vram_bytes)}` : gpu.name)),
  ].filter(Boolean).join(", ");
  return {
    id: machine.id,
    name: machine.name,
    os: machine.os === "MACOS" ? "macOS" : "Linux",
    hardware,
    status: machine.status,
  };
}

function mapMachineInsight(machineId: string, details: WireMachineDetails): MachineInsight {
  return {
    hostname: details.hostname,
    osVersion: details.os_version ?? undefined,
    kernelVersion: details.kernel_version ?? undefined,
    daemonVersion: details.daemon_version,
    startedAt: details.started_at,
    bind: details.bind,
    link: details.link
      ? {
          state: details.link.state,
          controlUrl: details.link.control_url ?? undefined,
          lastHeartbeatAt: details.link.last_heartbeat_at ?? undefined,
          message: details.link.message ?? undefined,
        }
      : undefined,
    processes: details.processes.map((entry) => ({
      sessionId: compositeId(machineId, entry.session_id),
      processes: entry.processes.map((process) => ({
        pid: process.pid,
        name: process.name,
        cpuPct: process.cpu_percent,
        rssBytes: process.rss_bytes,
        gpuMemBytes: process.gpu_mem_bytes ?? undefined,
      })),
    })),
  };
}

function mapMetricPoint(point: MachineMetricPoint): MachineMetricSample {
  const percent = (used: number | null, total: number | null) =>
    used === null || total === null || total <= 0 ? undefined : Math.round((used / total) * 1000) / 10;
  return {
    at: point.at,
    cpuPct: Math.round(point.cpu_percent * 10) / 10,
    ramPct: percent(point.ram_used_bytes, point.ram_total_bytes),
    gpuPct: point.gpu_percent === null ? undefined : Math.round(point.gpu_percent * 10) / 10,
    vramPct: percent(point.vram_used_bytes, point.vram_total_bytes),
  };
}

function mapRegistryMachine(row: RegistryMachine, now: number): Machine {
  const expired = row.credential !== null && row.credential.expires_at <= now;
  const credentialStatus = row.role === "CONTROL" ? undefined
    : row.credential === null ? "MISSING" as const
      : expired ? "EXPIRED" as const : "ACTIVE" as const;
  const endpoint = row.endpoint
    ? `${row.endpoint.dns_name ?? row.endpoint.address}:${row.endpoint.port}`
    : undefined;
  return {
    id: row.machine_id,
    name: row.display_name,
    os: row.os === "MACOS" ? "macOS" : "Linux",
    hardware: row.daemon_version ? `Sentinal ${row.daemon_version}` : "Hardware unavailable",
    status: expired ? "AUTH_EXPIRED" : row.status,
    lastSeen: formatLastSeen(row.last_seen, now),
    endpoint,
    capabilities: row.capabilities,
    credentialStatus,
  };
}

export function mapSession(session: WireSession): Session {
  return {
    id: compositeId(session.machine_id, session.id),
    machine: session.machine_id,
    name: session.title ?? `Session ${session.id}`,
    project: session.project_id ?? undefined,
    cwd: session.cwd,
    state: session.state,
    runningFor: formatDuration(Date.now() - session.created_at),
    details: session.shell,
    shell: session.shell,
    events: [],
    health: session.health ? mapHealth(session.health) : undefined,
    training: session.training
      ? {
          loss: session.training.loss ?? undefined,
          firstLoss: session.training.first_loss ?? undefined,
          bestValLoss: session.training.best_val_loss ?? undefined,
          learningRate: session.training.learning_rate ?? undefined,
          trend: session.training.trend,
        }
      : undefined,
    origin: session.origin,
    capture: session.capture,
    exitCode: session.exit_code,
    log: session.log ?? undefined,
    continuedFrom: session.continued_from === null || session.continued_from === undefined
      ? undefined : compositeId(session.machine_id, session.continued_from),
    createdAt: session.created_at,
    updatedAt: session.updated_at,
  };
}

function mapHealth(health: Health): SessionHealth {
  return {
    progress: health.progress === null ? undefined : `${Math.round(health.progress)}%`,
    activity: health.activity,
    resources: health.resources,
    stability: health.stability,
    errors: String(health.errors),
    overall: health.overall.replaceAll("_", " "),
    evidence: health.evidence,
  };
}

function eventDetail(kind: EventKind): string {
  switch (kind.type) {
    case "COMMAND_STARTED": return kind.text;
    case "COMMAND_FINISHED": return `exit ${kind.exit_code} · ${formatDuration(kind.duration_ms)}`;
    case "PROCESS_STARTED": return `${kind.name} · pid ${kind.pid}`;
    case "PROCESS_EXITED": return `pid ${kind.pid} · ${kind.exit_code === null ? "exit unknown" : `exit ${kind.exit_code}`}`;
    case "PROGRESS_UPDATED": return `${kind.unit} ${kind.current}${kind.total === null ? "" : `/${kind.total}`}`;
    case "WARNING_DETECTED":
    case "ERROR_DETECTED": return kind.message;
    case "RESOURCE_HIGH": return `${kind.resource} ${kind.percent.toFixed(1)}%`;
    case "WAITING_FOR_INPUT": return kind.prompt ?? "waiting for input";
    case "POSSIBLY_STALLED": return `quiet for ${formatDuration(kind.silent_for_ms)}`;
    case "CHECKPOINT_CREATED": return kind.path;
    case "SERVER_STARTED": return kind.url ?? (kind.port === null ? "server listening" : `port ${kind.port}`);
  }
}

export function mapWatcherEvent(observed: WireEvent): SessionEvent {
  return [new Date(observed.at).toLocaleTimeString(), observed.kind.type, eventDetail(observed.kind)];
}

export function watcherEventPatch(observed: WireEvent): Partial<Session> | undefined {
  const kind = observed.kind;
  if (kind.type === "PROGRESS_UPDATED" && kind.total !== null) {
    return { progress: { current: kind.current, total: kind.total, unit: kind.unit } };
  }
  if (kind.type === "WAITING_FOR_INPUT") return { prompt: kind.prompt ?? undefined };
  if (kind.type === "SERVER_STARTED") {
    return { details: kind.url ?? (kind.port === null ? undefined : `Listening on port ${kind.port}`) };
  }
  return undefined;
}

function relativeDate(startedAt: number): { date: string; daysAgo: number } {
  const daysAgo = Math.max(0, Math.floor((Date.now() - startedAt) / 86_400_000));
  if (daysAgo === 0) return { date: "Today", daysAgo };
  if (daysAgo === 1) return { date: "Yesterday", daysAgo };
  return { date: `${daysAgo} days ago`, daysAgo };
}

function mapCommand(command: Command): HistoryEntry {
  const elapsed = (command.finished_at ?? Date.now()) - command.started_at;
  return {
    id: compositeId(command.machine_id, command.id),
    ok: command.exit_code === null ? null : command.exit_code === 0,
    command: command.text,
    duration: command.exit_code === null ? "running" : formatDuration(elapsed),
    machine: command.machine_id,
    cwd: command.cwd,
    ...relativeDate(command.started_at),
    sessionId: compositeId(command.machine_id, command.session_id),
  };
}

function workerSocketUrl(controlUrl: string, owner: string): string {
  const endpoint = new URL(controlUrl);
  endpoint.pathname = `/api/v1/machines/${encodeURIComponent(owner)}/ws`;
  endpoint.search = "";
  endpoint.hash = "";
  return endpoint.toString();
}

class DaemonTransport {
  private socket: ApiSocket | null = null;
  private connecting: Promise<ApiSocket> | null = null;
  private readonly pending: PendingReply[] = [];
  private readonly listeners = new Set<(event: LiveEvent) => void>();
  private latestGpuPct: number | undefined;
  private latestVramTotal = 0;
  private latestRamTotal = 0;
  private readonly eventDeduper = new EventDeduper();
  private everOpened = false;

  constructor(
    readonly url: string,
    private machine: string,
    private readonly socketFactory: (url: string) => ApiSocket,
    private readonly ownsBrowserAuthentication: boolean,
  ) {}

  setMachine(machine: string): void {
    this.machine = machine;
  }

  async request(request: ClientRequest, accepts: (message: DaemonMessage) => boolean): Promise<DaemonMessage> {
    const socket = await this.connect();
    return new Promise((resolve, reject) => {
      const pending: PendingReply = {
        accepts,
        resolve,
        reject,
        timeout: setTimeout(() => {
          const index = this.pending.indexOf(pending);
          if (index >= 0) this.pending.splice(index, 1);
          health.count("ERRORS", this.machine === "control" ? null : this.machine, "ERROR_TIMEOUT");
          reject(daemonError(`Machine '${this.machine}' did not answer within 10 seconds`));
        }, 10_000),
      };
      this.pending.push(pending);
      const envelope: Envelope<ClientRequest> = { version: PROTOCOL_VERSION, payload: request };
      socket.send(JSON.stringify(envelope));
    });
  }

  async sendBytes(session: number, bytes: Uint8Array): Promise<void> {
    const socket = await this.connect();
    socket.send(encodeTerminalFrame(session, bytes));
  }

  subscribe(listener: (event: LiveEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  reset(): void {
    const socket = this.socket;
    this.socket = null;
    this.connecting = null;
    socket?.close();
    this.rejectPending(daemonError("connection reset"));
  }

  private connect(): Promise<ApiSocket> {
    if (this.socket?.readyState === SOCKET_OPEN) return Promise.resolve(this.socket);
    if (this.connecting !== null) return this.connecting;
    if (this.everOpened) {
      health.count("WS_RECONNECTS", this.machine === "control" ? null : this.machine, "SOCKET_MACHINE");
    }
    const endpoint = new URL(this.url);
    this.connecting = new Promise((resolve, reject) => {
      const socket = this.socketFactory(endpoint.toString());
      socket.binaryType = "arraybuffer";
      let opened = false;
      socket.onopen = () => {
        opened = true;
        this.everOpened = true;
        this.socket = socket;
        this.connecting = null;
        resolve(socket);
      };
      socket.onerror = () => {
        if (!opened) {
          this.connecting = null;
          health.count("ERRORS", this.machine === "control" ? null : this.machine, "ERROR_UNREACHABLE");
          reject(daemonError("authentication or connection error"));
        }
      };
      socket.onclose = (event) => {
        if (this.socket === socket) this.socket = null;
        this.connecting = null;
        this.rejectPending(daemonError("connection closed"));
        if (this.ownsBrowserAuthentication && event.code === 4_001) {
          health.count("ERRORS", null, "ERROR_AUTH_EXPIRED");
          window.dispatchEvent(new Event(AUTH_EXPIRED_EVENT));
        }
      };
      socket.onmessage = (event) => this.handleMessage(event);
    });
    return this.connecting;
  }

  private handleMessage(event: MessageEvent<string | ArrayBuffer | Blob>): void {
    if (typeof event.data !== "string") return;
    let envelope: Envelope<DaemonMessage>;
    try {
      envelope = JSON.parse(event.data) as Envelope<DaemonMessage>;
    } catch {
      this.rejectPending(daemonError("daemon sent invalid JSON"));
      return;
    }
    if (envelope.version !== PROTOCOL_VERSION) {
      this.rejectPending(daemonError(`unsupported protocol version ${envelope.version}`));
      return;
    }
    const message = envelope.payload;
    if (message.type === "ERROR") {
      const pending = this.pending.shift();
      if (pending) {
        clearTimeout(pending.timeout);
        pending.reject(daemonError(message.message));
      }
      return;
    }
    if (message.type === "SESSION_UPDATED" && message.session.machine_id === this.machine) {
      this.emit({ type: "SESSION_UPDATED", session: mapSession(message.session) });
    } else if (message.type === "MACHINE_METRICS") {
      this.latestGpuPct = message.metrics.gpus.reduce<number | undefined>(
        (highest, gpu) => highest === undefined ? gpu.util_percent : Math.max(highest, gpu.util_percent), undefined,
      );
      this.latestVramTotal = message.metrics.gpus.reduce((total, gpu) => total + gpu.vram_total_bytes, 0);
      this.latestRamTotal = message.metrics.ram_total_bytes;
    } else if (message.type === "SESSION_METRICS") {
      const totals = message.metrics.totals;
      this.emit({
        type: "SESSION_METRICS",
        sessionId: compositeId(this.machine, message.metrics.session_id),
        metrics: {
          cpuPct: Math.round(totals.cpu_percent * 10) / 10,
          ramGB: Math.round((totals.rss_bytes / 1024 ** 3) * 10) / 10,
          ramTotalGB: this.latestRamTotal > 0 ? Math.round((this.latestRamTotal / 1024 ** 3) * 10) / 10 : undefined,
          gpuPct: totals.gpu_mem_bytes > 0 ? this.latestGpuPct : undefined,
          vramGB: this.latestVramTotal > 0 || totals.gpu_mem_bytes > 0
            ? Math.round((totals.gpu_mem_bytes / 1024 ** 3) * 10) / 10 : undefined,
          vramTotalGB: this.latestVramTotal > 0
            ? Math.round((this.latestVramTotal / 1024 ** 3) * 10) / 10 : undefined,
        },
      });
    } else if (message.type === "COMMAND_UPDATED" && message.command.machine_id === this.machine) {
      this.emit({
        type: "COMMAND_UPDATED",
        sessionId: compositeId(this.machine, message.command.session_id),
        command: message.command.finished_at === null ? message.command.text : undefined,
      });
    } else if (message.type === "EVENT" && this.eventDeduper.accept(this.machine, message.event.id)) {
      this.emit({
        type: "EVENT_APPENDED",
        sessionId: compositeId(this.machine, message.event.session_id),
        event: mapWatcherEvent(message.event),
        sessionPatch: watcherEventPatch(message.event),
      });
    }
    const index = this.pending.findIndex((pending) => pending.accepts(message));
    if (index >= 0) {
      const pending = this.pending.splice(index, 1)[0];
      clearTimeout(pending.timeout);
      pending.resolve(message);
    }
  }

  private emit(event: LiveEvent): void {
    for (const listener of this.listeners) listener(event);
  }

  private rejectPending(error: Error): void {
    for (const pending of this.pending.splice(0)) {
      clearTimeout(pending.timeout);
      pending.reject(error);
    }
  }
}

export function createApiAdapter(options: ApiAdapterOptions = {}): DataAdapter {
  const storage = options.storage ?? window.localStorage;
  const socketFactory = options.socketFactory ?? ((url: string) => new WebSocket(url));
  const fetcher = options.fetcher ?? window.fetch.bind(window);
  const now = options.now ?? Date.now;
  const authBaseUrl = options.daemonHttpUrl ?? defaultDaemonHttpUrl();
  const controlSocketUrl = options.daemonUrl ?? defaultDaemonUrl();
  const listeners = new Set<(event: LiveEvent) => void>();
  const connections = new Map<string, MachineConnection>();
  const controlTransport = new DaemonTransport(controlSocketUrl, "control", socketFactory, true);
  const baseRegistry = createRegistryApi({ baseUrl: authBaseUrl, fetcher, storage });
  let controlMachineId: string | null = null;
  let initialization: Promise<void> | null = null;
  let pollTimer: ReturnType<typeof setInterval> | null = null;
  let projection: ProjectionView | null = null;
  let projectionSocket: ApiSocket | null = null;
  let projectionReconnect: ReturnType<typeof setTimeout> | null = null;
  let projectionReload: Promise<boolean> | null = null;
  let legacyRefresh: Promise<void> | null = null;
  let firstLegacyRefresh = true;
  const recipeRoutes = new Map<string, { machineId: string; sourceId: string }>();

  const emit = (event: LiveEvent) => {
    for (const listener of listeners) listener(event);
  };
  controlTransport.subscribe(emit);

  const newConnection = (id: string, machine: Machine, sessions: Session[]): MachineConnection => {
    const transport = id === controlMachineId
      ? controlTransport
      : new DaemonTransport(workerSocketUrl(controlSocketUrl, id), id, socketFactory, false);
    if (transport !== controlTransport) transport.subscribe(emit);
    return {
      id,
      transport,
      machine,
      sessions,
      retry: new RetryState({ baseMs: 2_000, maximumMs: 60_000, wakeAfterMs: 30_000 }),
      retryAt: 0,
    };
  };

  const projectionUrl = (revision: number): string => {
    const endpoint = new URL(controlSocketUrl);
    endpoint.pathname = "/api/v1/control/live";
    endpoint.search = `?after=${revision}`;
    endpoint.hash = "";
    return endpoint.toString();
  };

  const installProjection = (next: ProjectionView): void => {
    projection = next;
    controlMachineId = next.controlMachineId;
    health.setControlMachine(next.controlMachineId);
    controlTransport.setMachine(next.controlMachineId);
    const present = new Set(next.machines.keys());
    for (const [id, connection] of connections) {
      if (!present.has(id)) {
        connection.transport.reset();
        connections.delete(id);
      }
    }
    for (const [id, projected] of next.machines) {
      const existing = connections.get(id);
      if (existing) {
        existing.machine = projected.machine;
        existing.sessions = projected.sessions;
      } else {
        connections.set(id, newConnection(id, projected.machine, projected.sessions));
      }
    }
    if (pollTimer !== null) {
      clearInterval(pollTimer);
      pollTimer = null;
    }
    emit({
      type: "DATA_RESET",
      machines: [...connections.values()].map((connection) => connection.machine),
      sessions: [...connections.values()].flatMap((connection) => connection.sessions),
    });
  };

  const closeProjectionStream = (): void => {
    if (projectionReconnect !== null) clearTimeout(projectionReconnect);
    projectionReconnect = null;
    projectionSocket?.close();
    projectionSocket = null;
  };

  const loadProjection = async (): Promise<boolean> => {
    if (projectionReload !== null) return projectionReload;
    const reload: Promise<boolean> = (async () => {
      let response: Response;
      try {
        response = await fetcher(`${authBaseUrl}/control/snapshot`, {
          credentials: "same-origin",
          cache: "no-store",
        });
      } catch {
        return false;
      }
      if (response.status === 404 || response.status === 501) return false;
      if (!response.ok) {
        if (response.status === 401) {
          health.count("ERRORS", null, "ERROR_AUTH_EXPIRED");
          window.dispatchEvent(new Event(AUTH_EXPIRED_EVENT));
        }
        throw daemonError(`control projection request failed with status ${response.status}`);
      }
      const next = projectionView(await response.json() as ControlProjectionSnapshot, now());
      if (next === null) return false;
      markLoad("snapshot");
      installProjection(next);
      startProjectionStream();
      await refreshProjectedLegacy();
      return true;
    })().finally(() => {
      if (projectionReload === reload) projectionReload = null;
    });
    projectionReload = reload;
    return reload;
  };

  const startProjectionStream = (): void => {
    if (projection === null || projectionSocket !== null) return;
    const socket = socketFactory(projectionUrl(projection.revision));
    projectionSocket = socket;
    socket.onopen = () => markLoad("live");
    socket.onmessage = (event: MessageEvent<string | ArrayBuffer | Blob>) => {
      if (typeof event.data !== "string" || projection === null) return;
      let message: ControlProjectionMessage;
      try {
        message = JSON.parse(event.data) as ControlProjectionMessage;
      } catch {
        void loadProjection();
        return;
      }
      const events = applyProjectionMessage(projection, message, now());
      if (events === null) {
        closeProjectionStream();
        void loadProjection();
        return;
      }
      if (message.type === "PROJECTION_DELTA" && message.machine_id) {
        const projected = projection.machines.get(message.machine_id);
        const connection = connections.get(message.machine_id);
        if (projected && connection) {
          connection.machine = projected.machine;
          connection.sessions = projected.sessions;
        }
      }
      for (const event of events) emit(event);
    };
    socket.onclose = (event) => {
      if (projectionSocket !== socket) return;
      projectionSocket = null;
      if (event.code === 4_001) {
        health.count("ERRORS", null, "ERROR_AUTH_EXPIRED");
        window.dispatchEvent(new Event(AUTH_EXPIRED_EVENT));
        return;
      }
      if (projection !== null && projectionReconnect === null) {
        projectionReconnect = setTimeout(() => {
          projectionReconnect = null;
          health.count("WS_RECONNECTS", controlMachineId, "SOCKET_DASHBOARD");
          startProjectionStream();
        }, 2_000);
      }
    };
    socket.onerror = () => undefined;
  };

  const upsertWorker = (row: RegistryMachine): MachineConnection | null => {
    if (row.role !== "WORKER") return null;
    const machine = mapRegistryMachine(row, now());
    let connection = connections.get(row.machine_id);
    if (!connection) {
      const transport = new DaemonTransport(
        workerSocketUrl(controlSocketUrl, row.machine_id), row.machine_id, socketFactory, false,
      );
      transport.subscribe(emit);
      connection = {
        id: row.machine_id,
        transport,
        machine,
        sessions: [],
        retry: new RetryState({ baseMs: 2_000, maximumMs: 60_000, wakeAfterMs: 30_000 }),
        retryAt: 0,
        registry: row,
      };
      connections.set(row.machine_id, connection);
    } else {
      connection.registry = row;
      const status = machine.status === "ONLINE" && connection.machine.status === "OFFLINE"
        ? "OFFLINE" : machine.status;
      connection.machine = { ...connection.machine, ...machine, status, hardware: connection.hardware ?? machine.hardware };
    }
    return connection;
  };

  const loadRegistry = async (): Promise<void> => {
    let rows: RegistryMachine[];
    try {
      rows = await baseRegistry.list();
    } catch (error) {
      if (error instanceof RegistryError && error.code === "NOT_CONTROL") return;
      return;
    }
    const present = new Set<string>();
    for (const row of rows) {
      present.add(row.machine_id);
      if (row.machine_id === controlMachineId) {
        const connection = connections.get(row.machine_id);
        if (connection) {
          // Remembered so live queries keep the owner's display name, not the hostname.
          connection.registry = row;
          // The control's liveness comes from its own socket, not its registry row.
          connection.machine = {
            ...connection.machine,
            ...mapRegistryMachine(row, now()),
            status: connection.machine.status,
            lastSeen: connection.machine.lastSeen,
            hardware: connection.hardware ?? connection.machine.hardware,
          };
          emit({ type: "MACHINE_UPDATED", machine: connection.machine });
        }
      } else {
        const connection = upsertWorker(row);
        if (connection) emit({ type: "MACHINE_UPDATED", machine: connection.machine });
      }
    }
    for (const [id, connection] of connections) {
      if (id !== controlMachineId && !present.has(id)) {
        connection.transport.reset();
        connections.delete(id);
        emit({
          type: "DATA_RESET",
          machines: [...connections.values()].map((entry) => entry.machine),
          sessions: [...connections.values()].flatMap((entry) => entry.sessions),
        });
      }
    }
  };

  const initialize = async (): Promise<void> => {
    if (await loadProjection()) return;
    const reply = await controlTransport.request({ type: "GET_MACHINE" }, (message) => message.type === "MACHINE");
    if (reply.type !== "MACHINE") throw daemonError("missing control Machine");
    controlMachineId = reply.machine.id;
    health.setControlMachine(reply.machine.id);
    controlTransport.setMachine(reply.machine.id);
    const current = connections.get(reply.machine.id);
    if (current) {
      current.machine = mapMachine(reply.machine);
      current.hardware = current.machine.hardware;
      current.retry.succeeded();
      current.retryAt = 0;
    } else {
      connections.set(reply.machine.id, {
        id: reply.machine.id,
        transport: controlTransport,
        machine: mapMachine(reply.machine),
        hardware: mapMachine(reply.machine).hardware,
        sessions: [],
        retry: new RetryState({ baseMs: 2_000, maximumMs: 60_000, wakeAfterMs: 30_000 }),
        retryAt: 0,
      });
    }
    await loadRegistry();
  };

  const ensureInitialized = async (): Promise<void> => {
    if (controlMachineId !== null) return;
    if (initialization === null) {
      const run = initialize();
      initialization = run;
      // A sign-in replaces `initialization`; an older run must not clear the new one.
      run.then(() => undefined, () => undefined).finally(() => {
        if (initialization === run) initialization = null;
      });
    }
    await initialization;
  };

  const queryMachine = async (connection: MachineConnection): Promise<void> => {
    const reply = await connection.transport.request({ type: "GET_MACHINE" }, (message) => message.type === "MACHINE");
    if (reply.type !== "MACHINE" || reply.machine.id !== connection.id) {
      throw daemonError(`Machine '${connection.id}' returned another Machine identity`);
    }
    const registry = connection.registry;
    const metadata = registry ? mapRegistryMachine(registry, now()) : undefined;
    connection.machine = {
      ...mapMachine(reply.machine),
      name: registry?.display_name ?? reply.machine.name,
      endpoint: metadata?.endpoint,
      capabilities: registry?.capabilities,
      credentialStatus: metadata?.credentialStatus,
      lastSeen: "0s",
      status: "ONLINE",
    };
    connection.hardware = connection.machine.hardware;
    connection.retry.succeeded();
    connection.retryAt = 0;
  };

  const querySessions = async (connection: MachineConnection): Promise<Session[]> => {
    const reply = await connection.transport.request({ type: "LIST_SESSIONS" }, (message) => message.type === "SESSIONS");
    if (reply.type !== "SESSIONS") return connection.sessions;
    // A row stamped with another Machine ID is never shown or routed. Dropping
    // it (not the whole Machine) keeps the valid Sessions and a steady status.
    const owned = reply.sessions.filter((session) => session.machine_id === connection.id);
    const mapped = owned.map(mapSession);
    connection.sessions = mapped;
    return mapped;
  };

  const markFailure = (connection: MachineConnection): void => {
    connection.retryAt = connection.retry.failedAt(now());
    if (connection.machine.status === "ONLINE") {
      connection.machine = { ...connection.machine, status: "OFFLINE", lastSeen: connection.machine.lastSeen ?? "0s" };
    }
    emit({ type: "MACHINE_UPDATED", machine: connection.machine });
  };

  const queryEligible = (connection: MachineConnection): boolean =>
    connection.machine.status !== "AUTH_EXPIRED" && connection.machine.status !== "DISCONNECTED" &&
    connection.machine.status !== "REVOKED";

  const refreshConnection = async (
    connection: MachineConnection,
    withSessions: boolean,
    skipMachine = false,
  ): Promise<void> => {
    try {
      if (!skipMachine) await queryMachine(connection);
      if (withSessions) await querySessions(connection);
      emit({ type: "MACHINE_UPDATED", machine: connection.machine });
      if (withSessions) for (const session of connection.sessions) emit({ type: "SESSION_UPDATED", session });
    } catch {
      markFailure(connection);
    }
  };

  const refreshProjectedLegacy = async (): Promise<void> => {
    if (projection === null || projection.legacyMachineIds.size === 0) return;
    const legacy = [...projection.legacyMachineIds]
      .map((id) => connections.get(id))
      .filter((connection): connection is MachineConnection => connection !== undefined && queryEligible(connection));
    await mapConcurrent(legacy, QUERY_CONCURRENCY, (connection) => refreshConnection(connection, true));
  };

  const refreshDue = async (): Promise<void> => {
    await ensureInitialized();
    if (projection !== null) return;
    await refreshLegacy();
  };

  const refreshLegacy = async (): Promise<void> => {
    legacyRefresh ??= (async () => {
      const initial = firstLegacyRefresh;
      firstLegacyRefresh = false;
      if (!initial) await loadRegistry();
      const at = now();
      const due = [...connections.values()].filter((connection) =>
        queryEligible(connection) && connection.retryAt <= at);
      await mapConcurrent(due, QUERY_CONCURRENCY, (connection) =>
        refreshConnection(connection, true, initial && connection.id === controlMachineId));
    })().finally(() => { legacyRefresh = null; });
    await legacyRefresh;
  };

  const getMachines = async (): Promise<Machine[]> => {
    await ensureInitialized();
    if (projection !== null) return [...connections.values()].map((connection) => connection.machine);
    await refreshLegacy();
    return [...connections.values()].map((connection) => connection.machine);
  };

  const getSessions = async (): Promise<Session[]> => {
    await ensureInitialized();
    if (projection !== null) return [...connections.values()].flatMap((connection) => connection.sessions);
    await refreshLegacy();
    return [...connections.values()].flatMap((connection) => connection.sessions);
  };

  const owner = (sessionId: string): MachineConnection => {
    const id = machineId(sessionId);
    const connection = connections.get(id);
    if (!connection) throw daemonError(`unknown owning Machine '${id}'`);
    return connection;
  };

  const liveOwner = (sessionId: string, operation: string): MachineConnection => {
    const connection = owner(sessionId);
    if (connection.machine.status !== "ONLINE") {
      throw daemonError(
        `Machine '${connection.id}' is ${connection.machine.freshness ?? connection.machine.status}; ${operation} requires a live authoritative route`,
      );
    }
    return connection;
  };

  const outputUrl = (sessionId: string, offset: number, limit: number): string => {
    const id = machineId(sessionId);
    const local = localId(sessionId);
    const prefix = id === controlMachineId ? authBaseUrl : `${authBaseUrl}/machines/${encodeURIComponent(id)}`;
    return `${prefix}/sessions/${local}/output?offset=${offset}&limit=${limit}`;
  };

  const auth = createAuthApi({
    baseUrl: authBaseUrl,
    fetcher,
    storage,
    resetTransport: () => {
      closeProjectionStream();
      projection = null;
      for (const connection of connections.values()) connection.transport.reset();
      connections.clear();
      controlMachineId = null;
      firstLegacyRefresh = true;
      // Loads started before this sign-in or sign-out ran with the old credentials.
      initialization = null;
      projectionReload = null;
    },
  });

  const registry: DataAdapter["registry"] = {
    ...baseRegistry,
    async rename(id, displayName) {
      const row = await baseRegistry.rename(id, displayName);
      upsertWorker(row);
      return row;
    },
    async revoke(id) {
      await baseRegistry.revoke(id);
      const connection = connections.get(id);
      connection?.transport.reset();
      connections.delete(id);
    },
  };

  const adapter: DataAdapter = {
    getMachines,
    getSessions,
    async createSession(create = {}) {
      await ensureInitialized();
      if (controlMachineId === null) throw daemonError("missing control Machine");
      // Workers are reached through the control's routed connection, as Recipes are.
      const target = create.machineId ?? controlMachineId;
      const connection = connections.get(target);
      if (!connection) throw daemonError(`unknown Machine '${target}'`);
      if (connection.machine.status !== "ONLINE") {
        throw daemonError(`Machine '${target}' is not live; the Session was not started`);
      }
      const message = await connection.transport.request({
        type: "CREATE_SESSION",
        shell: create.shell ?? null,
        cwd: create.cwd ?? null,
        cols: create.cols ?? 80,
        rows: create.rows ?? 24,
        origin: "UI",
        title: create.title ?? null,
        command: null,
      }, (reply) =>
        isFullSessionUpdate(reply) && reply.type === "SESSION_UPDATED" && reply.session.machine_id === target);
      if (message.type !== "SESSION_UPDATED") throw daemonError("missing created Session");
      return mapSession(message.session);
    },
    async continueSession(sessionId) {
      await ensureInitialized();
      const connection = liveOwner(sessionId, "Continue");
      const source = localId(sessionId);
      const message = await connection.transport.request(
        { type: "CONTINUE_SESSION", session_id: source },
        (reply) => reply.type === "SESSION_UPDATED" && reply.session.capture === "FULL" &&
          reply.session.machine_id === connection.id && reply.session.continued_from === source,
      );
      if (message.type !== "SESSION_UPDATED") throw daemonError("missing continued Session");
      return mapSession(message.session);
    },
    async replayOutput(sessionId, onChunk, replay = {}) {
      await ensureInitialized();
      owner(sessionId);
      const maximum = Math.min(64 * 1024 * 1024, Math.max(1, Math.floor(replay.maxBytes ?? 64 * 1024 * 1024)));
      let offset = 0;
      let bytesRead = 0;
      let warning: string | undefined;
      for (let page = 0; page < 256; page += 1) {
        const limit = Math.min(262144, maximum - bytesRead);
        if (limit <= 0) return warning ? { warning, bytesRead, truncated: true } : { bytesRead, truncated: true };
        const response = await fetcher(outputUrl(sessionId, offset, limit), {
          credentials: "same-origin",
        });
        if (!response.ok) {
          let message = `saved output request failed with status ${response.status}`;
          try {
            const failure = (await response.json()) as { message?: string };
            if (failure.message) message = failure.message;
          } catch {
            // Keep the bounded status-only fallback for a non-JSON proxy error.
          }
          throw daemonError(message);
        }
        const chunk = new Uint8Array(await response.arrayBuffer());
        if (chunk.length > limit) throw daemonError("saved output exceeded the requested page limit");
        if (chunk.length > 0) onChunk(chunk);
        bytesRead += chunk.length;
        warning = response.headers.get("x-sentinal-output-warning") ?? warning;
        if (response.headers.get("x-sentinal-output-complete") === "true") {
          return warning ? { warning, bytesRead } : { bytesRead };
        }
        if (bytesRead >= maximum) return warning ? { warning, bytesRead, truncated: true } : { bytesRead, truncated: true };
        const next = Number(response.headers.get("x-sentinal-output-next-offset"));
        if (!Number.isSafeInteger(next) || next <= offset) {
          throw daemonError("saved output returned an invalid continuation offset");
        }
        offset = next;
      }
      throw daemonError("saved output exceeded the 64 MiB replay limit");
    },
    resolveTerminal(sessionId) {
      const connection = liveOwner(sessionId, "terminal attach");
      return { websocketUrl: connection.transport.url, localSessionId: localId(sessionId), machineId: connection.id };
    },
    async refreshSessionDetail(sessionId) {
      await ensureInitialized();
      let connection: MachineConnection;
      try {
        connection = liveOwner(sessionId, "Session detail");
      } catch {
        return;
      }
      try {
        const reply = await connection.transport.request(
          { type: "LIST_SESSIONS" },
          (message) => message.type === "SESSIONS",
        );
        if (reply.type !== "SESSIONS") return;
        const local = localId(sessionId);
        const match = reply.sessions.find((session) => session.id === local);
        if (match) emit({ type: "SESSION_UPDATED", session: mapSession(match) });
      } catch {
        markFailure(connection);
      }
    },
    async getRecipes() {
      await ensureInitialized();
      recipeRoutes.clear();
      const targets = [...connections.values()].filter(queryEligible);
      const catalogs = await mapConcurrent(targets, QUERY_CONCURRENCY, async (connection) => {
        try {
          const reply = await connection.transport.request(
            { type: "LIST_RECIPES" },
            (message) => message.type === "RECIPES",
          );
          if (reply.type !== "RECIPES") return [];
          return reply.recipes
            .filter((recipe) => recipe.machine_id === null || recipe.machine_id === connection.id)
            .map((recipe): Recipe => {
              const id = `${connection.id}/${recipe.id}`;
              recipeRoutes.set(id, { machineId: connection.id, sourceId: recipe.id });
              return {
                id,
                sourceId: recipe.id,
                name: recipe.name,
                category: recipe.category ?? "OTHER",
                project: recipe.project ?? undefined,
                machine: connection.id,
                machineIds: [connection.id],
                template: recipe.command,
                params: Object.fromEntries(recipe.params.map((param) => [param.name, param.default ?? ""])),
              };
            });
        } catch {
          markFailure(connection);
          return [];
        }
      });
      return catalogs.flat();
    },
    async getHistory(filters: HistoryFilters = {}) {
      await ensureInitialized();
      let targets: MachineConnection[];
      if (filters.sessionId !== undefined) targets = [owner(filters.sessionId)];
      else if (filters.machine !== undefined) {
        const connection = connections.get(filters.machine);
        targets = connection ? [connection] : [];
      } else targets = [...connections.values()].filter(queryEligible);
      const settled = await mapConcurrent(targets, QUERY_CONCURRENCY, async (connection) => {
        try {
          const request: ClientRequest = filters.query?.trim()
            ? { type: "SEARCH_COMMANDS", query: filters.query.trim(), limit: 500 }
            : { type: "GET_HISTORY", session_id: filters.sessionId === undefined ? null : localId(filters.sessionId), limit: 500 };
          const message = await connection.transport.request(request, (reply) => reply.type === "HISTORY");
          if (message.type !== "HISTORY" || message.commands.some((command) => command.machine_id !== connection.id)) {
            throw daemonError(`Machine '${connection.id}' returned foreign history`);
          }
          return { connection, commands: message.commands, failed: false as const };
        } catch {
          markFailure(connection);
          return { connection, commands: [] as Command[], failed: true as const };
        }
      });
      const unavailable = settled.filter((result) => result.failed).map((result) => result.connection.id);
      const projects = new Map(
        [...connections.values()].flatMap((connection) => connection.sessions).map((session) => [session.id, session.project]),
      );
      const results = settled.flatMap((result) => result.commands).map((command) => {
        const entry = mapCommand(command);
        return { ...entry, project: projects.get(entry.sessionId ?? "") };
      }).filter((entry) => !filters.project || entry.project === filters.project).filter((entry) => {
        if (!filters.result) return true;
        if (filters.result === "running") return entry.ok === null;
        return filters.result === "ok" ? entry.ok === true : entry.ok === false;
      }) as HistoryResults;
      if (unavailable.length > 0) results.unavailableMachines = unavailable;
      return results;
    },
    subscribe(listener) {
      listeners.add(listener);
      void ensureInitialized().then(() => {
        if (projection === null && listeners.has(listener) && pollTimer === null) {
          pollTimer = setInterval(() => void refreshDue(), INTEGRITY_INTERVAL_MS);
        }
      }).catch(() => undefined);
      const onVisibility = () => {
        if (document.visibilityState !== "visible") return;
        if (projection !== null) {
          if (projectionSocket === null) startProjectionStream();
          return;
        }
        const at = now();
        for (const connection of connections.values()) connection.retryAt = connection.retry.resumeAt(at);
        void refreshDue();
      };
      document.addEventListener("visibilitychange", onVisibility);
      return () => {
        listeners.delete(listener);
        document.removeEventListener("visibilitychange", onVisibility);
        if (listeners.size === 0 && pollTimer !== null) {
          clearInterval(pollTimer);
          pollTimer = null;
        }
      };
    },
    async sendInput(sessionId, data) {
      await ensureInitialized();
      await liveOwner(sessionId, "terminal input").transport.sendBytes(localId(sessionId), encoder.encode(data));
    },
    async sendSignal(sessionId, signal) {
      await ensureInitialized();
      const connection = liveOwner(sessionId, signal === "SIGINT" ? "terminal input" : "kill");
      const local = localId(sessionId);
      if (signal === "SIGINT") {
        await connection.transport.sendBytes(local, new Uint8Array([3]));
        return;
      }
      await connection.transport.request(
        { type: "KILL", session_id: local },
        (reply) => isFullSessionUpdate(reply, local) && reply.type === "SESSION_UPDATED" &&
          reply.session.machine_id === connection.id,
      );
    },
    diagnostics: {
      async ping(machineId, samples = 5) {
        await ensureInitialized();
        const connection = connections.get(machineId);
        if (!connection) throw daemonError(`unknown Machine '${machineId}'`);
        const roundTripsMs: number[] = [];
        let failed = 0;
        let error: string | undefined;
        for (let sample = 0; sample < samples; sample += 1) {
          const started = performance.now();
          try {
            await connection.transport.request({ type: "GET_MACHINE" }, (message) => message.type === "MACHINE");
            roundTripsMs.push(performance.now() - started);
          } catch (failure) {
            failed += 1;
            error = failure instanceof Error ? failure.message : String(failure);
          }
        }
        return error === undefined ? { roundTripsMs, failed } : { roundTripsMs, failed, error };
      },
      async controlPing(machineId) {
        const response = await fetcher(`${authBaseUrl}/machines/${encodeURIComponent(machineId)}/ping`, {
          credentials: "same-origin",
          cache: "no-store",
        });
        if (!response.ok) {
          let message = `control ping failed with status ${response.status}`;
          try {
            message = ((await response.json()) as { message?: string }).message ?? message;
          } catch {
            // Keep the status-only message for a non-JSON proxy error.
          }
          throw daemonError(message);
        }
        const ping = (await response.json()) as MachinePing;
        return {
          roundTripsMs: ping.round_trips_us.map((microseconds) => microseconds / 1_000),
          failed: ping.failed,
          ...(ping.error === null ? {} : { error: ping.error }),
          path: ping.path,
          ...(ping.relay === null ? {} : { relay: ping.relay }),
        };
      },
    },
    async runRecipe(recipeId, params, targetMachineId) {
      await ensureInitialized();
      const route = recipeRoutes.get(recipeId);
      if (!route || route.machineId !== targetMachineId) {
        throw daemonError(`Recipe '${recipeId}' is not available on Machine '${targetMachineId}'`);
      }
      const connection = connections.get(targetMachineId);
      if (!connection) throw daemonError(`unknown Recipe Machine '${targetMachineId}'`);
      if (connection.machine.status !== "ONLINE") {
        throw daemonError(`Machine '${targetMachineId}' is not live; Recipe execution was not attempted`);
      }
      const reply = await connection.transport.request(
        { type: "RUN_RECIPE", recipe_id: route.sourceId, values: params, session_id: null },
        (message) => message.type === "SESSION_UPDATED" && message.session.machine_id === targetMachineId,
      );
      if (reply.type !== "SESSION_UPDATED") throw daemonError("missing Recipe Session");
      return { sessionId: compositeId(targetMachineId, reply.session.id) };
    },
    async saveRecipe(targetMachineId, recipe) {
      await ensureInitialized();
      const connection = connections.get(targetMachineId);
      if (!connection) throw daemonError(`unknown Recipe Machine '${targetMachineId}'`);
      if (connection.machine.status !== "ONLINE") {
        throw daemonError(`Machine '${targetMachineId}' is not live; Recipe save was not attempted`);
      }
      const wire: WireRecipe = {
        id: recipe.sourceId ?? recipe.id,
        name: recipe.name,
        category: recipe.category || null,
        project: recipe.project ?? null,
        machine_id: null,
        command: recipe.template,
        params: Object.entries(recipe.params).map(([name, value]) => ({
          name,
          default: value || null,
          description: null,
        })),
      };
      await connection.transport.request(
        { type: "SAVE_RECIPE", recipe: wire },
        (message) => message.type === "RECIPES",
      );
    },
    registry,
    auth,
    machineInsights: {
      async details(id) {
        await ensureInitialized();
        const connection = connections.get(id);
        if (!connection) throw daemonError(`unknown Machine '${id}'`);
        const reply = await connection.transport.request(
          { type: "GET_MACHINE_DETAILS" },
          (message) => message.type === "MACHINE_DETAILS",
        );
        if (reply.type !== "MACHINE_DETAILS") throw daemonError(`Machine '${id}' sent no details`);
        return mapMachineInsight(id, reply.details);
      },
      async metrics(id, range) {
        await ensureInitialized();
        const connection = connections.get(id);
        if (!connection) throw daemonError(`unknown Machine '${id}'`);
        const reply = await connection.transport.request(
          { type: "GET_MACHINE_METRICS", range },
          (message) => message.type === "MACHINE_METRICS_HISTORY" && message.range === range,
        );
        if (reply.type !== "MACHINE_METRICS_HISTORY") throw daemonError(`Machine '${id}' sent no history`);
        return reply.points.map(mapMetricPoint);
      },
    },
    notifications: {
      async getRules() {
        await ensureInitialized();
        const output: UiNotificationRules = {};
        await mapConcurrent([...connections.values()].filter(queryEligible), QUERY_CONCURRENCY, async (connection) => {
          try {
            const reply = await connection.transport.request(
              { type: "GET_NOTIFICATION_RULES" },
              (message) => message.type === "NOTIFICATION_RULES",
            );
            if (reply.type !== "NOTIFICATION_RULES") return;
            output[`${connection.id}:default`] = reply.rules.default;
            for (const override of reply.rules.sessions) {
              output[compositeId(connection.id, override.session_id)] = override.rules;
            }
          } catch {
            markFailure(connection);
          }
        });
        return output;
      },
      async setRule(id, rules) {
        await ensureInitialized();
        const defaultSuffix = ":default";
        const isDefault = id.endsWith(defaultSuffix);
        const targetMachineId = isDefault ? id.slice(0, -defaultSuffix.length) : machineId(id);
        const connection = connections.get(targetMachineId);
        if (!connection) throw daemonError(`unknown notification Machine '${targetMachineId}'`);
        await connection.transport.request(
          { type: "SET_NOTIFICATION_RULE", session_id: isDefault ? null : localId(id), rules },
          (message) => message.type === "NOTIFICATION_RULES",
        );
      },
      async getJournal() {
        await ensureInitialized();
        const groups = await mapConcurrent(
          [...connections.values()].filter(queryEligible), QUERY_CONCURRENCY,
          async (connection): Promise<JournalNotification[]> => {
            try {
              const reply = await connection.transport.request(
                { type: "GET_NOTIFICATION_JOURNAL" },
                (message) => message.type === "NOTIFICATION_JOURNAL",
              );
              if (reply.type !== "NOTIFICATION_JOURNAL") return [];
              return reply.entries.map((entry) => ({
                id: `${connection.id}:${entry.id}`,
                text: entry.text,
                machineId: connection.id,
                sessionId: compositeId(connection.id, entry.session_id),
                category: entry.category ?? undefined,
                state: entry.state ?? (entry.delivered ? "DELIVERED" : undefined),
                attempts: entry.attempts ?? undefined,
                at: entry.at,
                delivered: entry.delivered,
                resultClass: entry.result_class ?? undefined,
                requestBytes: entry.request_bytes ?? undefined,
              }));
            } catch {
              markFailure(connection);
              return [];
            }
          },
        );
        return groups.flat();
      },
      async getStatus() {
        await ensureInitialized();
        const groups = await mapConcurrent(
          [...connections.values()].filter(queryEligible), QUERY_CONCURRENCY,
          async (connection): Promise<NotificationDeliveryStatus[]> => {
            try {
              const reply = await connection.transport.request(
                { type: "GET_NOTIFICATION_RULES" },
                (message) => message.type === "NOTIFICATION_RULES",
              );
              if (reply.type !== "NOTIFICATION_RULES") return [];
              return [{
                machineId: connection.id,
                configured: reply.status.configured,
                server: reply.status.server ?? undefined,
                topic: reply.status.topic ?? undefined,
                tokenConfigured: reply.status.token_configured,
                configurationState: reply.status.configuration_state ?? undefined,
                diagnostic: reply.status.diagnostic ?? undefined,
                reloadedAt: reply.status.reloaded_at ?? undefined,
              }];
            } catch {
              return [];
            }
          },
        );
        return groups.flat();
      },
    },
  };
  // Failed actions are counted by code (never the message) for the health page.
  const { createSession, sendInput } = adapter;
  adapter.createSession = async (create) => {
    try {
      return await createSession(create);
    } catch (failure) {
      health.count("ERRORS", create?.machineId ?? null, "ERROR_CREATE");
      throw failure;
    }
  };
  adapter.sendInput = async (sessionId, data) => {
    try {
      return await sendInput(sessionId, data);
    } catch (failure) {
      health.count("ERRORS", machineId(sessionId), "ERROR_INPUT");
      throw failure;
    }
  };
  if (options.health) {
    const url = `${authBaseUrl}/health/samples`;
    options.health.start((batch, beacon) => {
      const body = JSON.stringify(batch);
      if (beacon && typeof navigator.sendBeacon === "function") {
        navigator.sendBeacon(url, new Blob([body], { type: "text/plain;charset=UTF-8" }));
        return;
      }
      void fetcher(url, {
        method: "POST",
        credentials: "same-origin",
        headers: { "content-type": "application/json" },
        body,
        keepalive: true,
      }).catch(() => undefined);
    });
    const collector = options.health;
    window.addEventListener("pagehide", () => collector.flush(true));
  }
  return adapter;
}

export const apiAdapter = createApiAdapter({ health });
