import { describe, expect, it, vi } from "vitest";

import { decodeTerminalFrame } from "../../frame";
import type { ClientRequest } from "../../protocol/ClientRequest";
import type { DaemonMessage } from "../../protocol/DaemonMessage";
import type { Envelope } from "../../protocol/Envelope";
import type { Machine as WireMachine } from "../../protocol/Machine";
import type { Session as WireSession } from "../../protocol/Session";
import type { Command } from "../../protocol/Command";
import type { RegistryMachine } from "../../protocol/RegistryMachine";
import type { ControlProjectionMessage } from "../../protocol/ControlProjectionMessage";
import type { ControlProjectionSnapshot } from "../../protocol/ControlProjectionSnapshot";
import { createApiAdapter, mapSession, type ApiSocket } from ".";

const machine: WireMachine = {
  id: "test-machine",
  name: "Test Machine",
  os: "MACOS",
  cpu: "Apple M4",
  cpu_cores: 10,
  ram_bytes: 17_179_869_184,
  gpus: [],
  status: "ONLINE",
  last_seen: 1_700_000_000_000,
};

const session: WireSession = {
  id: 1,
  machine_id: "test-machine",
  project_id: null,
  title: null,
  shell: "/bin/zsh",
  cwd: "/tmp",
  state: "IDLE",
  current_command: null,
  origin: "UI",
  capture: "FULL",
  pid: 4242,
  process_started_at: null,
  exit_code: null,
  log: null,
  created_at: 1_700_000_000_000,
  updated_at: 1_700_000_000_000,
  health: {
    progress: 70,
    activity: "GOOD",
    resources: "GOOD",
    stability: "GOOD",
    errors: 0,
    overall: "HEALTHY",
    evidence: ["progress reached 70.0%"],
  },
};

const watcherEvent: import("../../protocol/Event").Event = {
  id: 12,
  session_id: 1,
  command_id: 9,
  at: 1_700_000_001_000,
  kind: { type: "PROGRESS_UPDATED", current: 7, total: 10, unit: "epoch" },
};

const command: Command = {
  id: 9,
  session_id: 1,
  machine_id: "test-machine",
  text: "python train.py",
  cwd: "/work/model",
  started_at: 1_700_000_000_000,
  finished_at: 1_700_000_031_000,
  exit_code: 0,
  class: "ML_TRAINING",
};

class MemoryStorage {
  private readonly values = new Map<string, string>();

  getItem(key: string) {
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string) {
    this.values.set(key, value);
  }

  removeItem(key: string) {
    this.values.delete(key);
  }
}

class FakeAuthServer {
  password: string | null;
  browserAuthenticated = false;
  readonly outputRequests: URL[] = [];
  readonly unavailableMachines = new Set<string>();
  /** Machines whose Session list also carries a row stamped with another Machine ID. */
  readonly foreignSessionMachines = new Set<string>();
  readonly revokedMachines: string[] = [];
  readonly paths: string[] = [];
  registryMachines: RegistryMachine[] = [];
  projectionSnapshot: ControlProjectionSnapshot | null = null;

  constructor(password: string | null) {
    this.password = password;
  }

  fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const url = new URL(String(input), "http://daemon.test");
    this.paths.push(url.pathname);
    const request = init ?? {};
    if (url.pathname === "/api/v1/auth/status") {
      return json({ setup_required: this.password === null });
    }
    if (url.pathname === "/api/v1/auth/browser/setup") {
      if (new Headers(request.headers).get("authorization") !== "Bearer bootstrap-token") {
        return failure("INVALID_BOOTSTRAP", "invalid bootstrap machine token", 401);
      }
      this.password = body(request).password as string;
      this.browserAuthenticated = true;
      return json(this.issue());
    }
    if (url.pathname === "/api/v1/auth/browser/login") {
      if (body(request).password !== this.password) {
        return failure("INVALID_CREDENTIALS", "invalid owner password", 401, 4);
      }
      this.browserAuthenticated = true;
      return json(this.issue());
    }
    if (!this.browserAuthenticated) {
      return failure("INVALID_SESSION", "invalid or expired browser session", 401);
    }
    if (url.pathname === "/api/v1/control/snapshot") {
      return this.projectionSnapshot === null ? new Response(null, { status: 404 }) : json(this.projectionSnapshot);
    }
    if (url.pathname === "/api/v1/auth/browser/session") {
      return json({ expires_at: 1_790_086_400_000 });
    }
    if (url.pathname === "/api/v1/auth/browser/logout") {
      this.browserAuthenticated = false;
      return new Response(null, { status: 204 });
    }
    if (url.pathname === "/api/v1/auth/browser/password") {
      const values = body(request);
      if (values.current_password !== this.password) {
        return failure("INVALID_CREDENTIALS", "invalid owner password", 401, 0);
      }
      this.password = values.new_password as string;
      this.browserAuthenticated = true;
      return json(this.issue());
    }
    if (url.pathname === "/api/v1/machines") {
      return json({ machines: this.registryMachines });
    }
    const ping = url.pathname.match(/^\/api\/v1\/machines\/([^/]+)\/ping$/);
    if (ping) {
      return json({
        machine_id: decodeURIComponent(ping[1]),
        round_trips_us: [12_500, 4_000],
        failed: 1,
        error: "the selected Machine is unreachable",
        path: "RELAYED",
        relay: "fra",
      });
    }
    const registryMachine = url.pathname.match(/^\/api\/v1\/machines\/([^/]+)$/);
    if (registryMachine && request.method === "DELETE") {
      const id = decodeURIComponent(registryMachine[1]);
      this.revokedMachines.push(id);
      this.registryMachines = this.registryMachines.filter((entry) => entry.machine_id !== id);
      return new Response(null, { status: 204 });
    }
    if (url.pathname === "/api/v1/sessions/1/output" || /\/api\/v1\/machines\/[^/]+\/sessions\/1\/output/.test(url.pathname)) {
      this.outputRequests.push(url);
      const offset = Number(url.searchParams.get("offset") ?? "0");
      if (offset === 0) {
        return new Response(new TextEncoder().encode("saved "), {
          headers: {
            "content-type": "application/octet-stream",
            "x-sentinal-output-next-offset": "6",
            "x-sentinal-output-complete": "false",
          },
        });
      }
      return new Response(new TextEncoder().encode("output"), {
        headers: {
          "content-type": "application/octet-stream",
          "x-sentinal-output-next-offset": "12",
          "x-sentinal-output-complete": "true",
          "x-sentinal-output-warning": "TRUNCATED_ZSTD",
        },
      });
    }
    return new Response(null, { status: 404 });
  };

  private issue() {
    return { expires_at: 1_790_086_400_000 };
  }
}

