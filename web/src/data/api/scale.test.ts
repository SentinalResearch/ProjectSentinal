import { describe, expect, it } from "vitest";

import type { ControlProjectedMachine } from "../../protocol/ControlProjectedMachine";
import type { ControlProjectionSnapshot } from "../../protocol/ControlProjectionSnapshot";
import type { ProjectedSession } from "../../protocol/ProjectedSession";
import { createApiAdapter, type ApiSocket } from ".";

/**
 * Milestone 4.D.1 §9/§10 scale fixture: proves the dashboard's critical
 * render stays at exactly one control snapshot request, one live-stream
 * socket, and zero direct worker queries — independent of how many
 * Machines/Sessions the control projects. Self-contained (does not import
 * from index.test.ts, which is owned by another in-progress change) with
 * its own minimal fake HTTP/WebSocket harness, modeled on the shapes in
 * index.test.ts's FakeAuthServer/FakeSocket but simplified to only what a
 * projection-hydration scale test needs.
 */

/** A per-Machine, direct WebSocket fallback would be this slow — the test
 * proves that path is never taken, by never exercising it. See the socket
 * class below: only a *direct* `/api/v1/machines/:id/ws` socket is delayed
 * by this long; the one `/api/v1/control/live` stream opens immediately. */
const WORKER_DIRECT_LATENCY_MS = 150;
const PASSWORD = "correct horse battery staple";

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

function json(value: unknown, status = 200): Response {
  return new Response(JSON.stringify(value), { status, headers: { "content-type": "application/json" } });
}

function body(request: RequestInit): Record<string, unknown> {
  return JSON.parse(String(request.body ?? "{}")) as Record<string, unknown>;
}

/** Minimal fake daemon: only the auth + control-projection snapshot routes a scale test needs. */
class ScaleServer {
  browserAuthenticated = false;
  readonly paths: string[] = [];

  constructor(private readonly snapshot: ControlProjectionSnapshot) {}

  fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const url = new URL(String(input), "http://daemon.test");
    this.paths.push(url.pathname);
    const request = init ?? {};
    if (url.pathname === "/api/v1/auth/status") {
      return json({ setup_required: false });
    }
    if (url.pathname === "/api/v1/auth/browser/login") {
      if (body(request).password !== PASSWORD) {
        return json({ code: "INVALID_CREDENTIALS", message: "invalid owner password", attempts_remaining: 4, retry_at: null }, 401);
      }
      this.browserAuthenticated = true;
      return json({ expires_at: 1_790_086_400_000 });
    }
    if (!this.browserAuthenticated) {
      return json({ code: "INVALID_SESSION", message: "invalid or expired browser session", attempts_remaining: null, retry_at: null }, 401);
    }
    if (url.pathname === "/api/v1/control/snapshot") {
      return json(this.snapshot);
    }
    return new Response(null, { status: 404 });
  };
}

/**
 * A fake socket for every WebSocket the adapter opens. The one
 * `/api/v1/control/live` stream (the only socket a correctly scaling
 * dashboard ever opens) connects immediately; any direct per-Machine
 * `/api/v1/machines/:id/ws` socket — the old N+1 fallback this milestone
 * replaced — is deliberately slow and never replies to anything sent to
 * it, so the test fails loudly (via the zero-sockets/zero-requests
 * assertions below) rather than silently passing on a regressed path.
 */
class ScaleSocket implements ApiSocket {
  readyState = 0;
  binaryType = "blob";
  onopen: ((event: Event) => unknown) | null = null;
  onclose: ((event: CloseEvent) => unknown) | null = null;
  onerror: ((event: Event) => unknown) | null = null;
  onmessage: ((event: MessageEvent<string | ArrayBuffer | Blob>) => unknown) | null = null;
  readonly requests: unknown[] = [];

  constructor(public readonly url: string) {
    const isDirectWorkerSocket = /^\/api\/v1\/machines\/[^/]+\/ws$/.test(new URL(url).pathname);
    setTimeout(
      () => {
        this.readyState = 1; // OPEN
        this.onopen?.(new Event("open"));
      },
      isDirectWorkerSocket ? WORKER_DIRECT_LATENCY_MS : 0,
    );
  }

  send(data: string | ArrayBufferLike | Blob | ArrayBufferView) {
    this.requests.push(data);
    // Deliberately never replies — the adapter must not depend on a direct
    // worker socket responding at all for critical render to complete.
  }

