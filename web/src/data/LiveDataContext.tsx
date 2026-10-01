import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

import { dataAdapter } from "./index";
import type { LiveEvent } from "./adapter";
import type { Machine, Session } from "./types";
import type { DaemonMessage } from "../protocol/DaemonMessage";
import { DAEMON_MESSAGE_EVENT } from "../terminal/useRealTerminalSession";
import { mapWatcherEvent, watcherEventPatch } from "./api";

export interface LiveDataValue {
  machines: Machine[];
  sessions: Session[];
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
    });

    const unsubscribe = dataAdapter.subscribe((event: LiveEvent) => {
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

    let latestGpuPct: number | undefined;
    let latestVramTotal = 0;
    const onDaemonMessage = (raw: Event) => {
      const message = (raw as CustomEvent<DaemonMessage>).detail;
      if (message.type === "MACHINE_METRICS") {
        latestGpuPct = message.metrics.gpus.reduce<number | undefined>(
          (highest, gpu) => highest === undefined ? gpu.util_percent : Math.max(highest, gpu.util_percent),
          undefined,
        );
        latestVramTotal = message.metrics.gpus.reduce((total, gpu) => total + gpu.vram_total_bytes, 0);
      } else if (message.type === "SESSION_METRICS") {
        const totals = message.metrics.totals;
        setSessions((prev) => prev.map((session) => session.id === String(message.metrics.session_id)
          ? {
              ...session,
              metrics: {
                ...session.metrics,
                cpuPct: Math.round(totals.cpu_percent * 10) / 10,
                ramGB: Math.round((totals.rss_bytes / 1024 ** 3) * 10) / 10,
                gpuPct: totals.gpu_mem_bytes > 0 ? latestGpuPct : undefined,
                vramGB: latestVramTotal > 0 || totals.gpu_mem_bytes > 0
                  ? Math.round((totals.gpu_mem_bytes / 1024 ** 3) * 10) / 10
                  : undefined,
                vramTotalGB: latestVramTotal > 0 ? Math.round((latestVramTotal / 1024 ** 3) * 10) / 10 : undefined,
              },
            }
          : session));
      } else if (message.type === "COMMAND_UPDATED") {
        setSessions((prev) => prev.map((session) => session.id === String(message.command.session_id)
          ? { ...session, command: message.command.finished_at === null ? message.command.text : undefined }
          : session));
      } else if (message.type === "EVENT") {
        const event = message.event;
        setSessions((prev) => prev.map((session) => session.id === String(event.session_id)
          ? {
              ...session,
              ...watcherEventPatch(event),
              events: [...(session.events ?? []), mapWatcherEvent(event)],
            }
          : session));
      }
    };
    window.addEventListener(DAEMON_MESSAGE_EVENT, onDaemonMessage);

    return () => {
      cancelled = true;
      unsubscribe();
      window.removeEventListener(DAEMON_MESSAGE_EVENT, onDaemonMessage);
    };
  }, []);

  const value = useMemo<LiveDataValue>(() => ({ machines, sessions, loading }), [machines, sessions, loading]);

  return <LiveDataContext.Provider value={value}>{children}</LiveDataContext.Provider>;
}

export function useLiveData(): LiveDataValue {
  const ctx = useContext(LiveDataContext);
  if (!ctx) throw new Error("useLiveData must be used within LiveDataProvider");
  return ctx;
}
