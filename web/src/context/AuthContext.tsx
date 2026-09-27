import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

import { dataAdapter } from "../data";
import type { AuthState } from "../data/types";

interface AuthContextValue extends AuthState {
  loading: boolean;
  login: (password: string) => Promise<AuthState>;
  setup: (password: string) => Promise<AuthState>;
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

  useEffect(() => {
    let cancelled = false;
    dataAdapter.auth.getState().then((next) => {
      if (!cancelled) {
        setState(next);
        setLoading(false);
      }
    });
    return () => {
      cancelled = true;
    };
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

  const setup = useCallback(async (password: string) => {
    const next = await dataAdapter.auth.setup(password);
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
    () => ({ ...state, loading, login, setup, logout, markExpired, clearLock }),
    [state, loading, login, setup, logout, markExpired, clearLock],
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used within AuthProvider");
  return ctx;
}
