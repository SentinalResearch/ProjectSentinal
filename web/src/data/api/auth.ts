import type { AuthFailure } from "../../protocol/AuthFailure";
import type { AuthSessionInfo } from "../../protocol/AuthSessionInfo";
import type { AuthStatus } from "../../protocol/AuthStatus";
import type { DataAdapter } from "../adapter";
import { config } from "../config";
import type { AuthState } from "../types";

export const SESSION_TOKEN_KEY = "sentinal.sessionToken";
export const AUTH_EXPIRED_EVENT = "terminal-agent-auth-expired";

export interface TokenStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

interface AuthApiOptions {
  baseUrl: string;
  fetcher: typeof fetch;
  storage: TokenStorage;
  resetTransport: () => void;
}

class AuthRequestError extends Error {
  constructor(readonly failure: AuthFailure) {
    super(failure.message);
    this.name = "AuthRequestError";
  }
}

export function defaultDaemonHttpUrl(): string {
  if (import.meta.env.VITE_DAEMON_HTTP_URL) return import.meta.env.VITE_DAEMON_HTTP_URL;
  if (import.meta.env.VITE_DAEMON_URL) {
    const endpoint = new URL(import.meta.env.VITE_DAEMON_URL);
    endpoint.protocol = endpoint.protocol === "wss:" ? "https:" : "http:";
    endpoint.pathname = endpoint.pathname.replace(/\/ws$/, "");
    endpoint.search = "";
    return endpoint.toString().replace(/\/$/, "");
  }
  return "/api/v1";
}

export function createAuthApi(options: AuthApiOptions): DataAdapter["auth"] {
  const { baseUrl, fetcher, storage, resetTransport } = options;

  const request = async <T>(path: string, init: RequestInit = {}): Promise<T> => {
    const response = await fetcher(`${baseUrl}${path}`, {
      ...init,
      credentials: "same-origin",
      headers: {
        ...(init.body ? { "content-type": "application/json" } : {}),
        ...init.headers,
      },
    });
    if (response.status === 204) return undefined as T;
    const value = (await response.json()) as T | AuthFailure;
    if (!response.ok) throw new AuthRequestError(value as AuthFailure);
    return value as T;
  };

  const clearSession = () => {
    // Remove credentials written by releases before browser sessions became
    // HttpOnly cookies. JavaScript never stores the replacement credential.
    storage.removeItem(SESSION_TOKEN_KEY);
    resetTransport();
  };

  const acceptSession = (_session: AuthSessionInfo) => {
    storage.removeItem(SESSION_TOKEN_KEY);
    resetTransport();
  };

  const stateFromFailure = (error: unknown): AuthState => {
    if (!(error instanceof AuthRequestError)) {
      return { status: "wrong", attempts: 0, lockUntil: null, message: "Could not reach the daemon." };
    }
    const { failure } = error;
    if (failure.code === "LOCKED") {
      return {
        status: "locked",
        attempts: config.loginMaxAttempts,
        lockUntil: failure.retry_at,
        message: failure.message,
      };
    }
    if (failure.code === "SETUP_REQUIRED") {
      return { status: "setup", attempts: 0, lockUntil: null, message: failure.message };
    }
    const attempts =
      failure.attempts_remaining === null ? 0 : config.loginMaxAttempts - failure.attempts_remaining;
    return {
      status: "wrong",
      attempts,
      lockUntil: null,
      message: failure.code === "INVALID_CREDENTIALS" ? undefined : failure.message,
    };
  };

  return {
    async getState(): Promise<AuthState> {
      const status = await request<AuthStatus>("/auth/status");
      if (status.setup_required) {
        clearSession();
        return { status: "setup", attempts: 0, lockUntil: null };
      }
      try {
        await request("/auth/browser/session");
        return { status: "signedIn", attempts: 0, lockUntil: null };
      } catch (error) {
        clearSession();
        if (error instanceof AuthRequestError && error.failure.code === "INVALID_SESSION") {
          return { status: "signedOut", attempts: 0, lockUntil: null };
        }
        return { status: "signedOut", attempts: 0, lockUntil: null };
      }
    },
    async login(password) {
      try {
        const session = await request<AuthSessionInfo>("/auth/browser/login", {
          method: "POST",
          body: JSON.stringify({ password }),
        });
        acceptSession(session);
        return { status: "signedIn", attempts: 0, lockUntil: null };
      } catch (error) {
        clearSession();
        return stateFromFailure(error);
      }
    },
    async setup(bootstrapToken, password) {
      try {
        const session = await request<AuthSessionInfo>("/auth/browser/setup", {
          method: "POST",
          headers: { authorization: `Bearer ${bootstrapToken}` },
          body: JSON.stringify({ password }),
        });
        acceptSession(session);
        return { status: "signedIn", attempts: 0, lockUntil: null };
      } catch (error) {
        clearSession();
        if (error instanceof AuthRequestError && error.failure.code === "ALREADY_CONFIGURED") {
          return { status: "signedOut", attempts: 0, lockUntil: null, message: error.message };
        }
        return {
          status: "setup",
          attempts: 0,
          lockUntil: null,
          message: error instanceof Error ? error.message : "Could not configure the owner password.",
        };
      }
    },
    async changePassword(current, next) {
      const session = await request<AuthSessionInfo>("/auth/browser/password", {
        method: "POST",
        body: JSON.stringify({ current_password: current, new_password: next }),
      });
      acceptSession(session);
    },
    async logout() {
      try {
        await request<void>("/auth/browser/logout", {
          method: "POST",
        });
      } finally {
        clearSession();
      }
    },
  };
}
