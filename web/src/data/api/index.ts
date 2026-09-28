import { encodeTerminalFrame } from "../../frame";
import type { ClientRequest } from "../../protocol/ClientRequest";
import type { DaemonMessage } from "../../protocol/DaemonMessage";
import type { Envelope } from "../../protocol/Envelope";
import type { Machine as WireMachine } from "../../protocol/Machine";
import type { Session as WireSession } from "../../protocol/Session";
import type { Command } from "../../protocol/Command";
import { NotImplemented, type DataAdapter, type LiveEvent } from "../adapter";
import type { HistoryEntry, Machine, Session } from "../types";
import {
  AUTH_EXPIRED_EVENT,
  createAuthApi,
  defaultDaemonHttpUrl,
  SESSION_TOKEN_KEY,
  type TokenStorage,
} from "./auth";

export { AUTH_EXPIRED_EVENT, defaultDaemonHttpUrl, SESSION_TOKEN_KEY } from "./auth";

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
    events: [],
  };
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
    return message.type === "SESSIONS" ? message.sessions.map(mapSession) : [];
  };

  const auth = createAuthApi({ baseUrl: authBaseUrl, fetcher, storage, resetTransport: () => transport.reset() });

  return {
    getMachines,
    getSessions,
    async createSession() {
      const message = await transport.request(
        {
          type: "CREATE_SESSION",
          shell: null,
          cwd: null,
          cols: 80,
          rows: 24,
          origin: "UI",
          title: null,
          command: null,
        },
        (reply) => reply.type === "SESSION_UPDATED",
      );
      if (message.type !== "SESSION_UPDATED") throw daemonError("missing created session");
      return mapSession(message.session);
    },
    getRecipes: () => unsupported("getRecipes"),
    async getHistory(filters = {}) {
      const request: ClientRequest = filters.query?.trim()
        ? { type: "SEARCH_COMMANDS", query: filters.query.trim(), limit: 500 }
        : { type: "GET_HISTORY", session_id: null, limit: 500 };
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
        (reply) => reply.type === "SESSION_UPDATED",
      );
    },
    runRecipe: () => unsupported("runRecipe"),
    pairMachine: () => unsupported("pairMachine"),
    rotateToken: () => unsupported("rotateToken"),
    revokeToken: () => unsupported("revokeToken"),
    getMachineTokens: () => unsupported("getMachineTokens"),
    auth,
    notifications: {
      getRules: () => unsupported("notifications.getRules"),
      setRule: () => unsupported("notifications.setRule"),
      getJournal: () => unsupported("notifications.getJournal"),
    },
  };
}

export const apiAdapter = createApiAdapter();
