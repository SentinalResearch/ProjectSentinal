import { NotImplemented, type DataAdapter } from "../adapter";

/**
 * Real implementation, talking to the daemon's HTTP/WebSocket API. Nothing here
 * exists yet (the daemon has no history/events/recipes API — see docs/ROADMAP.md
 * milestones 2-4) — every method throws until it's wired up. Select this adapter
 * with VITE_DATA=api.
 */
export const apiAdapter: DataAdapter = {
  getMachines() {
    throw new NotImplemented("getMachines");
  },
  getSessions() {
    throw new NotImplemented("getSessions");
  },
  getRecipes() {
    throw new NotImplemented("getRecipes");
  },
  getHistory() {
    throw new NotImplemented("getHistory");
  },
  subscribe() {
    throw new NotImplemented("subscribe");
  },
  sendInput() {
    throw new NotImplemented("sendInput");
  },
  sendSignal() {
    throw new NotImplemented("sendSignal");
  },
  runRecipe() {
    throw new NotImplemented("runRecipe");
  },
  pairMachine() {
    throw new NotImplemented("pairMachine");
  },
  rotateToken() {
    throw new NotImplemented("rotateToken");
  },
  revokeToken() {
    throw new NotImplemented("revokeToken");
  },
  getMachineTokens() {
    throw new NotImplemented("getMachineTokens");
  },
  auth: {
    getState() {
      throw new NotImplemented("auth.getState");
    },
    login() {
      throw new NotImplemented("auth.login");
    },
    setup() {
      throw new NotImplemented("auth.setup");
    },
    changePassword() {
      throw new NotImplemented("auth.changePassword");
    },
    logout() {
      throw new NotImplemented("auth.logout");
    },
  },
  notifications: {
    getRules() {
      throw new NotImplemented("notifications.getRules");
    },
    setRule() {
      throw new NotImplemented("notifications.setRule");
    },
    getJournal() {
      throw new NotImplemented("notifications.getJournal");
    },
  },
};