class FakeSocket implements ApiSocket {
  readonly url: string;
  readyState = 0;
  binaryType = "blob";
  onopen: ((event: Event) => unknown) | null = null;
  onclose: ((event: CloseEvent) => unknown) | null = null;
  onerror: ((event: Event) => unknown) | null = null;
  onmessage: ((event: MessageEvent<string | ArrayBuffer | Blob>) => void) | null = null;
  readonly binaryFrames: Uint8Array[] = [];
  readonly requests: ClientRequest[] = [];
  /** Push an unsolicited observation of an adopted process before each reply. */
  observeFirst = false;
  readonly machineId: string;

  constructor(url: string, private readonly server: FakeAuthServer) {
    this.url = url;
    const routed = new URL(url).pathname.match(/^\/api\/v1\/machines\/([^/]+)\/ws$/);
    this.machineId = routed ? decodeURIComponent(routed[1]) : machine.id;
    queueMicrotask(() => {
      if (!server.browserAuthenticated || server.unavailableMachines.has(this.machineId)) {
        this.onerror?.(new Event("error"));
        return;
      }
      this.readyState = WebSocket.OPEN;
      this.onopen?.(new Event("open"));
    });
  }

  send(data: string | ArrayBufferLike | Blob | ArrayBufferView) {
    if (this.server.unavailableMachines.has(this.machineId)) {
      this.readyState = WebSocket.CLOSED;
      this.onclose?.(new CloseEvent("close"));
      return;
    }
    if (typeof data !== "string") {
      const bytes = data instanceof Uint8Array ? data : new Uint8Array(data as ArrayBuffer);
      this.binaryFrames.push(bytes);
      return;
    }
    const request = (JSON.parse(data) as Envelope<ClientRequest>).payload;
    this.requests.push(request);
    const ownedMachine: WireMachine = {
      ...machine,
      id: this.machineId,
      name: this.machineId === machine.id ? machine.name : "Worker Machine",
    };
    const ownedSession: WireSession = { ...session, machine_id: this.machineId };
    const ownedCommand: Command = { ...command, machine_id: this.machineId };
    let payload: DaemonMessage;
    if (request.type === "GET_MACHINE") payload = { type: "MACHINE", machine: ownedMachine };
    else if (request.type === "LIST_SESSIONS") {
      const foreign = { ...ownedSession, id: 2, machine_id: "terminal-agent-machine" };
      payload = {
        type: "SESSIONS",
        sessions: this.server.foreignSessionMachines.has(this.machineId) ? [ownedSession, foreign] : [ownedSession],
      };
    }
    else if (request.type === "CREATE_SESSION") payload = { type: "SESSION_UPDATED", session: ownedSession };
    else if (request.type === "CONTINUE_SESSION") {
      payload = {
        type: "SESSION_UPDATED",
        session: { ...ownedSession, id: 2, state: "IDLE", continued_from: request.session_id },
      };
    }
    else if (request.type === "KILL") {
      payload = { type: "SESSION_UPDATED", session: { ...ownedSession, state: "INTERRUPTED" } };
    } else if (request.type === "GET_HISTORY" || request.type === "SEARCH_COMMANDS") {
      payload = { type: "HISTORY", commands: [ownedCommand] };
    } else if (request.type === "GET_EVENTS") {
      payload = { type: "EVENTS", events: [watcherEvent] };
    } else if (request.type === "LIST_RECIPES" || request.type === "SAVE_RECIPE") {
      payload = {
        type: "RECIPES",
        recipes: [{
          id: "cargo-test",
          name: "cargo test",
          category: "TESTS",
          project: null,
          machine_id: null,
          command: "cargo test {arguments}",
          params: [{ name: "arguments", default: "--workspace", description: null }],
        }],
      };
    } else if (request.type === "GET_MACHINE_DETAILS") {
      payload = {
        type: "MACHINE_DETAILS",
        details: {
          hostname: `${this.machineId}-host`,
          os_version: "macOS 26.6.2",
          kernel_version: "25.6.0",
          daemon_version: "0.1.9",
          started_at: 1_700_000_000_000,
          bind: "100.64.0.2:7420",
          link: null,
          processes: [{
            session_id: 1,
            processes: [{ pid: 4242, parent_pid: 1, name: "python", cpu_percent: 97.5, rss_bytes: 1_073_741_824, gpu_mem_bytes: null }],
          }],
        },
      };
    } else if (request.type === "GET_MACHINE_METRICS") {
      payload = {
        type: "MACHINE_METRICS_HISTORY",
        range: request.range,
        points: [
          { at: 1_700_000_000_000, cpu_percent: 42.5, ram_used_bytes: 4_294_967_296, ram_total_bytes: 17_179_869_184, gpu_percent: null, vram_used_bytes: null, vram_total_bytes: null },
          { at: 1_700_000_300_000, cpu_percent: 10, ram_used_bytes: 8_589_934_592, ram_total_bytes: 17_179_869_184, gpu_percent: 80, vram_used_bytes: 5, vram_total_bytes: 10 },
        ],
      };
    } else if (request.type === "RUN_RECIPE") {
      payload = { type: "SESSION_UPDATED", session: { ...ownedSession, id: 3, title: "cargo test" } };
    } else if (request.type === "GET_NOTIFICATION_RULES" || request.type === "SET_NOTIFICATION_RULE") {
      payload = {
        type: "NOTIFICATION_RULES",
        rules: {
          default: { finished: true, error: true, stalled: true, waiting: true, checkpoint: false, progress: false },
          sessions: [],
        },
        status: {
          configured: true,
          server: "https://ntfy.sh",
          topic: "safe-topic",
          token_configured: true,
          configuration_state: "CONFIGURED",
          diagnostic: null,
          reloaded_at: 1_700_000_000_000,
        },
      };
    } else if (request.type === "GET_NOTIFICATION_JOURNAL") {
      payload = { type: "NOTIFICATION_JOURNAL", entries: [] };
    } else {
      payload = { type: "ERROR", message: `unsupported test request ${request.type}` };
    }
    if (this.observeFirst) {
      const observed: DaemonMessage = {
        type: "SESSION_UPDATED",
        session: { ...ownedSession, id: 77, origin: "ADOPTED", capture: "PROCESS", state: "RUNNING" },
      };
      this.onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({ version: 1, payload: observed } satisfies Envelope<DaemonMessage>),
        }),
      );
    }
    this.onmessage?.(
      new MessageEvent("message", {
        data: JSON.stringify({ version: 1, payload } satisfies Envelope<DaemonMessage>),
      }),
    );
  }

  close() {
    this.readyState = WebSocket.CLOSED;
    this.onclose?.(new CloseEvent("close"));
  }

  expire() {
    this.readyState = WebSocket.CLOSED;
    this.onclose?.(new CloseEvent("close", { code: 4_001, reason: "browser session expired" }));
  }

  observe(payload: DaemonMessage) {
    this.onmessage?.(
      new MessageEvent("message", {
        data: JSON.stringify({ version: 1, payload } satisfies Envelope<DaemonMessage>),
      }),
    );
  }

  project(message: ControlProjectionMessage) {
    this.onmessage?.(new MessageEvent("message", { data: JSON.stringify(message) }));
  }
}

