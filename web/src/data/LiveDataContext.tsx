import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

import { dataAdapter } from "./index";
import { mergeProgress } from "./format";
import { latestGenerations } from "./lineage";
import { markLoad } from "./load-timing";
import { withMachineReach } from "./machine-status";
import type { LiveEvent } from "./adapter";
import type { Machine, Session } from "./types";

export interface LiveDataValue {
  machines: Machine[];
  /** Every Session, including earlier generations of continued terminals. */
  sessions: Session[];
  /** One entry per terminal (its latest generation): what lists and counts show. */
  terminals: Session[];
  /** Terminals the owner hid: left out of `terminals`, shown on the Hidden page. */
  hidden: Session[];
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
            progress: mergeProgress(prev[idx].progress, event.session.progress),
            // A question that is over must never be answered as if still open.
            prompt: event.session.state === "WAITING" ? event.session.prompt ?? prev[idx].prompt : undefined,
            promptOptions: event.session.state === "WAITING"
              ? event.session.promptOptions ?? prev[idx].promptOptions
              : undefined,
            details: prev[idx].details ?? event.session.details,
            metrics: event.session.metrics ?? prev[idx].metrics,
            training: event.session.training ?? prev[idx].training,
          };
          return next;
        });
      } else if (event.type === "SESSIONS_REMOVED") {
        const removed = new Set(event.sessionIds);
        setSessions((prev) => prev.filter((session) => !removed.has(session.id)));
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
        setSessions((prev) => prev.map((session) => {
          if (session.id !== event.sessionId) return session;
          // A new Command: the last one's counts are not this one's progress.
          const started = event.command !== undefined && event.command !== session.command;
          return started
            ? { ...session, command: event.command, progress: undefined }
            : { ...session, command: event.command };
        }));
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

  // Sessions on a Machine that is not online have an unknown state (X24).
  const shown = useMemo(() => withMachineReach(sessions, machines), [sessions, machines]);
  const latest = useMemo(() => latestGenerations(shown), [shown]);
  const terminals = useMemo(() => latest.filter((session) => !session.dismissed), [latest]);
  const hidden = useMemo(() => latest.filter((session) => session.dismissed), [latest]);
  const value = useMemo<LiveDataValue>(
    () => ({ machines, sessions: shown, terminals, hidden, loading }),
    [machines, shown, terminals, hidden, loading],
  );

  return <LiveDataContext.Provider value={value}>{children}</LiveDataContext.Provider>;
}

/** Live data when inside the provider, otherwise null (components used on their own). */
export function useOptionalLiveData(): LiveDataValue | null {
  return useContext(LiveDataContext);
}

export function useLiveData(): LiveDataValue {
  const ctx = useContext(LiveDataContext);
  if (!ctx) throw new Error("useLiveData must be used within LiveDataProvider");
  return ctx;
}
