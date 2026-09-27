import { encodeTerminalFrame } from "../../frame";
import type { ClientRequest } from "../../protocol/ClientRequest";
import type { DaemonMessage } from "../../protocol/DaemonMessage";
import type { Envelope } from "../../protocol/Envelope";
import type { Machine as WireMachine } from "../../protocol/Machine";
import type { Session as WireSession } from "../../protocol/Session";
import { NotImplemented, type DataAdapter, type LiveEvent } from "../adapter";
import type { AuthState, Machine, Session } from "../types";

const PROTOCOL_VERSION = 1;
const SOCKET_OPEN = 1;
const POLL_INTERVAL_MS = 2_000;
const encoder = new TextEncoder();

export const DAEMON_TOKEN_KEY = "sentinal.daemonToken";

export function defaultDaemonUrl(): string {
  return (
    import.meta.env.VITE_DAEMON_URL ??
    `${window.location.protocol === "https:" ? "wss" : "ws"}://${window.location.hostname}:7420/api/v1/ws`
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

interface TokenStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

interface PendingReply {
  accepts: (message: DaemonMessage) => boolean;
  resolve: (message: DaemonMessage) => void;
  reject: (error: Error) => void;
}

export interface ApiAdapterOptions {
  daemonUrl?: string;
  storage?: TokenStorage;
  socketFactory?: (url: string) => ApiSocket;
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

function mapSession(session: WireSession): Session {
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
    const token = this.storage.getItem(DAEMON_TOKEN_KEY);
    if (!token) return Promise.reject(daemonError("missing daemon token"));

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
      socket.onclose = () => {
        if (this.socket === socket) this.socket = null;
        this.connecting = null;
        this.rejectPending(daemonError("connection closed"));
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
    if (!this.storage.getItem(DAEMON_TOKEN_KEY)) return;
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
  const transport = new DaemonTransport(options.daemonUrl ?? defaultDaemonUrl(), storage, socketFactory);

  const getMachines = async (): Promise<Machine[]> => {
    const message = await transport.request({ type: "GET_MACHINE" }, (reply) => reply.type === "MACHINE");
    return message.type === "MACHINE" ? [mapMachine(message.machine)] : [];
  };

  const getSessions = async (): Promise<Session[]> => {
    const message = await transport.request({ type: "LIST_SESSIONS" }, (reply) => reply.type === "SESSIONS");
    return message.type === "SESSIONS" ? message.sessions.map(mapSession) : [];
  };

  const login = async (token: string): Promise<AuthState> => {
    storage.setItem(DAEMON_TOKEN_KEY, token);
    transport.reset();
    try {
      await getMachines();
      return { status: "signedIn", attempts: 0, lockUntil: null };
    } catch {
      storage.removeItem(DAEMON_TOKEN_KEY);
      transport.reset();
      return { status: "wrong", attempts: 1, lockUntil: null };
    }
  };

  return {
    getMachines,
    getSessions,
    async createSession() {
      const message = await transport.request(
        { type: "CREATE_SESSION", shell: null, cwd: null, cols: 80, rows: 24 },
        (reply) => reply.type === "SESSION_UPDATED",
      );
      if (message.type !== "SESSION_UPDATED") throw daemonError("missing created session");
      return mapSession(message.session);
    },
    getRecipes: () => unsupported("getRecipes"),
    getHistory: () => unsupported("getHistory"),
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
    auth: {
      async getState(): Promise<AuthState> {
        if (!storage.getItem(DAEMON_TOKEN_KEY)) {
          return { status: "signedOut", attempts: 0, lockUntil: null };
        }
        try {
          await getMachines();
          return { status: "signedIn", attempts: 0, lockUntil: null };
        } catch {
          storage.removeItem(DAEMON_TOKEN_KEY);
          transport.reset();
          return { status: "signedOut", attempts: 0, lockUntil: null };
        }
      },
      login,
      setup: login,
      changePassword: () => unsupported("auth.changePassword"),
      async logout() {
        storage.removeItem(DAEMON_TOKEN_KEY);
        transport.reset();
      },
    },
    notifications: {
      getRules: () => unsupported("notifications.getRules"),
      setRule: () => unsupported("notifications.setRule"),
      getJournal: () => unsupported("notifications.getJournal"),
    },
  };
}

export const apiAdapter = createApiAdapter();