function body(request: RequestInit): Record<string, unknown> {
  return JSON.parse(String(request.body ?? "{}")) as Record<string, unknown>;
}

function json(value: unknown, status = 200): Response {
  return new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function failure(code: string, message: string, status: number, attemptsRemaining: number | null = null) {
  return json({ code, message, attempts_remaining: attemptsRemaining, retry_at: null }, status);
}

function setup(password: string | null = "correct horse battery staple") {
  const storage = new MemoryStorage();
  const server = new FakeAuthServer(password);
  const sockets: FakeSocket[] = [];
  const adapter = createApiAdapter({
    daemonUrl: "ws://daemon.test/api/v1/ws",
    daemonHttpUrl: "http://daemon.test/api/v1",
    storage,
    fetcher: server.fetch,
    socketFactory: (url) => {
      const socket = new FakeSocket(url, server);
      sockets.push(socket);
      return socket;
    },
  });
  return { adapter, server, sockets, storage };
}

function workerRegistry(machineId = "worker-machine"): RegistryMachine {
  return {
    machine_id: machineId,
    display_name: "Worker Machine",
    role: "WORKER",
    status: "ONLINE",
    last_seen: 1_700_000_000_000,
    endpoint: { address: "100.64.0.2", port: 6842, dns_name: "worker.tailnet.ts.net" },
    os: "LINUX",
    daemon_version: "0.1.0",
    capabilities: ["SESSIONS", "EVENTS", "METRICS"],
    credential: {
      link_id: "link-worker",
      created_at: 1_700_000_000_000,
      expires_at: 1_900_000_000_000,
      last_used_at: 1_700_000_000_000,
    },
  };
}

function controlProjection(): ControlProjectionSnapshot {
  return {
    schema_version: 1,
    control_machine_id: "test-machine",
    view_revision: 8,
    generated_at: 1_700_000_002_000,
    machines: ["test-machine", "worker-machine"].map((machineId) => ({
      machine_id: machineId,
      projection_available: true,
      display_name: machineId === "test-machine" ? "Control" : "Worker",
      os: machineId === "test-machine" ? "MACOS" : "LINUX",
      daemon_version: "0.4.1",
      capabilities: ["SESSIONS", "EVENTS", "METRICS"],
      registry_status: "ONLINE",
      freshness: "LIVE",
      last_seen: 1_700_000_001_000,
      observed_at: 1_700_000_001_000,
      synced_at: 1_700_000_001_000,
      source_cursor: 20,
      sessions: [{
        id: 1,
        continued_from: null,
        title: `${machineId} Session`,
        state: "RUNNING",
        origin: "WATCH",
        capture: "FULL",
        capabilities: {
          can_attach: true,
          can_send_input: true,
          can_resize: true,
          can_kill_daemon_pty: true,
          exposes_commands: true,
          exposes_terminal_output: true,
          exposes_log_output: false,
          exposes_metrics: true,
        },
        health: null,
        created_at: 1_700_000_000_000,
        updated_at: 1_700_000_001_000,
        finished_at: null,
        exit_code: null,
      }],
      events: [],
      metrics: [],
      omitted_sessions: 0,
    })),
    omitted_machines: 0,
    omitted_sessions: 0,
    truncated: false,
  };
}

describe("apiAdapter authentication", () => {
  it("bootstraps the single owner without exposing a browser credential to storage", async () => {
    const { adapter, storage } = setup(null);

    await expect(adapter.auth.getState()).resolves.toMatchObject({ status: "setup" });
    await expect(
      adapter.auth.setup("bootstrap-token", "correct horse battery staple"),
    ).resolves.toMatchObject({ status: "signedIn" });

    expect(storage.getItem("sentinal.sessionToken")).toBeNull();
    expect(storage.getItem("sentinal.daemonToken")).toBeNull();
  });

  it("keeps first-run setup active when the machine token is wrong", async () => {
    const { adapter, storage } = setup(null);

    await expect(
      adapter.auth.setup("wrong-bootstrap", "correct horse battery staple"),
    ).resolves.toMatchObject({ status: "setup", message: "invalid bootstrap machine token" });
    expect(storage.getItem("sentinal.sessionToken")).toBeNull();
  });

  it("logs in with a password and maps Milestone 1 state", async () => {
    const { adapter, sockets } = setup();

    await expect(adapter.auth.login("correct horse battery staple")).resolves.toMatchObject({
      status: "signedIn",
    });
    await expect(adapter.getMachines()).resolves.toEqual([
      expect.objectContaining({ id: "test-machine", name: "Test Machine", os: "macOS", status: "ONLINE" }),
    ]);
    await expect(adapter.getSessions()).resolves.toEqual([
      expect.objectContaining({ id: "test-machine:1", machine: "test-machine", name: "Session 1", state: "IDLE" }),
    ]);
    expect(sockets).toHaveLength(1);
  });

  it("coalesces concurrent legacy initialization and never performs per-Session Event reads", async () => {
    const { adapter, server, sockets } = setup();
    await adapter.auth.login("correct horse battery staple");

    await Promise.all([adapter.getMachines(), adapter.getSessions()]);

    expect(server.paths.filter((path) => path === "/api/v1/machines")).toHaveLength(1);
    expect(sockets).toHaveLength(1);
    expect(sockets[0].requests.filter((request) => request.type === "GET_MACHINE")).toHaveLength(1);
    expect(sockets[0].requests.filter((request) => request.type === "LIST_SESSIONS")).toHaveLength(1);
    expect(sockets[0].requests.some((request) => request.type === "GET_EVENTS")).toBe(false);
  });

  it("does no two-second full refresh and uses a bounded sixty-second legacy integrity check", async () => {
    const { adapter, sockets } = setup();
    await adapter.auth.login("correct horse battery staple");
    await Promise.all([adapter.getMachines(), adapter.getSessions()]);
    vi.useFakeTimers();
    const unsubscribe = adapter.subscribe(() => undefined);
    await Promise.resolve();
    const initial = sockets[0].requests.length;

    await vi.advanceTimersByTimeAsync(5_000);
    expect(sockets[0].requests).toHaveLength(initial);

    await vi.advanceTimersByTimeAsync(55_000);
    expect(sockets[0].requests.length).toBeGreaterThan(initial);
    expect(sockets[0].requests.some((request) => request.type === "GET_EVENTS")).toBe(false);
    unsubscribe();
    vi.useRealTimers();
  });

  it("hydrates from one control snapshot and one live stream with no worker or Event query", async () => {
    const { adapter, server, sockets } = setup();
    server.projectionSnapshot = controlProjection();
    await adapter.auth.login("correct horse battery staple");

    const [machines, sessions] = await Promise.all([adapter.getMachines(), adapter.getSessions()]);

    expect(machines.map((entry) => entry.id)).toEqual(["test-machine", "worker-machine"]);
    expect(sessions.map((entry) => entry.id)).toEqual(["test-machine:1", "worker-machine:1"]);
    expect(server.paths.filter((path) => path === "/api/v1/control/snapshot")).toHaveLength(1);
    expect(sockets).toHaveLength(1);
    expect(new URL(sockets[0].url).pathname).toBe("/api/v1/control/live");
    expect(new URL(sockets[0].url).searchParams.get("after")).toBe("8");
    expect(sockets.flatMap((socket) => socket.requests)).toEqual([]);
  });

  it("uses one bounded direct refresh for an old worker without projection sync", async () => {
    const { adapter, server, sockets } = setup();
    const snapshot = controlProjection();
    snapshot.machines[1].projection_available = false;
    snapshot.machines[1].sessions = [];
    snapshot.machines[1].events = [];
    snapshot.machines[1].metrics = [];
    server.projectionSnapshot = snapshot;
    await adapter.auth.login("correct horse battery staple");

    const sessions = await adapter.getSessions();

    expect(sessions.map((entry) => entry.id)).toContain("worker-machine:1");
    const worker = sockets.find((socket) => socket.machineId === "worker-machine");
    expect(worker?.requests.map((request) => request.type)).toEqual(["GET_MACHINE", "LIST_SESSIONS"]);
    expect(worker?.requests.some((request) => request.type === "GET_EVENTS")).toBe(false);
  });

  it("applies the control live delta without opening a worker socket", async () => {
    const { adapter, server, sockets } = setup();
    server.projectionSnapshot = controlProjection();
    await adapter.auth.login("correct horse battery staple");
    await adapter.getSessions();
    const live: import("../adapter").LiveEvent[] = [];
    const unsubscribe = adapter.subscribe((event) => live.push(event));
    await Promise.resolve();

    sockets[0].project({
      type: "PROJECTION_DELTA",
      machine_id: "worker-machine",
      from_revision: 8,
      to_revision: 9,
      changes: [{
        type: "METRICS_REPLACE",
        cursor: 21,
        metrics: {
          scope: "SESSION",
          session_id: 1,
          at: 1_700_000_003_000,
          cpu_percent: 72,
          ram_used_bytes: 2_147_483_648,
          ram_total_bytes: null,
          gpu_percent: null,
          vram_used_bytes: null,
          vram_total_bytes: null,
        },
      }],
    });

    expect(live).toContainEqual(expect.objectContaining({
      type: "SESSION_METRICS",
      sessionId: "worker-machine:1",
      metrics: expect.objectContaining({ cpuPct: 72, ramGB: 2 }),
    }));
    expect(sockets).toHaveLength(1);
    unsubscribe();
  });

  it("fails live actions closed for an offline projected Machine", async () => {
    const { adapter, server, sockets } = setup();
    const snapshot = controlProjection();
    snapshot.machines[1].freshness = "OFFLINE";
    snapshot.machines[1].registry_status = "OFFLINE";
    server.projectionSnapshot = snapshot;
    await adapter.auth.login("correct horse battery staple");
    await adapter.getSessions();

    expect(() => adapter.resolveTerminal("worker-machine:1")).toThrow(/requires a live authoritative route/);
    await expect(adapter.sendInput("worker-machine:1", "unsafe")).rejects.toThrow(/requires a live authoritative route/);
    expect(sockets).toHaveLength(1);
  });

  it("keeps live actions routed to a delayed projected Machine", async () => {
    const { adapter, server } = setup();
    const snapshot = controlProjection();
    snapshot.machines[1].freshness = "STALE";
    server.projectionSnapshot = snapshot;
    await adapter.auth.login("correct horse battery staple");
    await adapter.getSessions();

    const worker = (await adapter.getMachines()).find((machine) => machine.id === "worker-machine");
    expect(worker).toMatchObject({ status: "ONLINE", freshness: "STALE" });
    expect(() => adapter.resolveTerminal("worker-machine:1")).not.toThrow();
  });

  it("maps the watcher's training summary onto the Session", () => {
    const mapped = mapSession({
      ...session,
      training: { loss: 0.362, first_loss: 1.84, best_val_loss: null, learning_rate: 0.0002, trend: "IMPROVING" },
    });
    expect(mapped.training).toEqual({
      loss: 0.362,
      firstLoss: 1.84,
      bestValLoss: undefined,
      learningRate: 0.0002,
      trend: "IMPROVING",
    });
    expect(mapped.metrics).toBeUndefined();
  });

  it("loads deterministic Health without replaying every Session Event", async () => {
    const { adapter, sockets } = setup();
    await adapter.auth.login("correct horse battery staple");

    await expect(adapter.getSessions()).resolves.toEqual([
      expect.objectContaining({
        health: expect.objectContaining({
          progress: "70%",
          activity: "GOOD",
          overall: "HEALTHY",
          evidence: ["progress reached 70.0%"],
        }),
        events: [],
      }),
    ]);
    expect(sockets[0].requests.some((request) => request.type === "GET_EVENTS")).toBe(false);
  });

  it("returns remaining attempts for an invalid password", async () => {
    const { adapter, storage } = setup();

    await expect(adapter.auth.login("wrong password")).resolves.toMatchObject({
      status: "wrong",
      attempts: 1,
    });
    expect(storage.getItem("sentinal.sessionToken")).toBeNull();
  });

  it("does not reuse a dashboard load that failed before login", async () => {
    const { adapter, server } = setup();
    server.projectionSnapshot = controlProjection();
    const unsubscribe = adapter.subscribe(() => undefined);
    await adapter.auth.login("correct horse battery staple");

    // The load started by subscribe() was refused (not signed in yet); the
    // first action after login starts a fresh one instead of failing with it.
    await expect(adapter.getMachines()).resolves.toHaveLength(2);
    unsubscribe();
  });

  it("creates sessions, emits updates, and frames remote input", async () => {
    const { adapter, sockets } = setup();
    const updates: string[] = [];
    const unsubscribe = adapter.subscribe((event) => {
      if (event.type === "SESSION_UPDATED") updates.push(event.session.id);
    });
    await adapter.auth.login("correct horse battery staple");

    const created = await adapter.createSession();
    await adapter.sendInput(created.id, "echo ready\n");

    expect(created.id).toBe("test-machine:1");
    expect(updates).toEqual(["test-machine:1"]);
    const frame = decodeTerminalFrame(sockets[0].binaryFrames[0]);
    expect(frame.sessionId).toBe(1);
    expect(new TextDecoder().decode(frame.payload)).toBe("echo ready\n");
    unsubscribe();
  });

  it("continues through the dedicated idempotent daemon request", async () => {
    const { adapter, sockets } = setup();
    await adapter.auth.login("correct horse battery staple");

    const continued = await adapter.continueSession("test-machine:1");

    expect(sockets[0].requests.at(-1)).toEqual({
      type: "CONTINUE_SESSION",
      session_id: 1,
    });
    expect(continued).toMatchObject({ id: "test-machine:2", continuedFrom: "test-machine:1" });
  });

  it("pages authenticated saved output as bytes and reports decoder warnings", async () => {
    const { adapter, server } = setup();
    await adapter.auth.login("correct horse battery staple");
    const chunks: Uint8Array[] = [];

    const replay = await adapter.replayOutput("test-machine:1", (chunk) => chunks.push(chunk));

    expect(chunks.map((chunk) => new TextDecoder().decode(chunk)).join("")).toBe("saved output");
    expect(replay.warning).toBe("TRUNCATED_ZSTD");
    expect(server.outputRequests).toHaveLength(2);
    expect(server.outputRequests[0].searchParams.get("offset")).toBe("0");
    expect(server.outputRequests[1].searchParams.get("offset")).toBe("6");
  });

  it("stops replay at the caller's total history budget", async () => {
    const { adapter, server } = setup();
    await adapter.auth.login("correct horse battery staple");
    const chunks: Uint8Array[] = [];

    const replay = await adapter.replayOutput("test-machine:1", (chunk) => chunks.push(chunk), {
      maxBytes: 6,
    });

    expect(new TextDecoder().decode(chunks[0])).toBe("saved ");
    expect(replay).toMatchObject({ bytesRead: 6, truncated: true });
    expect(server.outputRequests).toHaveLength(1);
    expect(server.outputRequests[0].searchParams.get("limit")).toBe("6");
  });

  it("never mistakes an unsolicited observation for a create or kill reply", async () => {
    const { adapter, sockets } = setup();
    await adapter.auth.login("correct horse battery staple");
    await adapter.getSessions();
    sockets[0].observeFirst = true;

    const created = await adapter.createSession();
    expect(created.id).toBe("test-machine:1");
    await adapter.sendSignal(created.id, "KILL");
    expect(sockets[0].requests.at(-1)).toEqual({ type: "KILL", session_id: 1 });
  });

  it("uses daemon history and FTS search and maps duration and cwd", async () => {
    const { adapter, sockets } = setup();
    await adapter.auth.login("correct horse battery staple");

    await expect(adapter.getHistory({ query: "train" })).resolves.toEqual([
      expect.objectContaining({
        command: "python train.py",
        cwd: "/work/model",
        duration: "31s",
        ok: true,
        sessionId: "test-machine:1",
      }),
    ]);
    expect(sockets[0].requests).toContainEqual({ type: "SEARCH_COMMANDS", query: "train", limit: 500 });

    await adapter.getHistory({ sessionId: "test-machine:1" });
    expect(sockets[0].requests).toContainEqual({ type: "GET_HISTORY", session_id: 1, limit: 500 });
  });

  it("maps reduced-capture command, event, and metric observations", async () => {
    const { adapter, sockets } = setup();
    const events: import("../adapter").LiveEvent[] = [];
    adapter.subscribe((event) => events.push(event));
    await adapter.auth.login("correct horse battery staple");
    await adapter.getSessions();

    sockets[0].observe({
      type: "MACHINE_METRICS",
      metrics: {
        at: 1_700_000_000_100,
        cpu_percent: 50,
        ram_used_bytes: 8_589_934_592,
        ram_total_bytes: 17_179_869_184,
        gpus: [{ index: 0, util_percent: 96, vram_used_bytes: 12_884_901_888, vram_total_bytes: 25_769_803_776, temp_c: 71 }],
      },
    });
    sockets[0].observe({
      type: "SESSION_METRICS",
      metrics: {
        session_id: 1,
        at: 1_700_000_000_100,
        totals: { cpu_percent: 33.25, rss_bytes: 2_147_483_648, gpu_mem_bytes: 10_737_418_240 },
      },
    });
    sockets[0].observe({ type: "COMMAND_UPDATED", command: { ...command, finished_at: null, exit_code: null } });
    sockets[0].observe({
      type: "EVENT",
      event: {
        id: 13,
        session_id: 1,
        command_id: 9,
        at: 1_700_000_001_000,
        kind: { type: "RESOURCE_HIGH", resource: "GPU", percent: 96 },
      },
    });

    expect(events).toContainEqual({
      type: "SESSION_METRICS",
      sessionId: "test-machine:1",
      metrics: { cpuPct: 33.3, ramGB: 2, ramTotalGB: 16, gpuPct: 96, vramGB: 10, vramTotalGB: 24 },
    });
    expect(events).toContainEqual({ type: "COMMAND_UPDATED", sessionId: "test-machine:1", command: "python train.py" });
    expect(events).toContainEqual(expect.objectContaining({
      type: "EVENT_APPENDED",
      sessionId: "test-machine:1",
      event: expect.arrayContaining(["RESOURCE_HIGH"]),
    }));
  });

  it("rotates the browser session on password change and revokes it on logout", async () => {
    const { adapter, storage } = setup();
    await adapter.auth.login("correct horse battery staple");

    await adapter.auth.changePassword("correct horse battery staple", "a different secure passphrase");
    expect(storage.getItem("sentinal.sessionToken")).toBeNull();
    await adapter.auth.logout();

    expect(storage.getItem("sentinal.sessionToken")).toBeNull();
    await expect(adapter.auth.getState()).resolves.toMatchObject({ status: "signedOut" });
  });

  it("clears the session and announces daemon-enforced expiry", async () => {
    const { adapter, sockets, storage } = setup();
    let expired = false;
    window.addEventListener("terminal-agent-auth-expired", () => {
      expired = true;
    }, { once: true });
    await adapter.auth.login("correct horse battery staple");
    await adapter.getMachines();

    sockets[0].expire();

    expect(storage.getItem("sentinal.sessionToken")).toBeNull();
    expect(expired).toBe(true);
  });
});

describe("apiAdapter multi-Machine routing", () => {
  it("routes Recipe launch and notification settings only to their owning Machine", async () => {
    const { adapter, server, sockets } = setup();
    server.registryMachines = [workerRegistry()];
    await adapter.auth.login("correct horse battery staple");
    await adapter.getMachines();

    const recipes = await adapter.getRecipes();
    const workerRecipe = recipes.find((recipe) => recipe.machine === "worker-machine");
    expect(workerRecipe).toBeDefined();
    const control = sockets.find((socket) => socket.machineId === "test-machine")!;
    const worker = sockets.find((socket) => socket.machineId === "worker-machine")!;
    const controlRunCount = control.requests.filter((request) => request.type === "RUN_RECIPE").length;
    await expect(adapter.runRecipe(workerRecipe!.id, { arguments: "-p protocol" }, "worker-machine"))
      .resolves.toEqual({ sessionId: "worker-machine:3" });
    await adapter.notifications.setRule("worker-machine:default", {
      finished: true, error: true, stalled: false, waiting: true, checkpoint: false, progress: false,
    });

    expect(control.requests.filter((request) => request.type === "RUN_RECIPE")).toHaveLength(controlRunCount);
    expect(worker.requests).toContainEqual({
      type: "RUN_RECIPE", recipe_id: "cargo-test", values: { arguments: "-p protocol" }, session_id: null,
    });
    expect(worker.requests).toContainEqual(expect.objectContaining({
      type: "SET_NOTIFICATION_RULE", session_id: null,
    }));
  });

  it("starts a new Session on the chosen worker through the control", async () => {
    const { adapter, server, sockets } = setup();
    server.registryMachines = [workerRegistry()];
    await adapter.auth.login("correct horse battery staple");
    await adapter.getMachines();
    const control = sockets.find((socket) => socket.machineId === "test-machine")!;
    const worker = sockets.find((socket) => socket.machineId === "worker-machine")!;
    const controlCreates = () => control.requests.filter((request) => request.type === "CREATE_SESSION").length;
    const before = controlCreates();

    const created = await adapter.createSession({ machineId: "worker-machine", cols: 120, rows: 40 });

    expect(created).toMatchObject({ id: "worker-machine:1", machine: "worker-machine" });
    expect(controlCreates()).toBe(before);
    expect(worker.requests).toContainEqual(expect.objectContaining({
      type: "CREATE_SESSION", origin: "UI", cols: 120, rows: 40,
    }));
    await expect(adapter.createSession({ machineId: "no-such-machine" })).rejects.toThrow(/no-such-machine/);
  });

  it("times browser and control round trips to a worker for Diagnostics", async () => {
    const { adapter, server, sockets } = setup();
    server.registryMachines = [workerRegistry()];
    await adapter.auth.login("correct horse battery staple");
    await adapter.getMachines();
    const worker = sockets.find((socket) => socket.machineId === "worker-machine")!;
    const before = worker.requests.filter((request) => request.type === "GET_MACHINE").length;

    const browser = await adapter.diagnostics.ping("worker-machine", 3);
    const control = await adapter.diagnostics.controlPing("worker-machine");

    expect(browser.roundTripsMs).toHaveLength(3);
    expect(browser.failed).toBe(0);
    expect(worker.requests.filter((request) => request.type === "GET_MACHINE").length - before).toBe(3);
    expect(control).toEqual({
      roundTripsMs: [12.5, 4],
      failed: 1,
      error: "the selected Machine is unreachable",
      path: "RELAYED",
      relay: "fra",
    });
    await expect(adapter.diagnostics.ping("no-such-machine")).rejects.toThrow(/no-such-machine/);
  });

  it("keeps each daemon's own hardware description across registry refreshes", async () => {
    const { adapter, server } = setup();
    server.registryMachines = [
      { ...workerRegistry("test-machine"), role: "CONTROL", display_name: "Control", credential: null },
      workerRegistry(),
    ];
    const live: import("../adapter").LiveEvent[] = [];
    adapter.subscribe((event) => live.push(event));
    await adapter.auth.login("correct horse battery staple");

    for (let refresh = 0; refresh < 2; refresh += 1) {
      const machines = await adapter.getMachines();
      for (const id of ["test-machine", "worker-machine"]) {
        expect(machines.find((entry) => entry.id === id)?.hardware).toBe("Apple M4, 16 GB");
      }
    }
    // Live updates after the first query must not regress to the registry's version label.
    for (const id of ["test-machine", "worker-machine"]) {
      const updates = live.flatMap((event) =>
        event.type === "MACHINE_UPDATED" && event.machine.id === id ? [event.machine.hardware] : []);
      const known = updates.indexOf("Apple M4, 16 GB");
      expect(known, id).toBeGreaterThanOrEqual(0);
      expect(updates.slice(known).filter((hardware) => hardware.startsWith("Sentinal")), id).toEqual([]);
    }
  });

  it("hides a foreign-stamped Session without flipping its Machine offline", async () => {
    const { adapter, server } = setup();
    server.registryMachines = [
      { ...workerRegistry("test-machine"), role: "CONTROL", display_name: "Control", credential: null },
      workerRegistry(),
    ];
    server.foreignSessionMachines.add("test-machine");
    server.foreignSessionMachines.add("worker-machine");
    const statuses: string[] = [];
    adapter.subscribe((event) => {
      if (event.type === "MACHINE_UPDATED") statuses.push(`${event.machine.id}:${event.machine.status}`);
    });
    await adapter.auth.login("correct horse battery staple");

    for (let poll = 0; poll < 3; poll += 1) {
      await adapter.getMachines();
      const sessions = await adapter.getSessions();
      expect(sessions.map((entry) => entry.id).sort()).toEqual(["test-machine:1", "worker-machine:1"]);
    }
    expect(statuses.filter((status) => !status.endsWith(":ONLINE"))).toEqual([]);
  });

  it("asks only the owning Machine for its details and metric history", async () => {
    const { adapter, server, sockets } = setup();
    server.registryMachines = [workerRegistry()];
    await adapter.auth.login("correct horse battery staple");
    await adapter.getMachines();

    const details = await adapter.machineInsights.details("worker-machine");
    const history = await adapter.machineInsights.metrics("worker-machine", "DAY");

    expect(details).toMatchObject({
      hostname: "worker-machine-host",
      osVersion: "macOS 26.6.2",
      daemonVersion: "0.1.9",
      processes: [{ sessionId: "worker-machine:1", processes: [expect.objectContaining({ name: "python" })] }],
    });
    expect(history).toEqual([
      { at: 1_700_000_000_000, cpuPct: 42.5, ramPct: 25, gpuPct: undefined, vramPct: undefined },
      { at: 1_700_000_300_000, cpuPct: 10, ramPct: 50, gpuPct: 80, vramPct: 50 },
    ]);
    const control = sockets.find((socket) => socket.machineId === "test-machine");
    const worker = sockets.find((socket) => socket.machineId === "worker-machine");
    expect(worker?.requests).toContainEqual({ type: "GET_MACHINE_METRICS", range: "DAY" });
    expect(control?.requests.some((request) => request.type === "GET_MACHINE_DETAILS" || request.type === "GET_MACHINE_METRICS")).toBe(false);
  });

  it("keeps the owner's display name for a renamed control across polls", async () => {
    const { adapter, server } = setup();
    server.registryMachines = [
      { ...workerRegistry("test-machine"), role: "CONTROL", display_name: "Mac-Mini", credential: null },
    ];
    const names: string[] = [];
    adapter.subscribe((event) => {
      if (event.type === "MACHINE_UPDATED" && event.machine.id === "test-machine") names.push(event.machine.name);
    });
    await adapter.auth.login("correct horse battery staple");

    for (let poll = 0; poll < 3; poll += 1) {
      await adapter.getMachines();
      await adapter.getSessions();
    }
    expect(names.length).toBeGreaterThan(0);
    expect(new Set(names)).toEqual(new Set(["Mac-Mini"]));
  });

  it("keeps equal daemon-local IDs separate and routes every operation to its owner", async () => {
    const { adapter, server, sockets } = setup();
    server.registryMachines = [workerRegistry()];
    await adapter.auth.login("correct horse battery staple");

    await expect(adapter.getMachines()).resolves.toEqual(expect.arrayContaining([
      expect.objectContaining({ id: "test-machine", status: "ONLINE" }),
      expect.objectContaining({ id: "worker-machine", status: "ONLINE", credentialStatus: "ACTIVE" }),
    ]));
    await expect(adapter.getSessions()).resolves.toEqual(expect.arrayContaining([
      expect.objectContaining({ id: "test-machine:1", machine: "test-machine" }),
      expect.objectContaining({ id: "worker-machine:1", machine: "worker-machine" }),
    ]));

    const control = sockets.find((socket) => socket.machineId === "test-machine");
    const worker = sockets.find((socket) => socket.machineId === "worker-machine");
    expect(control).toBeDefined();
    expect(worker).toBeDefined();
    const controlRequestCount = control?.requests.length ?? 0;
    await adapter.sendInput("worker-machine:1", "worker only\n");
    await adapter.sendSignal("worker-machine:1", "KILL");
    await adapter.getHistory({ machine: "worker-machine", query: "train" });

    expect(control?.requests).toHaveLength(controlRequestCount);
    expect(worker?.requests).toContainEqual({ type: "KILL", session_id: 1 });
    expect(worker?.requests).toContainEqual({ type: "SEARCH_COMMANDS", query: "train", limit: 500 });
    expect(worker?.binaryFrames).toHaveLength(1);
    expect(new TextDecoder().decode(decodeTerminalFrame(worker!.binaryFrames[0]).payload)).toBe("worker only\n");
    expect(adapter.resolveTerminal("worker-machine:1")).toEqual({
      websocketUrl: "ws://daemon.test/api/v1/machines/worker-machine/ws",
      localSessionId: 1,
      machineId: "worker-machine",
    });

    const chunks: Uint8Array[] = [];
    await adapter.replayOutput("worker-machine:1", (chunk) => chunks.push(chunk));
    expect(server.outputRequests.at(-1)?.pathname).toBe("/api/v1/machines/worker-machine/sessions/1/output");
    for (const socket of sockets) {
      const endpoint = new URL(socket.url);
      expect(endpoint.origin).toBe("ws://daemon.test");
      expect(endpoint.searchParams.get("token")).toBeNull();
      expect(socket.url).not.toContain("a".repeat(64));
      expect(socket.url).not.toContain("b".repeat(64));
    }
  });

  it("retains an offline snapshot, isolates the healthy Machine, and deduplicates after recovery", async () => {
    const { adapter, server, sockets } = setup();
    server.registryMachines = [workerRegistry()];
    const live: import("../adapter").LiveEvent[] = [];
    const unsubscribe = adapter.subscribe((event) => live.push(event));
    await adapter.auth.login("correct horse battery staple");
    await adapter.getMachines();
    await adapter.getSessions();

    server.unavailableMachines.add("worker-machine");
    const whileOffline = await adapter.getSessions();
    expect(whileOffline).toEqual(expect.arrayContaining([
      expect.objectContaining({ id: "test-machine:1" }),
      expect.objectContaining({ id: "worker-machine:1" }),
    ]));
    await expect(adapter.getMachines()).resolves.toContainEqual(
      expect.objectContaining({ id: "worker-machine", status: "OFFLINE" }),
    );
    await adapter.sendInput("test-machine:1", "still healthy\n");

    server.unavailableMachines.delete("worker-machine");
    await adapter.getSessions();
    const worker = sockets.filter((socket) => socket.machineId === "worker-machine").at(-1);
    const before = live.filter((event) => event.type === "EVENT_APPENDED").length;
    worker?.observe({ type: "EVENT", event: { ...watcherEvent, id: 99 } });
    worker?.observe({ type: "EVENT", event: { ...watcherEvent, id: 99 } });
    expect(live.filter((event) => event.type === "EVENT_APPENDED")).toHaveLength(before + 1);
    unsubscribe();
  });

  it("revokes one Machine without dropping another connection", async () => {
    const { adapter, server } = setup();
    server.registryMachines = [workerRegistry(), workerRegistry("second-worker")];
    await adapter.auth.login("correct horse battery staple");
    await adapter.getMachines();

    await adapter.registry.revoke("worker-machine");

    expect(server.revokedMachines).toEqual(["worker-machine"]);
    await expect(adapter.getMachines()).resolves.toEqual(expect.arrayContaining([
      expect.objectContaining({ id: "test-machine" }),
      expect.objectContaining({ id: "second-worker" }),
    ]));
    await expect(adapter.getMachines()).resolves.not.toContainEqual(
      expect.objectContaining({ id: "worker-machine" }),
    );
  });
});
