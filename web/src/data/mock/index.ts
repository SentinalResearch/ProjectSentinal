import { config } from "../config";
import type { DataAdapter, HistoryFilters, LiveEvent } from "../adapter";
import type { AuthState, NotificationRuleSet, PairingToken, Session } from "../types";
import { acquireLiveUpdates, resolvePrompt } from "./live";
import { delay, emit, findSession, store, subscribe as subscribeStore, updateSession, appendEvent } from "./store";

let passwordHash: string | null = null;
store.auth.status = "setup";

const CONNECT_DELAY_MS = 5000;

function nowTime(): string {
  const now = new Date();
  return `${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`;
}

function renderTemplate(template: string, params: Record<string, string>): string {
  return template.replace(/\{(\w+)\}/g, (_, key: string) => params[key] ?? `{${key}}`);
}

export const mockAdapter: DataAdapter = {
  async getMachines() {
    return delay(store.machines.map((m) => ({ ...m })));
  },

  async getSessions() {
    return delay(store.sessions.map((s) => ({ ...s })));
  },

  async getRecipes() {
    return delay(store.recipes.map((r) => ({ ...r })));
  },

  async getHistory(filters?: HistoryFilters) {
    let entries = store.history;
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
    const command = renderTemplate(recipe.template, params);
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

  async pairMachine(machineName: string) {
    await delay(undefined, 300, 600);
    const token = `sa_${Math.random().toString(36).slice(2, 10)}`;
    const pairing: PairingToken = {
      machineName,
      token,
      command: config.pairCommand.replace("<TOKEN>", token),
      expiresAt: Date.now() + config.pairTokenTtlMs,
    };
    setTimeout(() => {
      const id = machineName.toLowerCase().replace(/\s+/g, "-");
      if (store.machines.some((m) => m.id === id)) return;
      const machine = { id, name: machineName, os: "Linux" as const, hardware: "Paired machine", status: "ONLINE" as const };
      store.machines.push(machine);
      store.tokens.push({
        machineId: id,
        machineName,
        maskedToken: config.tokenMask.replace("xxxx", String(Math.floor(Math.random() * 9000) + 1000)),
        createdAt: "Today",
      });
      emit({ type: "MACHINE_UPDATED", machine });
    }, CONNECT_DELAY_MS);
    return pairing;
  },

  async rotateToken(machineId: string) {
    await delay(undefined, 300, 600);
    const entry = store.tokens.find((t) => t.machineId === machineId);
    if (!entry) throw new Error(`no token for machine '${machineId}'`);
    entry.maskedToken = config.tokenMask.replace("xxxx", String(Math.floor(Math.random() * 9000) + 1000));
    entry.createdAt = "Today";
    return { ...entry };
  },

  async revokeToken(machineId: string) {
    await delay(undefined, 300, 600);
    store.tokens = store.tokens.filter((t) => t.machineId !== machineId);
  },

  async getMachineTokens() {
    return delay(store.tokens.map((t) => ({ ...t })));
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

    async setup(password: string): Promise<AuthState> {
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
  },
};
