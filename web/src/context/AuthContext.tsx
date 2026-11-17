import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

import { dataAdapter } from "../data";
import { AUTH_EXPIRED_EVENT } from "../data/api";
import type { AuthState } from "../data/types";

interface AuthContextValue extends AuthState {
  loading: boolean;
  /** False when the last status check could not reach the daemon at all. */
  reachable: boolean;
  /** Round-trip time of the last status check, for the startup screen and auth island. */
  latencyMs: number | null;
  /** Re-run the status check (the startup screen's "Try again"). */
  retry: () => void;
  login: (password: string) => Promise<AuthState>;
  setup: (bootstrapToken: string, password: string) => Promise<AuthState>;
  logout: () => Promise<void>;
  /** Session-expired is a server-driven transition (token TTL); exposed for the route guard to trigger. */
  markExpired: () => void;
  /** Client-side lock countdown reaching zero — resets to a normal signed-out state. */
  clearLock: () => void;
}

/** How long the success mark animation (dot grows to fill the frame) plays before the route guard redirects. */
const SUCCESS_ANIMATION_MS = 550;

const AuthContext = createContext<AuthContextValue | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<AuthState>({ status: "signedOut", attempts: 0, lockUntil: null });
  const [loading, setLoading] = useState(true);
  const [reachable, setReachable] = useState(true);
  const [latencyMs, setLatencyMs] = useState<number | null>(null);
  const [checkId, setCheckId] = useState(0);

  useEffect(() => {
    let cancelled = false;
    const started = performance.now();
    setLoading(true);
    dataAdapter.auth
      .getState()
      .then((next) => {
        if (cancelled) return;
        setState(next);
        setReachable(true);
        setLatencyMs(Math.round(performance.now() - started));
      })
      .catch(() => {
        if (cancelled) return;
        setState({ status: "signedOut", attempts: 0, lockUntil: null, message: "Could not reach the daemon." });
        setReachable(false);
        setLatencyMs(null);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [checkId]);

  const retry = useCallback(() => setCheckId((id) => id + 1), []);

  useEffect(() => {
    const expire = () => setState({ status: "expired", attempts: 0, lockUntil: null });
    window.addEventListener(AUTH_EXPIRED_EVENT, expire);
    return () => window.removeEventListener(AUTH_EXPIRED_EVENT, expire);
  }, []);

  // On success, the promise resolves immediately (so the caller can start the
  // mark's success animation) but committing `signedIn` into context — which is
  // what the route guard reacts to — waits for that animation to finish.
  const login = useCallback(async (password: string) => {
    const next = await dataAdapter.auth.login(password);
    if (next.status === "signedIn") {
      setTimeout(() => setState(next), SUCCESS_ANIMATION_MS);
    } else {
      setState(next);
    }
    return next;
  }, []);

  const setup = useCallback(async (bootstrapToken: string, password: string) => {
    const next = await dataAdapter.auth.setup(bootstrapToken, password);
    if (next.status === "signedIn") {
      setTimeout(() => setState(next), SUCCESS_ANIMATION_MS);
    } else {
      setState(next);
    }
    return next;
  }, []);

  const logout = useCallback(async () => {
    await dataAdapter.auth.logout();
    setState({ status: "signedOut", attempts: 0, lockUntil: null });
  }, []);

  const markExpired = useCallback(() => {
    setState((s) => ({ ...s, status: "expired" }));
  }, []);

  const clearLock = useCallback(() => {
    setState({ status: "signedOut", attempts: 0, lockUntil: null });
  }, []);

  const value = useMemo<AuthContextValue>(
    () => ({ ...state, loading, reachable, latencyMs, retry, login, setup, logout, markExpired, clearLock }),
    [state, loading, reachable, latencyMs, retry, login, setup, logout, markExpired, clearLock],
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used within AuthProvider");
  return ctx;
}
