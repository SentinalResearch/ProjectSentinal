import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

import { dataAdapter } from "./index";
import { latestGenerations } from "./lineage";
import { markLoad } from "./load-timing";
import type { LiveEvent } from "./adapter";
import type { Machine, Session } from "./types";

export interface LiveDataValue {
  machines: Machine[];
  /** Every Session, including earlier generations of continued terminals. */
  sessions: Session[];
  /** One entry per terminal (its latest generation): what lists and counts show. */
  terminals: Session[];
  /** True until the first getMachines()/getSessions() round-trip resolves. */
  loading: boolean;
}

const LiveDataContext = createContext<LiveDataValue | null>(null);

/**
 * Single shared source of machines/sessions for everything under the app shell.
 * Fetches once, then applies `dataAdapter.subscribe` live events (SESSION_UPDATED,
 * EVENT_APPENDED, MACHINE_UPDATED) by id so every consumer sees the same state
 * without re-fetching or missing an update. NOTIFICATION events aren't consumed
 * here yet — see FUTURE.md's "What happened while I was gone?" journal row.
 */
export function LiveDataProvider({ children }: { children: ReactNode }) {
  const [machines, setMachines] = useState<Machine[]>([]);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;

    Promise.all([dataAdapter.getMachines(), dataAdapter.getSessions()]).then(([m, s]) => {
      if (cancelled) return;
      setMachines(m);
      setSessions(s);
      setLoading(false);
      markLoad("shown");
    });

    const unsubscribe = dataAdapter.subscribe((event: LiveEvent) => {
      setLoading(false);
      if (event.type === "DATA_RESET") {
        setMachines(event.machines);
        setSessions(event.sessions);
      } else if (event.type === "SESSION_UPDATED") {
        setSessions((prev) => {
          const idx = prev.findIndex((s) => s.id === event.session.id);
          if (idx < 0) return [...prev, event.session];
          const next = prev.slice();
          next[idx] = {
            ...prev[idx],
            ...event.session,
            events: prev[idx].events,
            progress: event.session.progress ?? prev[idx].progress,
            prompt: event.session.prompt ?? prev[idx].prompt,
            details: prev[idx].details ?? event.session.details,
            metrics: event.session.metrics ?? prev[idx].metrics,
            training: event.session.training ?? prev[idx].training,
          };
          return next;
        });
      } else if (event.type === "MACHINE_UPDATED") {
        setMachines((prev) => {
          const idx = prev.findIndex((m) => m.id === event.machine.id);
          if (idx < 0) return [...prev, event.machine];
          const next = prev.slice();
          next[idx] = event.machine;
          return next;
        });
      } else if (event.type === "EVENT_APPENDED") {
        setSessions((prev) =>
          prev.map((s) => (s.id === event.sessionId
            ? { ...s, ...event.sessionPatch, events: [...(s.events ?? []), event.event] }
            : s)),
        );
      } else if (event.type === "COMMAND_UPDATED") {
        setSessions((prev) => prev.map((session) =>
          session.id === event.sessionId ? { ...session, command: event.command } : session,
        ));
      } else if (event.type === "SESSION_METRICS") {
        setSessions((prev) => prev.map((session) =>
          session.id === event.sessionId ? { ...session, metrics: { ...session.metrics, ...event.metrics } } : session,
        ));
      }
    });

    return () => {
      cancelled = true;
      unsubscribe();
    };
  }, []);

  const terminals = useMemo(() => latestGenerations(sessions), [sessions]);
  const value = useMemo<LiveDataValue>(
    () => ({ machines, sessions, terminals, loading }),
    [machines, sessions, terminals, loading],
  );

  return <LiveDataContext.Provider value={value}>{children}</LiveDataContext.Provider>;
}

export function useLiveData(): LiveDataValue {
  const ctx = useContext(LiveDataContext);
  if (!ctx) throw new Error("useLiveData must be used within LiveDataProvider");
  return ctx;
}
