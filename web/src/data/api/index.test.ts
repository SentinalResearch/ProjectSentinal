import { describe, expect, it } from "vitest";

import { decodeTerminalFrame } from "../../frame";
import type { ClientRequest } from "../../protocol/ClientRequest";
import type { DaemonMessage } from "../../protocol/DaemonMessage";
import type { Envelope } from "../../protocol/Envelope";
import type { Machine as WireMachine } from "../../protocol/Machine";
import type { Session as WireSession } from "../../protocol/Session";
import type { Command } from "../../protocol/Command";
import { createApiAdapter, type ApiSocket } from ".";

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
  readonly tokens = new Set<string>();
  private nextToken = 1;
  readonly outputRequests: URL[] = [];

  constructor(password: string | null) {
    this.password = password;
  }

  fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const url = new URL(String(input), "http://daemon.test");
    const request = init ?? {};
    if (url.pathname === "/api/v1/auth/status") {
      return json({ setup_required: this.password === null });
    }
    if (url.pathname === "/api/v1/auth/setup") {
      if (new Headers(request.headers).get("authorization") !== "Bearer bootstrap-token") {
        return failure("INVALID_BOOTSTRAP", "invalid bootstrap machine token", 401);
      }
      this.password = body(request).password as string;
      return json(this.issue());
    }
    if (url.pathname === "/api/v1/auth/login") {
      if (body(request).password !== this.password) {
        return failure("INVALID_CREDENTIALS", "invalid owner password", 401, 4);
      }
      return json(this.issue());
    }
    const token = bearer(request);
    if (!token || !this.tokens.has(token)) {
      return failure("INVALID_SESSION", "invalid or expired browser session", 401);
    }
    if (url.pathname === "/api/v1/auth/session") {
      return json({ expires_at: 1_790_086_400_000 });
    }
    if (url.pathname === "/api/v1/auth/logout") {
      this.tokens.delete(token);
      return new Response(null, { status: 204 });
    }
    if (url.pathname === "/api/v1/auth/password") {
      const values = body(request);
      if (values.current_password !== this.password) {
        return failure("INVALID_CREDENTIALS", "invalid owner password", 401, 0);
      }
      this.password = values.new_password as string;
      this.tokens.clear();
      return json(this.issue());
    }
    if (url.pathname === "/api/v1/sessions/1/output") {
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
    const token = `session-token-${this.nextToken++}`;
    this.tokens.add(token);
    return { token, expires_at: 1_790_086_400_000 };
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

  constructor(url: string, server: FakeAuthServer) {
    this.url = url;
    queueMicrotask(() => {
      const token = new URL(url).searchParams.get("token");
      if (!token || !server.tokens.has(token)) {
        this.onerror?.(new Event("error"));
        return;
      }
      this.readyState = WebSocket.OPEN;
      this.onopen?.(new Event("open"));
    });
  }

  send(data: string | ArrayBufferLike | Blob | ArrayBufferView) {
    if (typeof data !== "string") {
      const bytes = data instanceof Uint8Array ? data : new Uint8Array(data as ArrayBuffer);
      this.binaryFrames.push(bytes);
      return;
    }
    const request = (JSON.parse(data) as Envelope<ClientRequest>).payload;
    this.requests.push(request);
    let payload: DaemonMessage;
    if (request.type === "GET_MACHINE") payload = { type: "MACHINE", machine };
    else if (request.type === "LIST_SESSIONS") payload = { type: "SESSIONS", sessions: [session] };
    else if (request.type === "CREATE_SESSION") payload = { type: "SESSION_UPDATED", session };
    else if (request.type === "CONTINUE_SESSION") {
      payload = {
        type: "SESSION_UPDATED",
        session: { ...session, id: 2, state: "IDLE", continued_from: request.session_id },
      };
    }
    else if (request.type === "KILL") {
      payload = { type: "SESSION_UPDATED", session: { ...session, state: "INTERRUPTED" } };
    } else if (request.type === "GET_HISTORY" || request.type === "SEARCH_COMMANDS") {
      payload = { type: "HISTORY", commands: [command] };
    } else {
      payload = { type: "ERROR", message: `unsupported test request ${request.type}` };
    }
    if (this.observeFirst) {
      const observed: DaemonMessage = {
        type: "SESSION_UPDATED",
        session: { ...session, id: 77, origin: "ADOPTED", capture: "PROCESS", state: "RUNNING" },
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
}

function body(request: RequestInit): Record<string, unknown> {
  return JSON.parse(String(request.body ?? "{}")) as Record<string, unknown>;
}

function bearer(request: RequestInit): string | null {
  return new Headers(request.headers).get("authorization")?.replace(/^Bearer /, "") ?? null;
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

describe("apiAdapter authentication", () => {
  it("bootstraps the single owner and stores only the browser session token", async () => {
    const { adapter, storage } = setup(null);

    await expect(adapter.auth.getState()).resolves.toMatchObject({ status: "setup" });
    await expect(
      adapter.auth.setup("bootstrap-token", "correct horse battery staple"),
    ).resolves.toMatchObject({ status: "signedIn" });

    expect(storage.getItem("sentinal.sessionToken")).toBe("session-token-1");
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
      expect.objectContaining({ id: "1", machine: "test-machine", name: "Session 1", state: "IDLE" }),
    ]);
    expect(sockets).toHaveLength(1);
  });

  it("returns remaining attempts for an invalid password", async () => {
    const { adapter, storage } = setup();

    await expect(adapter.auth.login("wrong password")).resolves.toMatchObject({
      status: "wrong",
      attempts: 1,
    });
    expect(storage.getItem("sentinal.sessionToken")).toBeNull();
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

    expect(created.id).toBe("1");
    expect(updates).toEqual(["1"]);
    const frame = decodeTerminalFrame(sockets[0].binaryFrames[0]);
    expect(frame.sessionId).toBe(1);
    expect(new TextDecoder().decode(frame.payload)).toBe("echo ready\n");
    unsubscribe();
  });

  it("continues through the dedicated idempotent daemon request", async () => {
    const { adapter, sockets } = setup();
    await adapter.auth.login("correct horse battery staple");

    const continued = await adapter.continueSession("1");

    expect(sockets[0].requests.at(-1)).toEqual({
      type: "CONTINUE_SESSION",
      session_id: 1,
    });
    expect(continued).toMatchObject({ id: "2", continuedFrom: "1" });
  });

  it("pages authenticated saved output as bytes and reports decoder warnings", async () => {
    const { adapter, server } = setup();
    await adapter.auth.login("correct horse battery staple");
    const chunks: Uint8Array[] = [];

    const replay = await adapter.replayOutput("1", (chunk) => chunks.push(chunk));

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

    const replay = await adapter.replayOutput("1", (chunk) => chunks.push(chunk), {
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
    expect(created.id).toBe("1");
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
        sessionId: "1",
      }),
    ]);
    expect(sockets[0].requests).toContainEqual({ type: "SEARCH_COMMANDS", query: "train", limit: 500 });

    await adapter.getHistory({ sessionId: "1" });
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
        id: 12,
        session_id: 1,
        command_id: 9,
        at: 1_700_000_001_000,
        kind: { type: "RESOURCE_HIGH", resource: "GPU", percent: 96 },
      },
    });

    expect(events).toContainEqual({
      type: "SESSION_METRICS",
      sessionId: "1",
      metrics: { cpuPct: 33.3, ramGB: 2, gpuPct: 96, vramGB: 10, vramTotalGB: 24 },
    });
    expect(events).toContainEqual({ type: "COMMAND_UPDATED", sessionId: "1", command: "python train.py" });
    expect(events).toContainEqual(expect.objectContaining({
      type: "EVENT_APPENDED",
      sessionId: "1",
      event: expect.arrayContaining(["RESOURCE_HIGH"]),
    }));
  });

  it("rotates the browser session on password change and revokes it on logout", async () => {
    const { adapter, storage } = setup();
    await adapter.auth.login("correct horse battery staple");

    await adapter.auth.changePassword("correct horse battery staple", "a different secure passphrase");
    expect(storage.getItem("sentinal.sessionToken")).toBe("session-token-2");
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
