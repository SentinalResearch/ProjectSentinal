import { describe, expect, it } from "vitest";

import { decodeTerminalFrame } from "../../frame";
import type { ClientRequest } from "../../protocol/ClientRequest";
import type { DaemonMessage } from "../../protocol/DaemonMessage";
import type { Envelope } from "../../protocol/Envelope";
import type { Machine as WireMachine } from "../../protocol/Machine";
import type { Session as WireSession } from "../../protocol/Session";
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
  created_at: 1_700_000_000_000,
  updated_at: 1_700_000_000_000,
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
    let payload: DaemonMessage;
    if (request.type === "GET_MACHINE") payload = { type: "MACHINE", machine };
    else if (request.type === "LIST_SESSIONS") payload = { type: "SESSIONS", sessions: [session] };
    else if (request.type === "CREATE_SESSION") payload = { type: "SESSION_UPDATED", session };
    else if (request.type === "KILL") {
      payload = { type: "SESSION_UPDATED", session: { ...session, state: "INTERRUPTED" } };
    } else {
      payload = { type: "ERROR", message: `unsupported test request ${request.type}` };
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
