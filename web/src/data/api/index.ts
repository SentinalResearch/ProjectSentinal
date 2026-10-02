import { encodeTerminalFrame } from "../../frame";
import type { ClientRequest } from "../../protocol/ClientRequest";
import type { DaemonMessage } from "../../protocol/DaemonMessage";
import type { Envelope } from "../../protocol/Envelope";
import type { Machine as WireMachine } from "../../protocol/Machine";
import type { Session as WireSession } from "../../protocol/Session";
import type { Command } from "../../protocol/Command";
import type { Event as WireEvent } from "../../protocol/Event";
import type { EventKind } from "../../protocol/EventKind";
import type { Health } from "../../protocol/Health";
import { NotImplemented, type DataAdapter, type LiveEvent } from "../adapter";
import type { HistoryEntry, Machine, Session, SessionEvent, SessionHealth } from "../types";
import {
  AUTH_EXPIRED_EVENT,
  createAuthApi,
  defaultDaemonHttpUrl,
  SESSION_TOKEN_KEY,
  type TokenStorage,
} from "./auth";

export { AUTH_EXPIRED_EVENT, defaultDaemonHttpUrl, SESSION_TOKEN_KEY } from "./auth";
import { createRegistryApi } from "./registry";

const PROTOCOL_VERSION = 1;
const SOCKET_OPEN = 1;
const POLL_INTERVAL_MS = 2_000;
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
}

export interface ApiAdapterOptions {
  daemonUrl?: string;
  daemonHttpUrl?: string;
  storage?: TokenStorage;
  socketFactory?: (url: string) => ApiSocket;
  fetcher?: typeof fetch;
}

/**
 * Replies to CREATE_SESSION / KILL are SESSION_UPDATED for a FULL Session;
 * the daemon also pushes unsolicited SESSION_UPDATED observations for
 * reduced-capture Sessions, which must never be taken as a reply.
 */
