import { config } from "../config";
import { RegistryError, type DataAdapter, type HistoryFilters, type LiveEvent } from "../adapter";
import { previewTemplate } from "../recipeTemplate";
import type { AuthState, NotificationRuleSet, Session } from "../types";
import { mockInsight, mockMetrics } from "./insights";
import { acquireLiveUpdates, resolvePrompt } from "./live";
import { delay, emit, findSession, store, subscribe as subscribeStore, updateSession, appendEvent } from "./store";

let passwordHash: string | null = null;
store.auth.status = "setup";

const CONNECT_DELAY_MS = 5000;

function nowTime(): string {
  const now = new Date();
  return `${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`;
}


export const mockAdapter: DataAdapter = {
  async getMachines() {
    return delay(store.machines.map((m) => ({ ...m })));
  },

  async getSessions() {
    return delay(store.sessions.map((s) => ({ ...s })));
  },

  async createSession(options = {}) {
    await delay(undefined);
    const machine = store.machines.find(
      (entry) => entry.status === "ONLINE" && (options.machineId === undefined || entry.id === options.machineId),
    );
    if (!machine) throw new Error("cannot create session: no online machine");
    const id = `session-${Date.now()}`;
    const session: Session = {
      id,
      machine: machine.id,
      name: options.title ?? "New session",
      cwd: options.cwd,
      shell: options.shell,
      state: "CREATED",
      origin: "UI",
      capture: "FULL",
      runningFor: "0s",
      events: [[nowTime(), "SESSION_CREATED", "terminal session created"]],
    };
    store.sessions.push(session);
    updateSession(id, { state: "CREATED" });
    return { ...session };
  },

  async continueSession(sessionId) {
    await delay(undefined);
    const existing = store.sessions.find((session) => session.continuedFrom === sessionId);
    if (existing) return { ...existing };
    const source = findSession(sessionId);
    const id = `session-${Date.now()}`;
    const session: Session = {
      id,
      machine: source.machine,
      name: source.name,
      project: source.project,
      cwd: source.cwd,
      shell: source.shell,
      state: "IDLE",
      origin: "UI",
      capture: "FULL",
      continuedFrom: source.id,
      createdAt: Date.now(),
      runningFor: "0s",
      events: [],
    };
    store.sessions.push(session);
    updateSession(id, { state: "IDLE" });
    return { ...session };
  },

  async replayOutput() {
    return { bytesRead: 0 };
  },

  resolveTerminal() {
    throw new Error("mock Sessions do not have daemon terminal routes");
  },

  async refreshSessionDetail() {
    // Mock Sessions already carry every field (including cwd) from fixtures.json.
  },

  async getRecipes() {
    return delay(store.recipes.map((r) => ({ ...r })));
  },

  async getHistory(filters?: HistoryFilters) {
    let entries = store.history;
    if (filters?.sessionId) entries = entries.filter((h) => h.sessionId === filters.sessionId);
    if (filters?.machine) entries = entries.filter((h) => h.machine === filters.machine);
    if (filters?.project) entries = entries.filter((h) => h.project === filters.project);
    if (filters?.result === "ok") entries = entries.filter((h) => h.ok === true);
    if (filters?.result === "failed") entries = entries.filter((h) => h.ok === false);
    if (filters?.result === "running") entries = entries.filter((h) => h.ok === null);
    if (filters?.query) {
      const q = filters.query.toLowerCase();
      entries = entries.filter((h) => h.command.toLowerCase().includes(q));
    }
    return delay(entries.map((e) => ({ ...e })));
  },

  subscribe(listener: (event: LiveEvent) => void) {
    const releaseTicks = acquireLiveUpdates();
    const unsubscribeStore = subscribeStore(listener);
    return () => {
      unsubscribeStore();
      releaseTicks();
    };
  },

  async sendInput(sessionId: string, data: string) {
    await delay(undefined, 100, 250);
    const session = findSession(sessionId);
    const trimmed = data.trim().toLowerCase();
    if (session.state === "WAITING" && (trimmed === "y" || trimmed === "n")) {
      resolvePrompt(sessionId, trimmed);
    }
  },

  async sendSignal(sessionId: string, signal: "SIGINT" | "KILL") {
    await delay(undefined, 200, 500);
    updateSession(sessionId, { state: "INTERRUPTED", prompt: undefined });
    appendEvent(sessionId, [nowTime(), "COMMAND_INTERRUPTED", signal === "SIGINT" ? "Ctrl-C sent" : "killed"]);
  },

  async runRecipe(recipeId: string, params: Record<string, string>, machineId: string) {
    await delay(undefined, 300, 600);
    const recipe = store.recipes.find((r) => r.id === recipeId);
    if (!recipe) throw new Error(`unknown recipe '${recipeId}'`);
    const command = previewTemplate(recipe.template, params);
    const id = `${recipeId}-${Date.now()}`;
    const session: Session = {
      id,
      machine: machineId,
      name: recipe.name,
      command,
      state: "CREATED",
      runningFor: "0s",
      events: [[nowTime(), "COMMAND_STARTED", command]],
    };
    store.sessions.push(session);
    updateSession(id, { state: "CREATED" });
    setTimeout(() => updateSession(id, { state: "RUNNING" }), 1200);
    return { sessionId: id };
  },

  async saveRecipe(machineId: string, recipe: import("../types").Recipe) {
    await delay(undefined, 100, 200);
    const stored = { ...recipe, machine: machineId, machineIds: [machineId] };
    const index = store.recipes.findIndex((item) => item.id === recipe.id);
    if (index >= 0) store.recipes[index] = stored;
    else store.recipes.push(stored);
  },

  registry: {
    async list() {
      return delay(structuredClone(store.registry));
    },

    async createCode() {
      await delay(undefined, 200, 400);
      const symbols = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
      const raw = Array.from({ length: 10 }, () => symbols[Math.floor(Math.random() * 32)]).join("");
      const code_id = `pc_mock${Date.now()}`;
      const expires_at = Date.now() + 10 * 60_000;
      store.pairingCodes.push({ code_id, state: "WAITING", expires_at, machine_id: null });
      // Mock only: pretend a worker typed the code a few seconds later.
      setTimeout(() => {
        const pending = store.pairingCodes.find((entry) => entry.code_id === code_id);
        if (!pending || pending.state !== "WAITING") return;
        const machine_id = `01MOCK${Date.now().toString(36).toUpperCase().padStart(20, "0")}`.slice(0, 26);
        pending.state = "CONNECTED";
        pending.machine_id = machine_id;
        store.registry.push({
          machine_id,
          display_name: "New worker",
          role: "WORKER",
          status: "ONLINE",
          last_seen: Date.now(),
          endpoint: { address: "100.64.0.42", port: 7420, dns_name: null },
          os: "LINUX",
          daemon_version: "0.0.1",
          capabilities: ["SESSIONS", "EVENTS", "METRICS", "ADOPT"],
          credential: { link_id: `lk_${code_id}`, created_at: Date.now(), expires_at: Date.now() + 90 * 86_400_000, last_used_at: Date.now() },
        });
        const machine = { id: machine_id, name: "New worker", os: "Linux" as const, hardware: "Connected worker", status: "ONLINE" as const };
        store.machines.push(machine);
        emit({ type: "MACHINE_UPDATED", machine });
      }, CONNECT_DELAY_MS);
      return {
        code_id,
        code: `${raw.slice(0, 5)}-${raw.slice(5)}`,
        expires_at,
        connect_command: "sentinal connect https://sentinal-control.example.ts.net",
      };
    },

    async codeStatus(codeId: string) {
      const entry = store.pairingCodes.find((code) => code.code_id === codeId);
      if (!entry) throw new RegistryError("NOT_FOUND", "unknown pairing code");
      if (entry.state === "WAITING" && Date.now() >= entry.expires_at) entry.state = "EXPIRED";
      return delay({ ...entry }, 50, 100);
    },

    async cancelCode(codeId: string) {
      const entry = store.pairingCodes.find((code) => code.code_id === codeId && code.state === "WAITING");
      if (!entry) throw new RegistryError("NOT_FOUND", "no outstanding pairing code with that ID");
      entry.state = "CANCELLED";
    },

    async rename(machineId: string, displayName: string) {
      await delay(undefined, 150, 300);
      const name = displayName.trim();
      if (!name || name.length > 64) throw new RegistryError("INVALID_REQUEST", "display_name must be 1 to 64 characters");
      const entry = store.registry.find((machine) => machine.machine_id === machineId);
      if (!entry) throw new RegistryError("NOT_FOUND", "unknown Machine");
      entry.display_name = name;
      return structuredClone(entry);
    },

    async revoke(machineId: string) {
      await delay(undefined, 150, 300);
      const entry = store.registry.find((machine) => machine.machine_id === machineId);
      if (!entry) throw new RegistryError("NOT_FOUND", "unknown Machine");
      if (entry.role === "CONTROL") throw new RegistryError("INVALID_REQUEST", "the control Machine cannot be revoked");
      entry.status = "REVOKED";
      entry.credential = null;
      entry.endpoint = null;
    },
  },

  auth: {
    async getState() {
      return delay({ ...store.auth });
    },

    async login(password: string): Promise<AuthState> {
      await delay(undefined, 400, 800);
      if (store.auth.status === "locked" && store.auth.lockUntil !== null) {
        if (Date.now() < store.auth.lockUntil) {
          return { ...store.auth };
        }
        store.auth = { status: "signedOut", attempts: 0, lockUntil: null };
      }
      if (password === passwordHash) {
        store.auth = { status: "signedIn", attempts: 0, lockUntil: null };
        return { ...store.auth };
      }
      const attempts = store.auth.attempts + 1;
      if (attempts >= config.loginMaxAttempts) {
        store.auth = { status: "locked", attempts, lockUntil: Date.now() + config.loginLockMs };
      } else {
        store.auth = { status: "wrong", attempts, lockUntil: null };
      }
      return { ...store.auth };
    },

    async setup(_bootstrapToken: string, password: string): Promise<AuthState> {
      await delay(undefined, 400, 800);
      if (password.length < config.setupMinPasswordLength) {
        throw new Error(`password must be at least ${config.setupMinPasswordLength} characters`);
      }
      passwordHash = password;
      store.auth = { status: "signedIn", attempts: 0, lockUntil: null };
      return { ...store.auth };
    },

    async changePassword(current: string, next: string) {
      await delay(undefined, 400, 800);
      if (current !== passwordHash) throw new Error("current password is incorrect");
      if (next.length < config.setupMinPasswordLength) {
        throw new Error(`password must be at least ${config.setupMinPasswordLength} characters`);
      }
      passwordHash = next;
    },

    async logout() {
      await delay(undefined, 150, 300);
      store.auth = { status: "signedOut", attempts: 0, lockUntil: null };
    },
  },

  machineInsights: {
    details: mockInsight,
    metrics: mockMetrics,
  },

  diagnostics: {
    async ping(machineId, samples = 5) {
      const machine = store.machines.find((entry) => entry.id === machineId);
      if (!machine) throw new Error(`unknown Machine '${machineId}'`);
      if (machine.status !== "ONLINE") return delay({ roundTripsMs: [], failed: samples, error: "the selected Machine is unreachable" });
      const base = machine.id === "rtx" ? 38 : 9;
      return delay({ roundTripsMs: Array.from({ length: samples }, (_, index) => base + index * 1.5), failed: 0 });
    },
    async controlPing(machineId) {
      const machine = store.machines.find((entry) => entry.id === machineId);
      if (!machine) throw new Error(`unknown Machine '${machineId}'`);
      if (machine.status !== "ONLINE") {
        return delay({ roundTripsMs: [], failed: 5, error: "the selected Machine is unreachable", path: "UNKNOWN" as const });
      }
      return delay(machine.id === "rtx"
        ? { roundTripsMs: [31, 29.5, 30.2, 29.8, 30.1], failed: 0, path: "RELAYED" as const, relay: "fra" }
        : { roundTripsMs: [3.1, 2.4, 2.2, 2.5, 2.3], failed: 0, path: "DIRECT" as const });
    },
  },

  notifications: {
    async getRules() {
      return delay({ ...store.notificationRules });
    },
    async setRule(sessionId: string, rules: NotificationRuleSet) {
      await delay(undefined, 150, 300);
      store.notificationRules[sessionId] = rules;
    },
    async getJournal() {
      return delay(store.journal.map((j) => ({ ...j })));
    },
    async getStatus() {
      return delay(store.machines.map((machine) => ({
        machineId: machine.id,
        configured: true,
        server: "https://ntfy.sh",
        topic: "sentinal-demo",
        tokenConfigured: false,
      })));
    },
  },
};
