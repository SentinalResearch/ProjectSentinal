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

class FakeSocket implements ApiSocket {
  static readonly OPEN = 1;

  readonly url: string;
  readyState = 0;
  binaryType = "blob";
  onopen: ((event: Event) => unknown) | null = null;
  onclose: ((event: CloseEvent) => unknown) | null = null;
  onerror: ((event: Event) => unknown) | null = null;
  onmessage: ((event: MessageEvent<string | ArrayBuffer | Blob>) => void) | null = null;
  readonly binaryFrames: Uint8Array[] = [];

  constructor(url: string) {
    this.url = url;
    queueMicrotask(() => {
      if (new URL(url).searchParams.get("token") !== "test-token") {
        this.onerror?.(new Event("error"));
        return;
      }
      this.readyState = FakeSocket.OPEN;
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
    this.readyState = 3;
    this.onclose?.(new CloseEvent("close"));
  }
}

function setup() {
  const storage = new MemoryStorage();
  const sockets: FakeSocket[] = [];
  const adapter = createApiAdapter({
    daemonUrl: "ws://127.0.0.1:7420/api/v1/ws",
    storage,
    socketFactory: (url) => {
      const socket = new FakeSocket(url);
      sockets.push(socket);
      return socket;
    },
  });
  return { adapter, sockets, storage };
}

describe("apiAdapter", () => {
  it("authenticates with the daemon token and maps Milestone 1 state", async () => {
    const { adapter, sockets, storage } = setup();

    await expect(adapter.auth.login("test-token")).resolves.toMatchObject({ status: "signedIn" });
    await expect(adapter.getMachines()).resolves.toEqual([
      expect.objectContaining({ id: "test-machine", name: "Test Machine", os: "macOS", status: "ONLINE" }),
    ]);
    await expect(adapter.getSessions()).resolves.toEqual([
      expect.objectContaining({ id: "1", machine: "test-machine", name: "Session 1", state: "IDLE" }),
    ]);

    expect(storage.getItem("sentinal.daemonToken")).toBe("test-token");
    expect(sockets).toHaveLength(1);
  });

  it("creates sessions, emits updates, and frames remote input", async () => {
    const { adapter, sockets } = setup();
    const updates: string[] = [];
    const unsubscribe = adapter.subscribe((event) => {
      if (event.type === "SESSION_UPDATED") updates.push(event.session.id);
    });
    await adapter.auth.login("test-token");

    const created = await adapter.createSession();
    await adapter.sendInput(created.id, "echo ready\n");

    expect(created.id).toBe("1");
    expect(updates).toEqual(["1"]);
    const frame = decodeTerminalFrame(sockets[0].binaryFrames[0]);
    expect(frame.sessionId).toBe(1);
    expect(new TextDecoder().decode(frame.payload)).toBe("echo ready\n");
    unsubscribe();
  });

  it("rejects an invalid daemon token without persisting it", async () => {
    const { adapter, storage } = setup();

    await expect(adapter.auth.login("wrong-token")).resolves.toMatchObject({ status: "wrong" });

    expect(storage.getItem("sentinal.daemonToken")).toBeNull();
  });
});