function isFullSessionUpdate(reply: DaemonMessage, id?: number): boolean {
  return (
    reply.type === "SESSION_UPDATED" &&
    reply.session.capture === "FULL" &&
    (id === undefined || reply.session.id === id)
  );
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

function mapMachine(machine: WireMachine): Machine {
  const ramGb = machine.ram_bytes / 1024 ** 3;
  const hardware = [
    machine.cpu === "unknown" ? `${machine.cpu_cores} CPU cores` : machine.cpu,
    machine.ram_bytes > 0 ? `${ramGb.toFixed(ramGb >= 10 ? 0 : 1)} GB RAM` : null,
    ...machine.gpus.map((gpu) => gpu.name),
  ]
    .filter(Boolean)
    .join(" · ");
  return {
    id: machine.id,
    name: machine.name,
    os: machine.os === "MACOS" ? "macOS" : "Linux",
    hardware,
    status: machine.status,
  };
}

export function mapSession(session: WireSession): Session {
  return {
    id: String(session.id),
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
    origin: session.origin,
    capture: session.capture,
    exitCode: session.exit_code,
    log: session.log ?? undefined,
    continuedFrom: session.continued_from === null || session.continued_from === undefined
      ? undefined
      : String(session.continued_from),
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

function applyWatcherEvents(session: Session, events: WireEvent[]): Session {
  let result = { ...session, events: events.map(mapWatcherEvent) };
  for (const event of events) {
    result = { ...result, ...watcherEventPatch(event) };
  }
  return result;
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
    id: String(command.id),
    ok: command.exit_code === null ? null : command.exit_code === 0,
    command: command.text,
    duration: command.exit_code === null ? "running" : formatDuration(elapsed),
    machine: command.machine_id,
    cwd: command.cwd,
    ...relativeDate(command.started_at),
    sessionId: String(command.session_id),
  };
}

class DaemonTransport {
  private socket: ApiSocket | null = null;
  private connecting: Promise<ApiSocket> | null = null;
  private readonly pending: PendingReply[] = [];
  private readonly listeners = new Set<(event: LiveEvent) => void>();
  private pollTimer: ReturnType<typeof setInterval> | null = null;
  private latestGpuPct: number | undefined;
  private latestVramTotal = 0;

  constructor(
    private readonly url: string,
    private readonly storage: TokenStorage,
    private readonly socketFactory: (url: string) => ApiSocket,
  ) {}

  async request(
    request: ClientRequest,
    accepts: (message: DaemonMessage) => boolean,
  ): Promise<DaemonMessage> {
    const socket = await this.connect();
    return new Promise((resolve, reject) => {
      this.pending.push({ accepts, resolve, reject });
      const envelope: Envelope<ClientRequest> = { version: PROTOCOL_VERSION, payload: request };
      socket.send(JSON.stringify(envelope));
    });
  }

  async sendBytes(sessionId: string, bytes: Uint8Array): Promise<void> {
    const socket = await this.connect();
    socket.send(encodeTerminalFrame(Number(sessionId), bytes));
  }

  subscribe(listener: (event: LiveEvent) => void): () => void {
    this.listeners.add(listener);
    if (this.pollTimer === null) {
      this.pollTimer = setInterval(() => void this.pollSessions(), POLL_INTERVAL_MS);
    }
    return () => {
      this.listeners.delete(listener);
      if (this.listeners.size === 0 && this.pollTimer !== null) {
        clearInterval(this.pollTimer);
        this.pollTimer = null;
      }
    };
  }

  reset() {
    const socket = this.socket;
    this.socket = null;
    this.connecting = null;
    socket?.close();
    this.rejectPending(daemonError("connection reset"));
  }

  private connect(): Promise<ApiSocket> {
    if (this.socket?.readyState === SOCKET_OPEN) return Promise.resolve(this.socket);
    if (this.connecting !== null) return this.connecting;
    const token = this.storage.getItem(SESSION_TOKEN_KEY);
    if (!token) return Promise.reject(daemonError("missing browser session"));

    const endpoint = new URL(this.url);
    endpoint.searchParams.set("token", token);
    this.connecting = new Promise((resolve, reject) => {
      const socket = this.socketFactory(endpoint.toString());
      socket.binaryType = "arraybuffer";
      let opened = false;
      socket.onopen = () => {
        opened = true;
        this.socket = socket;
        this.connecting = null;
        resolve(socket);
      };
      socket.onerror = () => {
        if (!opened) {
          this.connecting = null;
          reject(daemonError("authentication or connection error"));
        }
      };
      socket.onclose = (event) => {
        if (this.socket === socket) this.socket = null;
        this.connecting = null;
        this.rejectPending(daemonError("connection closed"));
        if (event.code === 4_001) {
          this.storage.removeItem(SESSION_TOKEN_KEY);
          window.dispatchEvent(new Event(AUTH_EXPIRED_EVENT));
        }
      };
      socket.onmessage = (event) => this.handleMessage(event);
    });
    return this.connecting;
  }

  private handleMessage(event: MessageEvent<string | ArrayBuffer | Blob>) {
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
      pending?.reject(daemonError(message.message));
      return;
    }

    if (message.type === "SESSION_UPDATED") {
      const session = mapSession(message.session);
      for (const listener of this.listeners) listener({ type: "SESSION_UPDATED", session });
    } else if (message.type === "MACHINE_METRICS") {
      this.latestGpuPct = message.metrics.gpus.reduce<number | undefined>(
        (highest, gpu) => highest === undefined ? gpu.util_percent : Math.max(highest, gpu.util_percent),
        undefined,
      );
      this.latestVramTotal = message.metrics.gpus.reduce((total, gpu) => total + gpu.vram_total_bytes, 0);
    } else if (message.type === "SESSION_METRICS") {
      const totals = message.metrics.totals;
      const metrics = {
        cpuPct: Math.round(totals.cpu_percent * 10) / 10,
        ramGB: Math.round((totals.rss_bytes / 1024 ** 3) * 10) / 10,
        gpuPct: totals.gpu_mem_bytes > 0 ? this.latestGpuPct : undefined,
        vramGB: this.latestVramTotal > 0 || totals.gpu_mem_bytes > 0
          ? Math.round((totals.gpu_mem_bytes / 1024 ** 3) * 10) / 10
          : undefined,
        vramTotalGB: this.latestVramTotal > 0
          ? Math.round((this.latestVramTotal / 1024 ** 3) * 10) / 10
          : undefined,
      };
      for (const listener of this.listeners) {
        listener({ type: "SESSION_METRICS", sessionId: String(message.metrics.session_id), metrics });
      }
    } else if (message.type === "COMMAND_UPDATED") {
      for (const listener of this.listeners) {
        listener({
          type: "COMMAND_UPDATED",
          sessionId: String(message.command.session_id),
          command: message.command.finished_at === null ? message.command.text : undefined,
        });
      }
    } else if (message.type === "EVENT") {
      const observed = message.event;
      for (const listener of this.listeners) {
        listener({
          type: "EVENT_APPENDED",
          sessionId: String(observed.session_id),
          event: mapWatcherEvent(observed),
          sessionPatch: watcherEventPatch(observed),
        });
      }
    }

    const index = this.pending.findIndex((pending) => pending.accepts(message));
    if (index >= 0) {
      const [pending] = this.pending.splice(index, 1);
      pending.resolve(message);
    }
  }

  private rejectPending(error: Error) {
    for (const pending of this.pending.splice(0)) pending.reject(error);
  }

  private async pollSessions() {
    if (!this.storage.getItem(SESSION_TOKEN_KEY)) return;
    try {
      const message = await this.request({ type: "LIST_SESSIONS" }, (reply) => reply.type === "SESSIONS");
      if (message.type !== "SESSIONS") return;
      for (const session of message.sessions.map(mapSession)) {
        for (const listener of this.listeners) listener({ type: "SESSION_UPDATED", session });
      }
    } catch {
      // Authentication state owns connection errors; polling retries on its next tick.
    }
  }
}

function unsupported(method: string): Promise<never> {
  return Promise.reject(new NotImplemented(method));
}

export function createApiAdapter(options: ApiAdapterOptions = {}): DataAdapter {
  const storage = options.storage ?? window.localStorage;
  const socketFactory = options.socketFactory ?? ((url: string) => new WebSocket(url));
  const fetcher = options.fetcher ?? window.fetch.bind(window);
  const authBaseUrl = options.daemonHttpUrl ?? defaultDaemonHttpUrl();
  const transport = new DaemonTransport(options.daemonUrl ?? defaultDaemonUrl(), storage, socketFactory);

  const getMachines = async (): Promise<Machine[]> => {
    const message = await transport.request({ type: "GET_MACHINE" }, (reply) => reply.type === "MACHINE");
    return message.type === "MACHINE" ? [mapMachine(message.machine)] : [];
  };

  const getSessions = async (): Promise<Session[]> => {
    const message = await transport.request({ type: "LIST_SESSIONS" }, (reply) => reply.type === "SESSIONS");
    if (message.type !== "SESSIONS") return [];
    const sessions: Session[] = [];
    for (const wire of message.sessions) {
      const reply = await transport.request(
        { type: "GET_EVENTS", session_id: wire.id, after: null, limit: 500 },
        (candidate) => candidate.type === "EVENTS",
      );
      sessions.push(applyWatcherEvents(mapSession(wire), reply.type === "EVENTS" ? reply.events : []));
    }
    return sessions;
  };

  const auth = createAuthApi({ baseUrl: authBaseUrl, fetcher, storage, resetTransport: () => transport.reset() });

  return {
    getMachines,
    getSessions,
    async createSession(options = {}) {
      const message = await transport.request(
        {
          type: "CREATE_SESSION",
          shell: options.shell ?? null,
          cwd: options.cwd ?? null,
          cols: options.cols ?? 80,
          rows: options.rows ?? 24,
          origin: "UI",
          title: options.title ?? null,
          command: null,
        },
        (reply) => isFullSessionUpdate(reply),
      );
      if (message.type !== "SESSION_UPDATED") throw daemonError("missing created session");
      return mapSession(message.session);
    },
    async continueSession(sessionId) {
      const sourceId = Number(sessionId);
      const message = await transport.request(
        { type: "CONTINUE_SESSION", session_id: sourceId },
        (reply) =>
          reply.type === "SESSION_UPDATED" &&
          reply.session.capture === "FULL" &&
          reply.session.continued_from === sourceId,
      );
      if (message.type !== "SESSION_UPDATED") throw daemonError("missing continued Session");
      return mapSession(message.session);
    },
    async replayOutput(sessionId, onChunk, options = {}) {
      const token = storage.getItem(SESSION_TOKEN_KEY);
      if (!token) throw daemonError("missing browser session");
      const maximum = Math.min(64 * 1024 * 1024, Math.max(1, Math.floor(options.maxBytes ?? 64 * 1024 * 1024)));
      let offset = 0;
      let bytesRead = 0;
      let warning: string | undefined;
      for (let page = 0; page < 256; page += 1) {
        const limit = Math.min(262144, maximum - bytesRead);
        if (limit <= 0) return warning ? { warning, bytesRead, truncated: true } : { bytesRead, truncated: true };
        const endpoint = `${authBaseUrl}/sessions/${encodeURIComponent(sessionId)}/output?offset=${offset}&limit=${limit}`;
        const response = await fetcher(endpoint, {
          headers: { authorization: `Bearer ${token}` },
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
        const complete = response.headers.get("x-sentinal-output-complete") === "true";
        if (complete) return warning ? { warning, bytesRead } : { bytesRead };
        if (bytesRead >= maximum) {
          return warning ? { warning, bytesRead, truncated: true } : { bytesRead, truncated: true };
        }
        const next = Number(response.headers.get("x-sentinal-output-next-offset"));
        if (!Number.isSafeInteger(next) || next <= offset) {
          throw daemonError("saved output returned an invalid continuation offset");
        }
        offset = next;
      }
      throw daemonError("saved output exceeded the 64 MiB replay limit");
    },
    getRecipes: () => unsupported("getRecipes"),
    async getHistory(filters = {}) {
      const request: ClientRequest = filters.query?.trim()
        ? { type: "SEARCH_COMMANDS", query: filters.query.trim(), limit: 500 }
        : {
            type: "GET_HISTORY",
            session_id: filters.sessionId === undefined ? null : Number(filters.sessionId),
            limit: 500,
          };
      const message = await transport.request(request, (reply) => reply.type === "HISTORY");
      if (message.type !== "HISTORY") return [];
      const sessionsById = new Map((await getSessions()).map((session) => [session.id, session]));
      return message.commands
        .map((command) => {
          const entry = mapCommand(command);
          return { ...entry, project: sessionsById.get(entry.sessionId ?? "")?.project };
        })
        .filter((entry) => !filters.machine || entry.machine === filters.machine)
        .filter((entry) => !filters.project || entry.project === filters.project)
        .filter((entry) => {
          if (!filters.result) return true;
          if (filters.result === "running") return entry.ok === null;
          return filters.result === "ok" ? entry.ok === true : entry.ok === false;
        });
    },
    subscribe: (listener) => transport.subscribe(listener),
    sendInput: (sessionId, data) => transport.sendBytes(sessionId, encoder.encode(data)),
    async sendSignal(sessionId, signal) {
      if (signal === "SIGINT") {
        await transport.sendBytes(sessionId, new Uint8Array([3]));
        return;
      }
      await transport.request(
        { type: "KILL", session_id: Number(sessionId) },
        (reply) => isFullSessionUpdate(reply, Number(sessionId)),
      );
    },
    runRecipe: () => unsupported("runRecipe"),
    registry: createRegistryApi({ baseUrl: authBaseUrl, fetcher, storage }),
    auth,
    notifications: {
      getRules: () => unsupported("notifications.getRules"),
      setRule: () => unsupported("notifications.setRule"),
      getJournal: () => unsupported("notifications.getJournal"),
    },
  };
}

export const apiAdapter = createApiAdapter();