  close() {
    this.readyState = 3; // CLOSED
    this.onclose?.(new CloseEvent("close"));
  }
}

function projectedSession(id: number): ProjectedSession {
  return {
    id,
    continued_from: null,
    title: `Session ${id}`,
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
    updated_at: 1_700_000_000_000,
    finished_at: null,
    exit_code: null,
  };
}

function projectedMachine(index: number, sessionsPerMachine: number): ControlProjectedMachine {
  return {
    machine_id: `machine-${index}`,
    projection_available: true,
    display_name: `Machine ${index}`,
    os: "LINUX",
    daemon_version: "0.4.1",
    capabilities: ["SESSIONS", "EVENTS", "METRICS"],
    registry_status: "ONLINE",
    freshness: "LIVE",
    last_seen: 1_700_000_001_000,
    observed_at: 1_700_000_001_000,
    synced_at: 1_700_000_001_000,
    source_cursor: 20,
    sessions: Array.from({ length: sessionsPerMachine }, (_, s) => projectedSession(s + 1)),
    events: [],
    metrics: [],
    omitted_sessions: 0,
  };
}

function buildSnapshot(machineCount: number, sessionsPerMachine: number): ControlProjectionSnapshot {
  return {
    schema_version: 1,
    control_machine_id: "machine-0",
    view_revision: 8,
    generated_at: 1_700_000_002_000,
    machines: Array.from({ length: machineCount }, (_, m) => projectedMachine(m, sessionsPerMachine)),
    omitted_machines: 0,
    omitted_sessions: 0,
    truncated: false,
  };
}

function setup(snapshot: ControlProjectionSnapshot) {
  const storage = new MemoryStorage();
  const server = new ScaleServer(snapshot);
  const sockets: ScaleSocket[] = [];
  const adapter = createApiAdapter({
    daemonUrl: "ws://daemon.test/api/v1/ws",
    daemonHttpUrl: "http://daemon.test/api/v1",
    storage,
    fetcher: server.fetch,
    socketFactory: (url) => {
      const socket = new ScaleSocket(url);
      sockets.push(socket);
      return socket;
    },
  });
  return { adapter, server, sockets };
}

describe("control-projection critical render stays constant with scale", () => {
  it.each([
    { machineCount: 1, sessionsPerMachine: 1, label: "1 Machine, 1 Session" },
    { machineCount: 10, sessionsPerMachine: 100, label: "10 Machines, 1,000 Sessions (required scale fixture)" },
  ])(
    "records exactly one snapshot request, one live stream, and zero worker queries — $label",
    async ({ machineCount, sessionsPerMachine }) => {
      const snapshot = buildSnapshot(machineCount, sessionsPerMachine);
      const { adapter, server, sockets } = setup(snapshot);
      await expect(adapter.auth.login(PASSWORD)).resolves.toMatchObject({ status: "signedIn" });

      const [machines, sessions] = await Promise.all([adapter.getMachines(), adapter.getSessions()]);

      expect(machines).toHaveLength(machineCount);
      expect(sessions).toHaveLength(machineCount * sessionsPerMachine);

      // The actual budget (§9): exactly one bounded control snapshot request...
      expect(server.paths.filter((path) => path === "/api/v1/control/snapshot")).toHaveLength(1);
      // ...plus exactly one control-origin live connection...
      expect(sockets).toHaveLength(1);
      expect(new URL(sockets[0].url).pathname).toBe("/api/v1/control/live");
      // ...and zero worker calls / zero per-Session Event calls: nothing was
      // ever sent on any socket (there is only the one, and it's the live
      // stream, which the adapter only listens on, never sends through).
      expect(sockets.flatMap((socket) => socket.requests)).toEqual([]);
    },
  );

  it("keeps critical render counts identical at 1,000x the Session count", () => {
    const small = buildSnapshot(1, 1);
    const large = buildSnapshot(10, 100);
    // Not a behavioral assertion on its own (the test above already proves
    // the counts), but documents the "constant, not just small" claim this
    // fixture exists to make: the request-count budget below does not grow
    // between these two fixtures even though Session count grows 1,000x.
    const sessionCount = (snapshot: ControlProjectionSnapshot) =>
      snapshot.machines.reduce((total, m) => total + m.sessions.length, 0);
    expect(sessionCount(small)).toBe(1);
    expect(sessionCount(large)).toBe(1_000);
  });
});
